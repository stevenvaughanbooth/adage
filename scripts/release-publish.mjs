#!/usr/bin/env node
// Standalone: the workflow preserves this helper in RUNNER_TEMP before checking out the release.
// Large dist manifests stay in files, never process arguments or environment variables.
import { appendFileSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const apps = ['adage-cli', 'adage-exec', 'adage-train'];
const shaPattern = /^[0-9a-f]{40}$/;
const tagPattern = /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*))?$/;

function requireThat(condition, message) {
  if (!condition) throw new Error(message);
}

export function releaseVersion(tag) {
  requireThat(typeof tag === 'string' && tag.length <= 128 && tagPattern.test(tag),
    'release_tag must be a canonical vMAJOR.MINOR.PATCH tag, optionally with a prerelease');
  return tag.slice(1);
}

export function resolveRelease({ tag, repository, mode, ref, sha }, api) {
  const version = releaseVersion(tag);
  requireThat(typeof repository === 'string' && /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository),
    'invalid GitHub repository');
  requireThat(mode === 'automatic' || mode === 'recovery', 'invalid release mode');
  if (mode === 'recovery') requireThat(ref === 'refs/heads/main', 'dispatch the repaired workflow from main');
  else requireThat(ref === `refs/tags/${tag}`, 'automatic publication must run from the release tag');

  const prefix = `repos/${repository}`;
  const tagRef = api(`${prefix}/git/ref/tags/${tag}`);
  requireThat(tagRef.ref === `refs/tags/${tag}`, 'GitHub returned a different tag');
  let object = tagRef.object;
  for (let depth = 0; object?.type === 'tag' && depth < 5; depth++) {
    requireThat(shaPattern.test(object.sha), 'invalid annotated tag SHA');
    object = api(`${prefix}/git/tags/${object.sha}`).object;
  }
  requireThat(object?.type === 'commit' && shaPattern.test(object.sha), 'tag does not resolve to a commit');
  if (mode === 'automatic') requireThat(object.sha === sha, 'release tag moved since this workflow started');

  const release = api(`${prefix}/releases/tags/${tag}`);
  const prerelease = version.includes('-');
  requireThat(release.tag_name === tag && release.draft === false && Boolean(release.published_at),
    'a published GitHub Release for this exact tag is required');
  requireThat(release.prerelease === prerelease, 'GitHub Release prerelease status disagrees with the tag');
  return { tag, version, repository, sourceSha: object.sha, prerelease };
}

export function sourceVersions(root) {
  const json = (path) => JSON.parse(readFileSync(resolve(root, path), 'utf8'));
  const cargo = readFileSync(resolve(root, 'Cargo.toml'), 'utf8');
  const section = cargo.split(/^\[workspace\.package\]\s*$/m)[1]?.split(/^\[/m)[0];
  const version = section?.match(/^version\s*=\s*"([^"]+)"\s*(?:#.*)?$/m)?.[1];
  requireThat(version, 'workspace package version is missing');
  const marketplace = json('.claude-plugin/marketplace.json');
  requireThat(Array.isArray(marketplace.plugins) && marketplace.plugins.length > 0, 'marketplace plugins are missing');
  return {
    'Cargo.toml': version,
    'npm/cli/package.json': json('npm/cli/package.json').version,
    'npm/platform/package.json': json('npm/platform/package.json').version,
    'plugin/claude-code/.claude-plugin/plugin.json': json('plugin/claude-code/.claude-plugin/plugin.json').version,
    '.claude-plugin/marketplace.json metadata': marketplace.metadata?.version,
    ...Object.fromEntries(marketplace.plugins.map((plugin, index) => [`marketplace plugin ${index}`, plugin.version])),
  };
}

export function validateRelease({ identity, manifest, sourceSha, versions }) {
  const version = releaseVersion(identity.tag);
  requireThat(identity.version === version && shaPattern.test(identity.sourceSha), 'invalid resolved release identity');
  requireThat(sourceSha === identity.sourceSha, 'checked-out source is not the resolved release commit');
  requireThat(manifest.announcement_tag === identity.tag, 'manifest tag disagrees with the release tag');
  requireThat(manifest.announcement_is_prerelease === identity.prerelease,
    'manifest prerelease status disagrees with the GitHub Release');
  requireThat(Array.isArray(manifest.releases), 'manifest releases are missing');
  const [owner, repo] = identity.repository.split('/');
  for (const app of apps) {
    const matches = manifest.releases.filter((release) => release.app_name === app);
    requireThat(matches.length === 1, `manifest must contain exactly one ${app} release`);
    const release = matches[0];
    requireThat(release.app_version === version, `${app} manifest version disagrees with the release tag`);
    const hosting = release.hosting?.github;
    requireThat(hosting?.owner === owner && hosting.repo === repo && hosting.artifact_base_url === 'https://github.com'
      && hosting.artifact_download_path === `/${identity.repository}/releases/download/${identity.tag}`,
    `${app} manifest points to a different release`);
  }
  requireThat(Object.keys(versions).length >= 6, 'source version checks are incomplete');
  for (const [file, value] of Object.entries(versions)) {
    requireThat(value === version, `${file} version disagrees with the release tag`);
  }
  return version;
}

function main() {
  const [command, statePath, manifestPath] = process.argv.slice(2);
  requireThat(statePath, 'usage: release-publish.mjs resolve <state> | validate <state> <manifest>');
  if (command === 'resolve') {
    const identity = resolveRelease({
      tag: process.env.RELEASE_TAG,
      repository: process.env.GITHUB_REPOSITORY,
      mode: process.env.RELEASE_MODE,
      ref: process.env.GITHUB_REF,
      sha: process.env.GITHUB_SHA,
    }, (endpoint) => JSON.parse(execFileSync('gh', ['api', endpoint], { encoding: 'utf8' })));
    writeFileSync(statePath, `${JSON.stringify(identity)}\n`);
    requireThat(process.env.GITHUB_OUTPUT, 'GITHUB_OUTPUT is required');
    appendFileSync(process.env.GITHUB_OUTPUT, `source_sha=${identity.sourceSha}\n`);
    console.log(`Resolved ${identity.tag} to ${identity.sourceSha}`);
  } else if (command === 'validate') {
    requireThat(manifestPath, 'manifest file path is required');
    const identity = JSON.parse(readFileSync(statePath, 'utf8'));
    const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
    const sourceSha = execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
    const version = validateRelease({ identity, manifest, sourceSha, versions: sourceVersions(process.cwd()) });
    requireThat(process.env.GITHUB_ENV, 'GITHUB_ENV is required');
    appendFileSync(process.env.GITHUB_ENV, `VERSION=${version}\n`);
    console.log(`Verified ${identity.tag}: source ${sourceSha}, all binary and package versions agree`);
  } else throw new Error('expected resolve or validate');
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) { console.error(`release-publish: ${error.message}`); process.exitCode = 1; }
}
