// Reading untyped JSON (shape.ts; T-X-02): a malformed value is nothing, never a throw.
import { describe, expect, test } from 'claude-code/testing'

import { isRecord, num, records, str, strings } from './shape'

describe('T-X-02 · shapes', () => {
  test('an object is one that is not null and not a list', () => {
    expect([isRecord({}), isRecord([]), isRecord(null), isRecord('x')]).toEqual([true, false, false, false])
  })

  test('a string, a finite number, the strings and the objects of a list; anything else is empty', () => {
    expect([str('a'), str(1), str(undefined)]).toEqual(['a', '', ''])
    expect([num(3), num(Number.NaN), num(Infinity), num('3')]).toEqual([3, 0, 0, 0])
    expect(strings(['a', 1, 'b'])).toEqual(['a', 'b'])
    expect(strings('a')).toEqual([])
    expect(records([{ a: 1 }, [], null, 'x'])).toEqual([{ a: 1 }])
    expect(records(undefined)).toEqual([])
  })
})
