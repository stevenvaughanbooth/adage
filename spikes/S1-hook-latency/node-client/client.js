#!/usr/bin/env node
// S1 hook-latency spike: Node.js equivalent of client/src/main.rs, for
// comparing what a TS/JS hook client would cost vs the static Rust binary.
// Same modes: noop | stdin | socket.

'use strict';
const net = require('net');

function drainStdin() {
  return new Promise((resolve) => {
    const chunks = [];
    process.stdin.on('data', (c) => chunks.push(c));
    process.stdin.on('end', () => resolve(Buffer.concat(chunks)));
    process.stdin.on('error', () => resolve(Buffer.concat(chunks)));
    process.stdin.resume();
  });
}

function extractFields(buf) {
  const hasEvent = buf.includes('"hook_event_name"');
  const hasSession = buf.includes('"session_id"');
  return [hasEvent, hasSession];
}

function socketRoundtrip(sockPath) {
  return new Promise((resolve) => {
    const sock = net.createConnection(sockPath);
    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      try { sock.destroy(); } catch (_) {}
      resolve();
    };
    const timer = setTimeout(finish, 100);
    sock.on('connect', () => {
      sock.write('{"t0":"hit"}\n');
    });
    sock.on('data', (d) => {
      process.stdout.write(d);
      clearTimeout(timer);
      finish();
    });
    sock.on('error', () => {
      clearTimeout(timer);
      finish();
    });
  });
}

async function main() {
  const mode = process.argv[2] || 'stdin';
  if (mode === 'noop') {
    process.exit(0);
  } else if (mode === 'stdin') {
    const buf = await drainStdin();
    extractFields(buf);
    process.exit(0);
  } else if (mode === 'socket') {
    const buf = await drainStdin();
    extractFields(buf);
    const sockPath = process.argv[3] || '/tmp/adage-s1.sock';
    await socketRoundtrip(sockPath);
    process.exit(0);
  } else {
    process.exit(0);
  }
}

main();
