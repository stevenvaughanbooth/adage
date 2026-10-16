/**
 * Shell → canonical action.
 *
 * 71% of tool calls in the sample corpus are Bash, so this is the critical path of the
 * whole normalizer, not a subsection of it. Production should use tree-sitter-bash; this
 * is a dependency-free tokenizer that handles the shapes that actually occur, and reports
 * confidence so we can measure how much it is missing.
 */

import type { CanonicalAction, ExecClass, Normalized } from "../types.ts";

/* ------------------------------------------------------------------ tokenize */

/** Split a command string into tokens, respecting quotes. Keeps operators as tokens. */
export function tokenize(src: string): string[] {
  const out: string[] = [];
  let cur = "";
  let quote: '"' | "'" | null = null;
  let i = 0;

  const flush = () => { if (cur) { out.push(cur); cur = ""; } };

  while (i < src.length) {
    const c = src[i];

    if (quote) {
      if (c === "\\" && quote === '"' && i + 1 < src.length) { cur += src[i + 1]; i += 2; continue; }
      if (c === quote) { quote = null; i++; continue; }
      cur += c; i++; continue;
    }

    if (c === '"' || c === "'") { quote = c; i++; continue; }
    if (c === "\\" && i + 1 < src.length) { cur += src[i + 1]; i += 2; continue; }

    if (/\s/.test(c)) { flush(); i++; continue; }

    // operators
    const two = src.slice(i, i + 2);
    if (two === "&&" || two === "||" || two === ">>" || two === "2>") {
      flush(); out.push(two); i += 2; continue;
    }
    if (c === "|" || c === ";" || c === ">" || c === "<" || c === "(" || c === ")") {
      flush(); out.push(c); i++; continue;
    }

    cur += c; i++;
  }
  flush();
  return out;
}

const SEQ_OPS = new Set(["&&", "||", ";"]);
const REDIRECTS = new Set([">", ">>", "2>"]);

/** Split a token stream into sequential command segments (&&, ;, ||). */
function splitSequence(tokens: string[]): string[][] {
  const segs: string[][] = [];
  let cur: string[] = [];
  for (const t of tokens) {
    if (SEQ_OPS.has(t)) { if (cur.length) segs.push(cur); cur = []; }
    else cur.push(t);
  }
  if (cur.length) segs.push(cur);
  return segs;
}

/** Split one segment on pipes into pipeline stages. */
function splitPipeline(tokens: string[]): string[][] {
  const stages: string[][] = [];
  let cur: string[] = [];
  for (const t of tokens) {
    if (t === "|") { if (cur.length) stages.push(cur); cur = []; }
    else cur.push(t);
  }
  if (cur.length) stages.push(cur);
  return stages;
}

/* ---------------------------------------------------------------- classify */

/** Trailing pipeline stages that only filter keep a pipeline read-only. */
const PURE_FILTERS = new Set([
  "head", "tail", "grep", "rg", "jq", "sed", "wc", "sort", "uniq", "cut", "awk",
  "tr", "column", "tee-less", "cat", "xargs-less", "python3", "node",
]);

const MUTATING_CMDS = new Set([
  "rm", "mv", "cp", "chmod", "chown", "mkdir", "touch", "ln", "tee", "dd",
  "npm", "pnpm", "yarn", "pip", "pip3", "brew", "apt", "cargo", "go",
]);

const GIT_WRITE = new Set([
  "add", "commit", "push", "pull", "checkout", "reset", "merge", "rebase",
  "cherry-pick", "revert", "stash", "tag", "branch", "clean", "apply", "am",
]);

const GIT_READ = new Set([
  "status", "log", "show", "diff", "blame", "describe", "rev-parse",
  "ls-files", "remote", "config", "reflog", "shortlog",
]);

const TEST_CMDS = new Set(["jest", "vitest", "pytest", "mocha", "ava", "tap", "phpunit", "rspec"]);
const BUILD_CMDS = new Set(["tsc", "make", "cmake", "webpack", "vite", "esbuild", "rollup", "gradle", "mvn"]);
const LINT_CMDS = new Set(["eslint", "prettier", "ruff", "flake8", "black", "clippy", "shellcheck"]);

function classifyExec(argv: string[]): { cls: ExecClass; mutating: boolean } {
  const cmd = basename(argv[0] ?? "");
  const sub = argv[1] ?? "";

  if (cmd === "git") {
    if (GIT_READ.has(sub)) return { cls: "vcs-read", mutating: false };
    if (GIT_WRITE.has(sub)) return { cls: "vcs-write", mutating: true };
    return { cls: "vcs-read", mutating: false };
  }

  // package-manager subcommands: `npm test` reads, `npm install` mutates
  if (["npm", "pnpm", "yarn", "bun"].includes(cmd)) {
    if (sub === "test" || (sub === "run" && /test/.test(argv[2] ?? ""))) return { cls: "test", mutating: false };
    if (sub === "run" && /build|compile/.test(argv[2] ?? "")) return { cls: "build", mutating: false };
    if (sub === "run" && /lint|format/.test(argv[2] ?? "")) return { cls: "lint", mutating: false };
    if (["install", "i", "add", "remove", "uninstall", "update"].includes(sub)) return { cls: "install", mutating: true };
    return { cls: "other", mutating: false };
  }

  if (cmd === "go" || cmd === "cargo") {
    if (sub === "test") return { cls: "test", mutating: false };
    if (sub === "build") return { cls: "build", mutating: false };
    return { cls: "other", mutating: sub === "get" || sub === "install" };
  }

  if (TEST_CMDS.has(cmd)) return { cls: "test", mutating: false };
  if (BUILD_CMDS.has(cmd)) return { cls: "build", mutating: false };
  if (LINT_CMDS.has(cmd)) return { cls: "lint", mutating: false };
  if (MUTATING_CMDS.has(cmd)) return { cls: "mutating", mutating: true };

  // repo-local script: the prime compilation candidate
  if (argv[0]?.startsWith("./") || argv[0]?.startsWith("scripts/") || argv[0]?.startsWith("tools/")) {
    return { cls: "lookup", mutating: false };
  }

  return { cls: "other", mutating: false };
}

function basename(p: string): string {
  const i = p.lastIndexOf("/");
  return i === -1 ? p : p.slice(i + 1);
}

/* ------------------------------------------------------------- read shapes */

/** `sed -n '10,40p' f` / `head -n 40 f` / `tail -n +10 f` → read with a range. */
function readRange(argv: string[]): [number, number] | undefined {
  const cmd = basename(argv[0]);
  if (cmd === "sed") {
    const m = argv.join(" ").match(/(\d+),(\d+)p/);
    if (m) return [Number(m[1]), Number(m[2])];
  }
  if (cmd === "head") {
    const i = argv.indexOf("-n");
    if (i >= 0 && argv[i + 1]) return [1, Number(argv[i + 1])];
  }
  if (cmd === "tail") {
    const i = argv.indexOf("-n");
    const v = argv[i + 1] ?? "";
    if (v.startsWith("+")) return [Number(v.slice(1)), Number.MAX_SAFE_INTEGER];
  }
  return undefined;
}

/**
 * Flags that consume the following token, per command.
 *
 * This is command-specific and getting it wrong silently corrupts templates: `-n` takes a
 * value for `head` but is a boolean for `rg`, so a shared table parses
 * `rg -n "TODO" src/` as a search for "src/".
 */
const VALUE_FLAGS: Record<string, Set<string>> = {
  grep: new Set(["-e", "-A", "-B", "-C", "-m", "-f", "--max-count", "--include", "--exclude"]),
  rg:   new Set(["-e", "-A", "-B", "-C", "-m", "-t", "-T", "-g", "--type", "--max-count", "--glob"]),
  head: new Set(["-n", "-c"]),
  tail: new Set(["-n", "-c"]),
  sed:  new Set(["-e", "-f"]),
  find: new Set(["-name", "-iname", "-type", "-path", "-maxdepth", "-mindepth"]),
  fd:   new Set(["-e", "-t", "-d", "--extension", "--type"]),
};

/** Non-flag operands, respecting the command's own flag grammar. */
function operands(argv: string[]): string[] {
  const valueFlags = VALUE_FLAGS[basename(argv[0] ?? "")] ?? new Set<string>();
  const out: string[] = [];
  for (let i = 1; i < argv.length; i++) {
    const t = argv[i];
    if (t.startsWith("-")) {
      if (valueFlags.has(t)) i++;
      continue;
    }
    out.push(t);
  }
  return out;
}

/* ------------------------------------------------------------------ main */

/**
 * Normalize one shell command string into canonical actions.
 * A `cd x && y` chain yields the actions of `y` with cwd updated.
 */
/** Shell keywords that introduce a compound construct we do not parse. */
const COMPOUND = new Set(["for", "while", "until", "if", "case", "function", "select"]);

export function normalizeShell(command: string, cwd: string): Normalized[] {
  const tokens = tokenize(command.trim());
  if (!tokens.length) return [];

  // unwrap `bash -lc "…"` / `sh -c "…"`
  const head = basename(tokens[0] ?? "");
  if (["bash", "sh", "zsh"].includes(head)) {
    const ci = tokens.findIndex((t) => /^-[a-z]*c$/.test(t));
    if (ci >= 0 && tokens[ci + 1]) return normalizeShell(tokens[ci + 1], cwd);
  }

  // Compound constructs (`for … do … done`, `if … fi`) must not be split on `;`, which
  // shreds them into meaningless fragments. Emit one opaque action with low confidence
  // so the coverage gate sees them honestly rather than counting garbage as understood.
  if (COMPOUND.has(tokens[0]) || tokens.includes("do") || tokens.includes("then")) {
    return [{
      action: { op: "exec", argv: [tokens[0], "…compound…"], cwd, class: "other" },
      mutating: /\b(rm|mv|cp|tee|>|>>)\b/.test(command),
      confidence: 0.2,
    }];
  }

  const out: Normalized[] = [];
  let curCwd = cwd;

  for (const seg of splitSequence(tokens)) {
    if (!seg.length) continue;

    // strip leading `VAR=value` assignment prefixes: `UA=... curl ...`
    while (seg.length > 1 && /^[A-Za-z_][A-Za-z0-9_]*=/.test(seg[0])) seg.shift();
    if (!seg.length) continue;

    // `cd dir` updates cwd for what follows and emits nothing
    if (basename(seg[0]) === "cd") {
      if (seg[1]) curCwd = resolvePath(curCwd, seg[1]);
      continue;
    }

    const hasRedirect = seg.some((t) => REDIRECTS.has(t));
    const stages = splitPipeline(seg.filter((t) => !REDIRECTS.has(t)));
    const argv = stages[0] ?? [];
    if (!argv.length) continue;

    // trailing stages that only filter keep the pipeline read-only
    const tailPure = stages.slice(1).every((s) => PURE_FILTERS.has(basename(s[0] ?? "")));
    const cmd = basename(argv[0]);
    const ops = operands(argv);

    let action: CanonicalAction;
    let mutating = hasRedirect;
    let confidence = 0.9;

    if (cmd === "sed" && argv.includes("-i")) {
      action = { op: "edit", path: resolvePath(curCwd, ops[ops.length - 1] ?? "") };
      mutating = true;
    } else if (["cat", "nl"].includes(cmd) && ops.length && !hasRedirect) {
      action = { op: "read", path: resolvePath(curCwd, ops[0]) };
    } else if (["sed", "head", "tail"].includes(cmd) && ops.length && !hasRedirect) {
      const range = readRange(argv);
      action = { op: "read", path: resolvePath(curCwd, ops[ops.length - 1]), ...(range ? { range } : {}) };
    } else if (["grep", "rg", "ag", "ack"].includes(cmd)) {
      if (argv.includes("--files")) {
        action = { op: "search", kind: "filename", pattern: "*", paths: ops.map((p) => resolvePath(curCwd, p)) };
      } else {
        const [pattern, ...paths] = ops;
        action = {
          op: "search", kind: "content", pattern: pattern ?? "",
          paths: paths.length ? paths.map((p) => resolvePath(curCwd, p)) : [curCwd],
        };
      }
    } else if (["find", "fd"].includes(cmd)) {
      const ni = argv.indexOf("-name");
      action = {
        op: "search", kind: "filename",
        pattern: ni >= 0 ? (argv[ni + 1] ?? "*") : (ops[0] ?? "*"),
        paths: [resolvePath(curCwd, ops[0] ?? ".")],
      };
    } else if (["ls", "tree", "eza", "exa"].includes(cmd)) {
      action = { op: "list", path: resolvePath(curCwd, ops[0] ?? ".") };
    } else if (cmd === "curl" || cmd === "wget") {
      const url = ops.find((o) => /^https?:\/\//.test(o));
      action = url ? { op: "fetch", url } : { op: "exec", argv, cwd: curCwd, class: "other" };
    } else {
      const { cls, mutating: m } = classifyExec(argv);
      action = { op: "exec", argv, cwd: curCwd, class: cls };
      mutating = mutating || m;
      if (cls === "other") confidence = 0.5;
    }

    if (!tailPure && stages.length > 1) mutating = true;
    out.push({ action, mutating, confidence });
  }

  return out;
}

/** Minimal path resolution — enough to group templates, not a realpath. */
export function resolvePath(cwd: string, p: string): string {
  if (!p) return cwd;
  if (p.startsWith("/")) return p;
  if (p.startsWith("~")) return p;
  if (p === ".") return cwd;
  const parts = (cwd + "/" + p).split("/");
  const stack: string[] = [];
  for (const part of parts) {
    if (part === "" || part === ".") continue;
    if (part === "..") stack.pop();
    else stack.push(part);
  }
  return "/" + stack.join("/");
}
