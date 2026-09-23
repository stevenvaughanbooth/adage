'use strict';
// Builds synthetic hook-event payloads that resemble what Claude Code would
// pipe to a hook's stdin: a small object for most events, and a large one
// for a PostToolBatch carrying tool output.

function pad(target, obj) {
  const base = JSON.stringify(obj);
  const need = Math.max(0, target - base.length - 20);
  const filler = 'x'.repeat(need);
  const withFiller = { ...obj, _filler: filler };
  let out = JSON.stringify(withFiller);
  // Nudge to hit the target length closely (JSON escaping can shift size).
  if (out.length < target) {
    out = JSON.stringify({ ...withFiller, _filler: filler + 'x'.repeat(target - out.length) });
  }
  return Buffer.from(out, 'utf8');
}

function small2kb() {
  return pad(2048, {
    hook_event_name: 'PreToolUse',
    session_id: '11111111-2222-3333-4444-555555555555',
    tool_name: 'Bash',
    cwd: '/Users/dev/project',
    tool_input: { command: 'git status', description: 'check working tree' },
  });
}

function large300kb() {
  return pad(300 * 1024, {
    hook_event_name: 'PostToolBatch',
    session_id: '11111111-2222-3333-4444-555555555555',
    tool_name: 'Bash',
    cwd: '/Users/dev/project',
    tool_output: { stdout: '', stderr: '', exit_code: 0 },
  });
}

module.exports = { small2kb, large300kb };
