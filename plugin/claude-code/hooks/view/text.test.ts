// The text helpers every drawn word uses (view/text.ts; ux-design §1.9, §1.10; T-B2-03, T-B2-11):
// glyphs and their twins, columns, cutting and fitting a line, plurals, a reflex named by its gist.
import { describe, expect, test } from 'claude-code/testing'

import { GRAPH } from '../fixtures/kit'
import { reflexesOf } from '../reads'
import { cols, cut, fit, g, plural, reflexOf, sayOf } from './text'

const REFLEXES = reflexesOf(GRAPH)

describe('T-B2-03, T-B2-11 · columns and glyphs', () => {
  test('a glyph or its ASCII twin; every glyph one column', () => {
    expect(g('reflex', false)).toBe('◠')
    expect(g('reflex', true)).toBe('~')
    expect(cols('◠ adage')).toBe(7)
  })

  test('cut: to n columns with an ellipsis, three dots in ASCII; never wider than asked', () => {
    expect(cut('abcdef', 10)).toBe('abcdef')
    expect(cut('abcdef', 4)).toBe('abc…')
    expect(cut('abcdef', 5, true)).toBe('ab...')
    expect(cut('abcdef', 2, true)).toBe('ab')
  })

  test('fit: parts dropped from the right while too wide, the first kept, then cut', () => {
    expect(fit(['one', 'two', 'three'], 80, false)).toBe('one · two · three')
    expect(fit(['one', 'two', 'three'], 10, false)).toBe('one · two')
    expect(fit(['a long first part', 'two'], 8, false)).toBe('a long …')
    expect(fit(['one', '', 'three'], 80, true)).toBe('one - three')
  })

  test('plural: one, many', () => {
    expect(plural(1, 'step', 'steps')).toBe('1 step')
    expect(plural(2, 'step', 'steps')).toBe('2 steps')
  })
})

describe('T-B2-02 · a reflex by what it tells the agent', () => {
  test('reflexOf finds it in the tag\'s track when one is named; sayOf is its gist, else its id in words', () => {
    expect(reflexOf(REFLEXES, 'codegen-after-schema', 'acme/learned@1')?.reference).toBe('acme/learned')
    expect(reflexOf(REFLEXES, 'nothing')).toBeUndefined()
    expect(sayOf(REFLEXES, 'codegen-after-schema')).toBe('run make gen after a schema edit')
    expect(sayOf([], 'codegen-after-schema')).toBe('codegen after schema')
  })
})
