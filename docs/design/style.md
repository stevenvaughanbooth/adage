# Adage style guide

> Status: adopted 2026-09-28 with the visual redesign ([plan](redesign-plan.md)). Every
> user-facing surface follows it: setup, every command's output, `adage watch`, `adage ui`
> and the web map. Words follow the [lexicon](../lexicon.md). Text the *agent* sees (directive
> templates) is out of scope: it stays plain and factual.

## One source

Colours, fonts, radii, spacing, motion and terminal glyphs are defined once, in
[`design/tokens/adage.tokens.json`](../../design/tokens/adage.tokens.json) (W3C DTCG 2025.10,
modes `night` and `day`). `cargo xtask tokens` generates:

- `crates/adage-term/src/tokens.rs`: the terminal half. Each colour has night and day values,
  the nearest xterm-256 index for each (computed, never hand-picked) and its role in a
  16-colour theme;
- `design/build/tokens.css`: CSS variables for the web map.

Both outputs are committed, so building Adage never needs Node. `cargo test --workspace` fails
when they are stale, and when any text colour falls below 4.5:1 contrast on any ground in
either mode. Never edit a generated file; never write a colour anywhere else.

## Type

| Role | Web | Terminal |
| --- | --- | --- |
| Display: headlines, big numbers | **Bricolage Grotesque** 600–700, tracking −0.03em | the user's font, bold |
| Body: explanation, UI | **Hanken Grotesk** 300 (body), 400–500 (UI) | the user's font |
| Mono: anything real (commands, reflex ids, evidence, timestamps) | **Red Hat Mono** 400–500 | the user's font |

All three are SIL OFL and are embedded with the map, never fetched from a font CDN. No italics
for emphasis: Bricolage has none, and emphasis is carried by colour (the accent) or weight.

## Colour

- **Semantic, never literal.** Code asks for a role (`Role::Fire`, `Role::Ink3`,
  `t.dim()`), never a colour. Raw ANSI codes, `Color::` values and hex literals belong in the
  theme and the generated tokens only.
- **The terminal's own colours win.** Body text uses the default foreground; nothing paints the
  background. At 16 colours each role maps to a named ANSI role so the user's palette applies;
  at 256 colours and truecolor, to the token for a dark or light ground.
- **One accent.** Violet (`accent`) marks the command in a header, the active step, selection,
  and commands the reader can run (`accent.soft`).
- **Lamps mean strength.** Green, amber and red appear only on fire, prime and inhibit (and on
  success, warning and failure marks). Never decorative, never bullets.
- **Orange means cost.** `cost` is for GPU spend and nothing else, so savings never use it.
- **Savings use no new colour.** The amounts (`~$0.034`, `3.1 turns`, the interval) stay in
  the default ink: they are evidence, not a celebration. Only the verdict carries a mark, with
  the existing roles: `saved` a `✓` in `lamp.fire` (success), `adds cost` a `✗` in
  `lamp.inhibit` (failure), `measuring` and `no measurable difference` a `○` in `ink.3`.
  Under `NO_COLOR` the verdict is the words alone.
- **Degrade by meaning.** Under `NO_COLOR` strengths are spelled out (`FIRE`, `PRIME`,
  `INHIBIT`), tertiary text dims and the accent is bold. Pipes and files get no escape codes at
  all. `ADAGE_COLOR=never|16|256|truecolor`, `ADAGE_THEME=day|night` and `ADAGE_ASCII=1`
  override detection.

## Glyphs

| Meaning | UTF-8 | ASCII |
| --- | --- | --- |
| lamp (strength) | `●` | `*` |
| off, nascent, withheld | `○` | `o` |
| instinct | `◉` | `(o)` |
| reflex | `◠` | `~` |
| done / failed | `✓` / `✗` | `+` / `x` |
| directs to | `→` | `->` |
| active / finished step | `◆` / `◇` | `*` / `+` |
| separator | `·` | `-` |
| procedure card | `▤` | `[=]` |
| rewrite (the command was changed before it ran) | `↪` | `=>` |
| quiet (uplift below the bar; waits for a person) | `◌` | `(.)` |
| from a skill (`adage train`) | `◈` | `<>` |
| notice | `»` | `>>` |

The last five are proposed for the newer concepts (procedure cards, PreToolUse rewrites, uplift,
compiled skills, notices); they are not yet in `adage.tokens.json`'s `glyph` group, which stays
the source: add them there (with their ASCII fallbacks) before any surface uses them. Each is
one column wide in common terminal fonts.

## The output grammar

Every command prints through `adage_term::Out`:

```text
adage status · acme-billing · gen 8dd5ec01 · hooks claude, codex      header: command, then facts

instincts   1 track · 1 instinct · adage/finish-line@0.1.0             key column is 12 wide
reflexes    2 tracks · 4 reflexes from 57 traces

  ● fire     ◠ lookup-work-item        23/33  lb 0.53 · 8 turns saved   rows align by display width
  ● inhibit  ◠ dead-end-d41b1e         11/43  "No module named pytest" → make test

next  adage review · 1 nascent reflex waits for you                     every command ends here
```

- **Header** first: `adage <command>` in the accent, then facts separated by ` · `.
- **Evidence over enthusiasm.** Numbers come with their denominators (`23/33`, `lb 0.53`).
- **Every command ends with `next`** when there is something to do, naming the exact command.
  Empty states and errors do too.
- **Errors** name what failed, then each cause, then help: `error`, `├─ cause`, `╰─ help`.
- **`--json`** is available on every command that reads state, and prints nothing else.

## Motion

A firing reflex pulses once (`duration.pulse`); nothing loops forever except the live
indicator. Headlines rise through a mask (`duration.rise`, `easing.out`). The web honours
`prefers-reduced-motion`; the terminal honours `ADAGE_REDUCED_MOTION=1`.

## Tests

Any user-visible change updates a snapshot:

- terminal frames: insta snapshots of ratatui `TestBackend` buffers (`crates/adage-cli/src/ui/snapshots/`);
- printed output: insta snapshots of `Out` rendered plain, ASCII and coloured
  (`crates/adage-term/src/snapshots/`);
- the web map: Playwright screenshots in both modes, with axe checks.

`cargo insta review` shows what changed.
