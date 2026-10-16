#!/usr/bin/env node
'use strict';
// S1 hook-latency spike: benchmark harness.
//
// Spawns the compiled Rust hook client (and, for comparison, a Node.js
// client) the way a Node-based harness like Claude Code would --
// child_process.spawn, one child per event -- and records wall-clock time
// from the spawn() call to the child's 'exit' event. Percentiles are
// computed per scenario and written to results/<scenario>.json plus a
// combined results/summary.json and results/summary.csv.
//
// Usage:
//   node bench/run.js [scenario-id ...]     # default: all scenarios
//
// Requires: client and server already built in release mode
// (`cargo build --release` from the spike root), and echoserver already
// running for the D/F scenarios (rerun.sh handles both).

const { spawn, execSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { small2kb, large300kb } = require('./payloads');

const ROOT = path.resolve(__dirname, '..');
const RESULTS_DIR = path.join(ROOT, 'results');
const SANDBOX_DIR = path.join(ROOT, 'sandbox');
const RUST_CLIENT = path.join(ROOT, 'target', 'release', 'hookclient');
const NODE_CLIENT = path.join(ROOT, 'node-client', 'client.js');
const SOCK_WARM = process.env.ADAGE_S1_SOCK || '/tmp/adage-s1-bench.sock';
const SOCK_MISSING = '/tmp/adage-s1-does-not-exist.sock';

const PAYLOAD_2KB = small2kb();
const PAYLOAD_300KB = large300kb();

function nowUptime() {
  try {
    return execSync('uptime').toString().trim();
  } catch (e) {
    return `uptime failed: ${e.message}`;
  }
}

function percentile(sortedMs, p) {
  if (sortedMs.length === 0) return null;
  const idx = Math.min(sortedMs.length - 1, Math.ceil((p / 100) * sortedMs.length) - 1);
  return sortedMs[Math.max(0, idx)];
}

function stats(samples) {
  const ok = samples.filter((s) => s.ms !== null).map((s) => s.ms);
  ok.sort((a, b) => a - b);
  const errors = samples.filter((s) => s.ms === null);
  const nonzero = ok.reduce((a, b) => a + b, 0);
  return {
    n: samples.length,
    ok: ok.length,
    errors: errors.length,
    min: ok.length ? ok[0] : null,
    p50: percentile(ok, 50),
    p95: percentile(ok, 95),
    p99: percentile(ok, 99),
    max: ok.length ? ok[ok.length - 1] : null,
    mean: ok.length ? nonzero / ok.length : null,
  };
}

function runOne(cmd, args, stdinBuf, stdio) {
  return new Promise((resolve) => {
    const t0 = process.hrtime.bigint();
    let child;
    try {
      child = spawn(cmd, args, { stdio });
    } catch (err) {
      resolve({ ms: null, error: String(err) });
      return;
    }
    let settled = false;
    child.on('error', (err) => {
      if (settled) return;
      settled = true;
      resolve({ ms: null, error: String(err) });
    });
    child.on('exit', (code, signal) => {
      if (settled) return;
      settled = true;
      const t1 = process.hrtime.bigint();
      resolve({ ms: Number(t1 - t0) / 1e6, code, signal });
    });
    if (stdio[0] === 'pipe' && child.stdin) {
      child.stdin.on('error', () => {}); // e.g. EPIPE if child exits before reading (noop)
      if (stdinBuf && stdinBuf.length > 0) {
        child.stdin.end(stdinBuf);
      } else {
        child.stdin.end();
      }
    }
  });
}

async function runScenario(scn) {
  const samples = [];
  const warmup = scn.freshCopy ? 0 : Math.min(20, Math.floor(scn.n / 10));
  for (let i = 0; i < warmup; i++) {
    const { cmd, args } = scn.build(i, true);
    await runOne(cmd, args, scn.stdin, scn.stdio);
  }
  for (let i = 0; i < scn.n; i++) {
    const { cmd, args, cleanup } = scn.build(i, false);
    const r = await runOne(cmd, args, scn.stdin, scn.stdio);
    samples.push(r);
    if (cleanup) cleanup();
  }
  return samples;
}

function buildScenarios() {
  fs.mkdirSync(SANDBOX_DIR, { recursive: true });
  const gDir = path.join(SANDBOX_DIR, 'g-copies');
  fs.mkdirSync(gDir, { recursive: true });

  return [
    {
      id: 'A_spawn_exit_floor',
      label: 'A: spawn + exit only (floor, exec form, no stdin plumbing)',
      n: 3000,
      stdin: null,
      stdio: ['ignore', 'ignore', 'ignore'],
      build: () => ({ cmd: RUST_CLIENT, args: ['noop'] }),
    },
    {
      id: 'B_stdin_2kb',
      label: 'B: + read 2 KB stdin',
      n: 3000,
      stdin: PAYLOAD_2KB,
      stdio: ['pipe', 'ignore', 'ignore'],
      build: () => ({ cmd: RUST_CLIENT, args: ['stdin'] }),
    },
    {
      id: 'C_stdin_300kb',
      label: 'C: + read 300 KB stdin (typical large PostToolBatch)',
      n: 3000,
      stdin: PAYLOAD_300KB,
      stdio: ['pipe', 'ignore', 'ignore'],
      build: () => ({ cmd: RUST_CLIENT, args: ['stdin'] }),
    },
    {
      id: 'D_socket_warm',
      label: 'D: + socket round trip, server warm',
      n: 3000,
      stdin: PAYLOAD_2KB,
      stdio: ['pipe', 'ignore', 'ignore'],
      build: () => ({ cmd: RUST_CLIENT, args: ['socket', SOCK_WARM] }),
    },
    {
      id: 'E_socket_missing',
      label: 'E: + socket connect to a missing socket (fail-open path)',
      n: 3000,
      stdin: PAYLOAD_2KB,
      stdio: ['pipe', 'ignore', 'ignore'],
      build: () => ({ cmd: RUST_CLIENT, args: ['socket', SOCK_MISSING] }),
    },
    {
      id: 'F_socket_shell_form',
      label: 'F: same as D but spawned via /bin/sh -c (shell form)',
      n: 3000,
      stdin: PAYLOAD_2KB,
      stdio: ['pipe', 'ignore', 'ignore'],
      build: () => ({
        cmd: '/bin/sh',
        args: ['-c', `"${RUST_CLIENT}" socket "${SOCK_WARM}"`],
      }),
    },
    {
      id: 'G_fresh_binary',
      label: 'G: freshly written, never-executed binary copy (Gatekeeper/codesign cost)',
      n: 300,
      stdin: PAYLOAD_2KB,
      stdio: ['pipe', 'ignore', 'ignore'],
      freshCopy: true,
      build: (i) => {
        const dst = path.join(gDir, `hookclient-${i}-${process.pid}`);
        fs.copyFileSync(RUST_CLIENT, dst);
        fs.chmodSync(dst, 0o755);
        return {
          cmd: dst,
          args: ['stdin'],
          cleanup: () => {
            try {
              fs.unlinkSync(dst);
            } catch (_) {}
          },
        };
      },
    },
    {
      id: 'NB_node_stdin_2kb',
      label: 'Node comparison: + read 2 KB stdin',
      n: 1000,
      stdin: PAYLOAD_2KB,
      stdio: ['pipe', 'ignore', 'ignore'],
      build: () => ({ cmd: process.execPath, args: [NODE_CLIENT, 'stdin'] }),
    },
    {
      id: 'ND_node_socket_warm',
      label: 'Node comparison: + socket round trip, server warm',
      n: 1000,
      stdin: PAYLOAD_2KB,
      stdio: ['pipe', 'ignore', 'ignore'],
      build: () => ({ cmd: process.execPath, args: [NODE_CLIENT, 'socket', SOCK_WARM] }),
    },
  ];
}

async function main() {
  const requested = process.argv.slice(2);
  const all = buildScenarios();
  const scenarios = requested.length ? all.filter((s) => requested.includes(s.id)) : all;
  if (scenarios.length === 0) {
    console.error('No matching scenarios for:', requested);
    process.exit(1);
  }

  if (!fs.existsSync(RUST_CLIENT)) {
    console.error(`Missing ${RUST_CLIENT} -- run cargo build --release first.`);
    process.exit(1);
  }

  fs.mkdirSync(RESULTS_DIR, { recursive: true });

  const meta = {
    started_at: new Date().toISOString(),
    uptime_before: nowUptime(),
    node_version: process.version,
    platform: `${os.type()} ${os.release()} ${os.machine ? os.machine() : os.arch()}`,
    cpus: os.cpus().length,
  };
  console.log('=== S1 hook-latency bench ===');
  console.log(meta.uptime_before);

  const summary = { meta, scenarios: {} };

  for (const scn of scenarios) {
    process.stdout.write(`Running ${scn.id} (n=${scn.n})... `);
    const beforeUptime = nowUptime();
    const t0 = Date.now();
    const samples = await runScenario(scn);
    const wallMs = Date.now() - t0;
    const afterUptime = nowUptime();
    const st = stats(samples);
    console.log(
      `done in ${(wallMs / 1000).toFixed(1)}s  p50=${st.p50?.toFixed(3)}ms p95=${st.p95?.toFixed(3)}ms p99=${st.p99?.toFixed(3)}ms errors=${st.errors}`
    );

    const record = {
      id: scn.id,
      label: scn.label,
      n: scn.n,
      uptime_before: beforeUptime,
      uptime_after: afterUptime,
      wall_seconds: wallMs / 1000,
      stats: st,
      samples_ms: samples.map((s) => s.ms),
    };
    fs.writeFileSync(
      path.join(RESULTS_DIR, `${scn.id}.json`),
      JSON.stringify(record, null, 2)
    );
    summary.scenarios[scn.id] = { label: scn.label, n: scn.n, stats: st, uptime_before: beforeUptime, uptime_after: afterUptime };
  }

  meta.uptime_after = nowUptime();
  fs.writeFileSync(path.join(RESULTS_DIR, 'summary.json'), JSON.stringify(summary, null, 2));

  const csvLines = ['scenario,n,ok,errors,min_ms,p50_ms,p95_ms,p99_ms,max_ms,mean_ms'];
  for (const [id, r] of Object.entries(summary.scenarios)) {
    const s = r.stats;
    csvLines.push(
      [
        id,
        s.n,
        s.ok,
        s.errors,
        s.min?.toFixed(4),
        s.p50?.toFixed(4),
        s.p95?.toFixed(4),
        s.p99?.toFixed(4),
        s.max?.toFixed(4),
        s.mean?.toFixed(4),
      ].join(',')
    );
  }
  fs.writeFileSync(path.join(RESULTS_DIR, 'summary.csv'), csvLines.join('\n') + '\n');

  console.log('Wrote results/summary.json and results/summary.csv');
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
