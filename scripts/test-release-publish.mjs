import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { copyFileSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { releaseVersion, resolveRelease, sourceVersions, validateRelease } from './release-publish.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const sourceSha = 'a'.repeat(40);
const identity = { tag: 'v0.1.0', version: '0.1.0', repository: 'stevenvaughanbooth/adage', sourceSha, prerelease: false };
const context = { tag: identity.tag, repository: identity.repository, mode: 'automatic', ref: 'refs/tags/v0.1.0', sha: sourceSha };
const versions = Object.fromEntries(['workspace', 'npm-cli', 'npm-platform', 'plugin', 'marketplace', 'marketplace-plugin']
  .map((name) => [name, '0.1.0']));
const manifest = () => ({
  announcement_tag: identity.tag,
  announcement_is_prerelease: false,
  releases: ['adage-cli', 'adage-exec', 'adage-train'].map((app_name) => ({
    app_name, app_version: '0.1.0',
    hosting: { github: { owner: 'stevenvaughanbooth', repo: 'adage', artifact_base_url: 'https://github.com',
      artifact_download_path: '/stevenvaughanbooth/adage/releases/download/v0.1.0' } },
  })),
});
const check = (value, overrides = {}) => validateRelease({ identity, sourceSha, versions, manifest: value, ...overrides });

function api({ annotated = false, release = {}, commit = sourceSha } = {}) {
  return (endpoint) => {
    if (endpoint.endsWith('/git/ref/tags/v0.1.0')) return { ref: 'refs/tags/v0.1.0',
      object: { type: annotated ? 'tag' : 'commit', sha: annotated ? 'b'.repeat(40) : commit } };
    if (endpoint.endsWith(`/git/tags/${'b'.repeat(40)}`)) return { object: { type: 'commit', sha: commit } };
    if (endpoint.endsWith('/releases/tags/v0.1.0')) return {
      tag_name: 'v0.1.0', draft: false, prerelease: false, published_at: '2026-10-16T00:00:00Z', ...release,
    };
    throw new Error('unexpected API endpoint');
  };
}

test('only canonical release tags reach the GitHub API', () => {
  for (const tag of ['main', '../v0.1.0', '--help', 'v0.1.0\nBAD=1', 'v0.1.0;echo bad', 'v01.1.0',
    'v0.1.0-01', 'v0.1.0+build', `v0.1.0-${'a'.repeat(128)}`]) {
    assert.throws(() => resolveRelease({ ...context, tag }, () => assert.fail('API must not run')), /release_tag/);
  }
  assert.equal(releaseVersion('v1.2.3-rc.1'), '1.2.3-rc.1');
});

test('both lightweight and annotated tags resolve to the exact source commit', () => {
  assert.deepEqual(resolveRelease(context, api()), identity);
  assert.deepEqual(resolveRelease(context, api({ annotated: true })), identity);
  assert.deepEqual(resolveRelease({ ...context, mode: 'recovery', ref: 'refs/heads/main', sha: 'c'.repeat(40) }, api()), identity);
});

test('wrong workflow refs, moved tags and unpublished releases stop before staging', () => {
  assert.throws(() => resolveRelease({ ...context, mode: 'recovery' }, api()), /from main/);
  assert.throws(() => resolveRelease({ ...context, ref: 'refs/heads/main' }, api()), /from the release tag/);
  assert.throws(() => resolveRelease(context, api({ commit: 'c'.repeat(40) })), /tag moved/);
  for (const release of [{ draft: true }, { published_at: null }, { tag_name: 'v0.2.0' }]) {
    assert.throws(() => resolveRelease(context, api({ release })), /published GitHub Release/);
  }
  assert.throws(() => resolveRelease(context, api({ release: { prerelease: true } })), /prerelease/);
});

test('tag, all three binary releases, hosting, source and every package version must agree', () => {
  assert.equal(check(manifest()), '0.1.0');
  assert.throws(() => check({ ...manifest(), announcement_tag: 'v0.2.0' }), /manifest tag/);
  assert.throws(() => check({ ...manifest(), announcement_is_prerelease: true }), /prerelease/);
  assert.throws(() => check(manifest(), { sourceSha: 'c'.repeat(40) }), /checked-out source/);
  for (let index = 0; index < 3; index++) {
    const value = manifest();
    value.releases[index].app_version = '0.2.0';
    assert.throws(() => check(value), /manifest version/);
    value.releases[index].app_version = '0.1.0';
    value.releases[index].hosting.github.artifact_download_path = '/another/repo/releases/download/v0.1.0';
    assert.throws(() => check(value), /different release/);
  }
  const duplicate = manifest();
  duplicate.releases.push(duplicate.releases[0]);
  assert.throws(() => check(duplicate), /exactly one adage-cli/);
  const missing = manifest();
  missing.releases.pop();
  assert.throws(() => check(missing), /exactly one adage-train/);
  for (const file of Object.keys(versions)) {
    assert.throws(() => check(manifest(), { versions: { ...versions, [file]: '0.2.0' } }), /version disagrees/);
  }
});

test('a manifest larger than the per-environment-variable limit validates through a real subprocess', () => {
  const temp = mkdtempSync(join(tmpdir(), 'adage-publisher-test-'));
  try {
    const actualVersions = sourceVersions(root);
    const version = actualVersions['Cargo.toml'];
    const tag = `v${version}`;
    const head = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim();
    const value = manifest();
    value.announcement_tag = tag;
    value.announcement_is_prerelease = version.includes('-');
    value.announcement_github_body = 'x'.repeat(256 * 1024);
    for (const release of value.releases) {
      release.app_version = version;
      release.hosting.github.artifact_download_path = `/${identity.repository}/releases/download/${tag}`;
    }
    const state = join(temp, 'identity.json');
    const manifestPath = join(temp, 'dist-manifest.json');
    const envFile = join(temp, 'env');
    const helper = join(temp, 'release-publish.mjs');
    copyFileSync(join(root, 'scripts/release-publish.mjs'), helper);
    writeFileSync(state, JSON.stringify({ ...identity, tag, version, sourceSha: head, prerelease: version.includes('-') }));
    writeFileSync(manifestPath, JSON.stringify(value));
    const run = () => spawnSync(process.execPath, [helper, 'validate', state, manifestPath], {
      cwd: root, encoding: 'utf8', env: { PATH: process.env.PATH, GITHUB_ENV: envFile },
    });
    const result = run();
    assert.equal(result.status, 0, result.stderr);
    assert.equal(readFileSync(envFile, 'utf8'), `VERSION=${version}\n`);
    value.releases[1].app_version = '999.0.0';
    writeFileSync(manifestPath, JSON.stringify(value));
    const rejected = run();
    assert.equal(rejected.status, 1);
    assert.match(rejected.stderr, /adage-exec manifest version disagrees/);
    assert.equal(readFileSync(envFile, 'utf8'), `VERSION=${version}\n`, 'a rejected manifest must not export a version');
  } finally { rmSync(temp, { recursive: true, force: true }); }
});

test('both publishers read manifest files and pin recovery source without changing provenance context', () => {
  for (const file of ['publish-npm.yml', 'publish-marketplace.yml']) {
    const workflow = readFileSync(join(root, '.github/workflows', file), 'utf8');
    assert.doesNotMatch(workflow, /\bPLAN:/);
    assert.doesNotMatch(workflow, /\$\{\{\s*inputs\.plan\s*\}\}/);
    assert.match(workflow, /manifest=artifacts\/plan-dist-manifest\.json/);
    assert.match(workflow, /manifest=artifacts\/dist-manifest\.json/);
    assert.match(workflow, /ref: \$\{\{ steps\.release\.outputs\.source_sha \}\}/);
    assert.match(workflow, /cp scripts\/release-publish\.mjs "\$RUNNER_TEMP\/release-publish\.mjs"/);
    assert.doesNotMatch(workflow, /(?:GITHUB_SHA|GITHUB_REF)\s*[:=]/);
  }
});
