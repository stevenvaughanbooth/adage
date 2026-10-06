// The dead end that never runs, as the binary words it (substitute.ts; plan C6, T-C6-03's reader):
// a learned replacement with its command, route and one line, or none. The answers are the wire's
// substitute.json and plan.json (docs/claude-mod/impl/wire.md). call.test.ts takes the call through
// the engine.
import { expect, test } from 'claude-code/testing'

import { wire, wireWith } from './fixtures/kit'
import { substituteAnswerOf, substituteOf } from './substitute'

const SUB = wire('substitute').substitute as Record<string, unknown>

test('T-C6-03 · substituteAnswerOf reads substitute.json: the command, the route, the track and the line, each there', () => {
  const read = substituteAnswerOf(wire('substitute'))
  expect(read).toEqual({ command: SUB.command, route: SUB.route, track: SUB.track, context: SUB.context })
  for (const v of Object.values(read ?? {})) expect(v).not.toBe('')
  // The dry run names the same replacement in its plan.
  expect(substituteOf((wire('plan').plan as Record<string, unknown>).substitute)).toEqual(read)
  expect(substituteAnswerOf(wireWith('substitute', null))).toBeUndefined()
  expect(substituteAnswerOf(undefined)).toBeUndefined()
})

test('T-C6-03 · a malformed substitute is none', () => {
  expect(substituteOf(SUB)).toEqual(SUB)
  expect(substituteOf({ ...SUB, command: ' ' })).toBeUndefined()
  expect(substituteOf({ ...SUB, route: 7 })).toBeUndefined()
  expect(substituteOf({ ...SUB, context: '' })).toBeUndefined()
  expect(substituteOf('make test')).toBeUndefined()
})

test('T-C6-03 · a missing track reads as empty; the replacement is kept as the binary wrote it', () => {
  expect(substituteOf({ ...SUB, track: undefined })).toEqual({ ...SUB, track: '' })
  expect(substituteOf({ ...SUB, command: '  make test  ' })?.command).toBe('  make test  ')
})
