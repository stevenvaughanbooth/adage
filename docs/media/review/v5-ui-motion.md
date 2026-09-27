# v5 · in-film UI and motion review

Coordinates are 1920×1080 (the stills are ×0.667). Line numbers refer to `film/index5.html`.

## Verdict

The content of the UI is right: the transcript proves the product, the callouts read at pitch distance, the evidence bars are honest. The craft is not there yet. Every panel simply fades in and out, callouts parallax with the follow cam, cards collide with each other and with the world (47–49 s, 81–89 s), the counters snap, the terminal wears fake macOS dots that rhyme badly with the brand lamps, and the film ends on a landing-page chip row. Fix the four collisions, give every element one birth-from-source choreography, and cut the chrome.

## Findings

1. **47–49 s · evidence cards collide.** `finish-line` (dx −520) lands on top of `lookup-work-item` (dx +40) while the camera is still climbing to the overhead key (line 209); by 49 s the two cards touch edge to edge and `lookup`'s badge is clipped. Fix: stop world-anchoring cards. Lay the four `.route` cards in one screen row: `top: 800px`, widths 400, gap 24, centred (x = 96 + i×424 + 52); leader from each card's top-centre to the node via `toScreen`. Delay all four to `tl ≥ 2.4` (camera settled), stagger .22 s.
2. **28–43 s · callouts drift with the follow cam.** `bx = x + dx` (line 311) re-anchors every frame, so `bash scripts/wi-lookup` slides 190 px left between 33 and 35 s. Best-in-class films never let text parallax. Fix: a fixed callout lane, `left: 300px`, `top: 300px` for sig cards and `top: 470px` for tool cards; only the leader end-point and dot are world-anchored. Leader goes from the node to the card's bottom-left corner as now.
3. **81–89 s · the close collides with the world.** The instinct rail passes through "Adage", and the goal sphere sits beside the three lamps as a fourth green dot (line 359, `top: 560`). Fix: from `T.close + .4` dim `railMat/coreMat` to .25 and the four signals + barrier to 0 (`railDim` branch, line 389); add `#ov` scrim `radial-gradient(900px 620px at 50% 46%, rgba(5,5,7,.6), transparent)` fading in over 1 s; lamps `top: 540px`, gap 28.
4. **28.2–31 s · three cards on one anchor in 2.2 s.** `you` (−190), `sig g` (−350), `sig r` (−190) churn on `start`. Cut the `you` peek (the transcript already shows the prompt at 29 s); start `sig r` at `SS.stepin + 1.1` so the green card has 1.1 s alone.
5. **29–43 s · terminal header.** Three grey `tb i` dots (line 59) are window chrome for a product that is not a window, and three-circles-in-a-row is the brand mark. Cut them. Header: `claude code · acme-billing` 14px/.12em ink3 left, `● adage` right with a deterministic pulse `opacity: .55 + .45·sin(t·5.2)`.
6. **31–43 s · counters.** (a) The ghost appears first (tl 1.4, x 520) with nothing at x 120 → orphan; show both at tl 1.4. (b) `textContent` swaps: a 120 px numeral snapping 15→20 is a jump cut; roll digits (spec below). (c) Labels rewrite themselves ("turn · the first edit"); keep labels fixed, add a 16px mono sub-line for milestones. (d) 33 s: the orange control trail runs through the "1"; give `.counter` a scrim (`padding: 24px 40px; margin: -24px -40px; background: radial-gradient(closest-side, rgba(5,5,7,.78), transparent)`). (e) 120 px numerals outrank the 92 px headline; use 96 px, labels 26 px.
7. **73.8–80 s · signal block.** `route="lookup-work-\nitem"` wraps inside an attribute value; the violet halo (`box-shadow 0 0 90px`) is a 2019 look; the centred box hides the very sphere it explains (75 s). Fix: pretty-print, one attribute per line; width 880, `left: 140px; top: 400px` (not centred); no glow; a 1.5px green leader from the green bar to `S1` via `toScreen`. Enter: see signature move 3.
8. **45–49 s · instinct rail crosses the subtitle** at y≈275. In `world()` line 396 set instinct `dim = .3` for `T.evidence + .5 … T.cool`.
9. **All cards · fill and radius.** `.peek` .90, `.term` .92, `.route` .92, `.sigbox` .92 are black holes over a bloomed rail; radii are 8/10/12/14/999. Unify on the frosted spec below; radius 10 everywhere, 8 for `.tag`.
10. **11 s · `.tag`** "Read tests/test_billing.py" clipped at the bottom edge. Clamp `x` to [96, 1824] and `y` to [96, 984]; fade if outside.
11. **43–50 s · badge duplicates the row.** `REFLEX · 23 OF 33` at 13px repeats "23 of 33 sessions". Badge = `REFLEX`/`INSTINCT` only, 15px.
12. **29–43 s · transcript wraps** ("looked / up with:", "Tests / pass") and the newest line sits 8 px from the bottom. Width 720 (`left: 1140`), viewport `bottom: 24px`, last line → "Done · payment_terms · models generated · tests pass".

## Component specs

**Surface (shared by `.peek .term .route .sigbox .pill`)**
`background: rgba(12,12,18,.72); backdrop-filter: blur(24px) saturate(140%); border: 1px solid rgba(255,255,255,.10); border-radius: 10px; box-shadow: 0 24px 60px -24px rgba(0,0,0,.85), inset 0 1px 0 rgba(255,255,255,.05)`. No coloured glow anywhere. Colour bar: drop `border-left`; use `::before { left:0; top:12px; bottom:12px; width:3px; border-radius:2px; background: var(--c) }`.

**Callout `.peek`** 520×auto, padding 12/16/13, mono 22/1.4, label 15px/.14em. Enter 320 ms `cubic-bezier(.2,.8,.2,1)`: opacity 0→1, translateY(8→0), scale(.98→1), blur(6→0). Exit 220 ms `cubic-bezier(.4,0,1,1)`: opacity→0, translateY(0→−6), blur→4. Dot: `r=6`, `fill:none; stroke:#fff; stroke-width:1.5` plus `r=2` filled (reads on the white agent). Leader 1.5px rgba(255,255,255,.45), draws with `stroke-dasharray`.

**Transcript `.term.tx`** 720×700 at (1140,150). Mono 20/1.55, `.tl` margin 0 0 14. Enter 480 ms: translateX(28→0), opacity, blur 4→0. Exit 320 ms reversed. Scroll: eased 600 ms per new line, 24 px bottom air, top mask 36 px.

**Counters** serif 96px tabular, labels sans 26px/400; ghost `#ff8a5c` with `text-shadow: 0 0 24px rgba(255,138,92,.25)`. Digit roll: each digit a 10-span column, `translateY(-d·1em)` over 260 ms `eio`, computed from the known schedule (deterministic).

**Evidence `.route`** 400×auto, padding 14/16, name mono 20/500, sentence sans 19/300, bar 4px radius 2 track rgba(255,255,255,.10) fill over 1.1 s `eo`, row mono 16 ink3. Enter as `.peek`.

**Signal `.sigbox`** 880 wide, mono 21/1.6, padding 20/28; tag names `#c9bfff`, attr keys ink3, values ink2, body ink. Lines light in order, 60 ms apart.

**Pill** mono 30, padding 16/28, radius 10, `border: 1px solid rgba(255,255,255,.14)`, no glow; block caret after `init` blinking at 530 ms. **Chips: cut.** URL becomes mono 20px ink3 at `top: 900`.

## Three signature moves

1. **Born from the signal.** Every callout in the film has one choreography: the rail sphere flashes (`openRing`), a ring-dot pops at its screen point (180 ms, `cubic-bezier(.34,1.56,.64,1)`), the leader draws to the fixed lane (260 ms), the card unfolds along it (`clip-path: inset(100% 0 0 0 → 0)`, 320 ms), and the `5 ms · CPU` stamp lands last (+140 ms, scale 1.15→1). Exit reverses in 220 ms, retracting into the dot.
2. **The odometer.** The ghost counter is a real odometer ticking every control step; at the cap its digits stop, desaturate `#ff8a5c → #7a7d8c` over 600 ms and a 2px red rule draws under "30". The hero "10" pulses once with the lamp glow at `SS.done`.
3. **Signals become lamps.** At 73.8 s the signal block does not fade in centred; it unfolds from `S1`'s screen position (`transform-origin` there, `clip-path: circle(0 → 140%)`, 640 ms) and stays tethered by a green leader. At `T.close − .5` it collapses back into the sphere; then S1 (green), S2 (amber) and the dead-end (red) are projected to screen and fly to the lamp row over 900 ms `eio`. The brand mark is literally the rail's signals.
