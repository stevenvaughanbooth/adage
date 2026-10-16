/**
 * Golden suite for the shell normalizer.
 *
 * 71% of tool calls in the corpus are Bash, so this is where regressions hurt most.
 * Run: node --experimental-strip-types --test test/shell.test.ts
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { normalizeShell } from "../src/normalize/shell.ts";

const one = (cmd: string) => {
  const r = normalizeShell(cmd, "/repo");
  assert.equal(r.length, 1, `expected 1 action from: ${cmd}, got ${r.length}`);
  return r[0];
};

test("cat and nl become reads", () => {
  assert.deepEqual(one("cat src/a.ts").action, { op: "read", path: "/repo/src/a.ts" });
  assert.deepEqual(one("nl -ba src/a.ts").action, { op: "read", path: "/repo/src/a.ts" });
});

test("sed/head/tail carry line ranges", () => {
  assert.deepEqual(one("sed -n '10,40p' src/a.ts").action,
    { op: "read", path: "/repo/src/a.ts", range: [10, 40] });
  assert.deepEqual(one("head -n 40 src/a.ts").action,
    { op: "read", path: "/repo/src/a.ts", range: [1, 40] });
});

test("value-taking flags are command-specific", () => {
  // -n is a boolean for rg but takes a value for head; a shared table gets this wrong
  const a = one('rg -n "TODO" src/').action as any;
  assert.equal(a.op, "search");
  assert.equal(a.pattern, "TODO");
  assert.deepEqual(a.paths, ["/repo/src"]);
});

test("cd updates cwd for what follows", () => {
  assert.deepEqual(one("cd pkg/core && cat a.ts").action,
    { op: "read", path: "/repo/pkg/core/a.ts" });
});

test("bash -lc is unwrapped", () => {
  assert.deepEqual(one(`bash -lc "cat src/a.ts"`).action, { op: "read", path: "/repo/src/a.ts" });
});

test("read-only pipelines stay read-only", () => {
  assert.equal(one("git log --oneline | head -20").mutating, false);
});

test("redirects and in-place edits are mutating", () => {
  assert.equal(one("echo hi > out.txt").mutating, true);
  const r = one("sed -i 's/a/b/' src/a.ts");
  assert.equal(r.mutating, true);
  assert.equal(r.action.op, "edit");
});

test("git read vs write is classified", () => {
  assert.equal((one("git status").action as any).class, "vcs-read");
  const w = one("git commit -m x");
  assert.equal((w.action as any).class, "vcs-write");
  assert.equal(w.mutating, true);
});

test("package-manager subcommands split by effect", () => {
  assert.equal((one("npm test").action as any).class, "test");
  assert.equal(one("npm install lodash").mutating, true);
});

test("repo scripts are lookup candidates", () => {
  assert.equal((one("./scripts/wi-lookup W-1234 --json").action as any).class, "lookup");
});

test("compound constructs are opaque, not shredded", () => {
  const r = normalizeShell("for f in a b; do echo $f; done", "/repo");
  assert.equal(r.length, 1, "a for-loop must not split into fragments");
  assert.ok(r[0].confidence < 0.5, "and must report low confidence");
});

test("variable assignment prefixes are stripped", () => {
  const a = one("UA=Mozilla curl -sL https://example.com").action as any;
  assert.equal(a.op, "fetch");
  assert.equal(a.url, "https://example.com");
});

test("sequences yield multiple actions in order", () => {
  const r = normalizeShell("cat a.ts && cat b.ts", "/repo");
  assert.equal(r.length, 2);
  assert.deepEqual(r.map((x) => (x.action as any).path), ["/repo/a.ts", "/repo/b.ts"]);
});
