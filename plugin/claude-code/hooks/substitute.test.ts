// The dead end that never runs, as the binary words it (substitute.ts; plan C6, T-C6-03's reader):
// a learned replacement with its command, route and one line, or none. call.test.ts takes the call
// through the engine.
import { expect, test } from 'claude-code/testing'

import { substituteOf } from './substitute'

const SUB = { command: 'make test', route: 'dead-end-pytest', track: 'acme/billing@1', context: 'adage ran make test in place of python3 -m pytest tests.' }

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
