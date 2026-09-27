# Adage v5 · brand and title review

## Verdict

The system (night, one violet, a talking serif, three lamps, one rail) is sound and the film uses it with discipline for 75 seconds. The last eleven seconds throw it away. The close is a landing page pasted over a busy 3D scene: seven tiers of type in six sizes, a random instinct rail cutting through the counters of the wordmark, a green signal sphere sitting in line with the lamps as a fourth lamp, and a wordmark whose treatment (gradient serif, blur-in, violet drop-shadow) was already spent on three numerals fifteen seconds earlier. There is no ident: nothing a viewer could draw afterwards. Fix the close as a 2.5-second ident that reuses the film's own objects (the paving head, the rail, the lamps), cut the end card to four things, and the brand will be remembered as one moving object rather than a palette.

## Findings

1. **80.0–81.1 s · wordmark reveal (index5.html L358, L365).** A blur-in with letter-spacing .1→−.03em is the stock 2026 AI reveal; the brand book itself flags this. Nothing connects the mark to the rail the film spent 60 s building. Fix: replace with the ident below; delete the letter-spacing lerp; keep Instrument Serif 300px, gradient `#fff 15% → #c9bfff 100%`.
2. **80–91 s · rail through the letters (KEYS L215–216, instRails L396).** Instinct rail 0 crosses "Adage" at y≈220–260 through the "d" and "a" counters. Fix: for `t ≥ T.close − .4` multiply `instRails` and `railMat/coreMat` opacity by `1 − P(t, T.close − .4, .4)`; the only rail on the end card is the ident rail (camera-attached, below).
3. **80–91 s · fourth lamp (L392–393).** Signal S3 (green, K.edit) projects to (1115, 590) on the 1080 frame, 100 px right of the red lamp at the lamps' y. S2 amber and the red barrier also stay lit. Fix: same fade as (2) on `S1..SG`, `barrier`, `barBox`, `deadDot`. Camera for close: `V3(0, 2.2, 22)` → `V3(0, 2.6, 24)`, lookAt `V3(0, .4, 0)`, fog `.035`, so the city is a haze under the horizon.
4. **80.8–81.7 s · lamps pop instead of light (L39, L365).** 26 px dots at gap 30 px with a scale .2→1 pop read as macOS controls, and the terminal titlebar (L59, three 10 px grey dots at 28–43 s) has already taught the viewer that three dots are window chrome. Fix: lamps 28 px, centre-to-centre 56 px, `box-shadow 0 0 30px 4px currentColor`; they *ignite* (spec below), never scale from zero. Remove the three grey dots from `.term .tb` (keep the title text only).
5. **64–73 s · gradient spent on numerals (L346).** `big serif glow` is the wordmark treatment; using it on −61%, 10×, 0 makes the reveal a fourth instance. Fix: numerals flat `#f4f4f7`, no gradient, keep blur-in; the gradient exists only on "Adage".
6. **L37 `.glow` drop-shadow.** Brand rule: never a drop shadow on the wordmark. Fix: remove `filter: drop-shadow(...)`; the glow under the mark comes from the bloomed rail core, not CSS.
7. **80–91 s · end card overload (L358–364).** Wordmark, lamps, tagline, descriptor, bordered pill, URL, four bordered chips; chips at the 20 px floor. Fix: see End card spec: four elements, one mono line, no borders.
8. **83.5 s · `.pill .c #ffd479` (L41).** A fourth warm hue 20 px from the amber lamp. Fix: on the end card set command words `#f4f4f7`, `$`/`&&` `#7a7d8c`; keep #ffd479 only inside product footage.
9. **4–17 s and 50–57 s · six oranges.** `--heat #ff8a5c` (L11), COL.heat 0xff7a4a (L106), heroLine 0xff8a4a, ghostLines 0xff6a2a (L162), controlLine 0xff9a6a (L163), cHeat 0xff6a2a (L371). Fix: one GPU orange `0xff7a59` for every tube and node heat; `0xffb08f` only for agent cores; `--heat: #ff7a59`.
10. **52.6–55.2 s · stat colour snaps (L334).** `u > .5 ? "#c9bfff" : "#ffb08f"` is a cut, not cooling. Fix: lerp in Color space: `new THREE.Color("#ff7a59").lerp(new THREE.Color("#c9bfff"), u).getStyle()`.
11. **22–28 s · instincts indistinguishable from the reflex.** Both violet with white core. Brand: instincts arrive amber, confirmed green. Fix: instinct core colour `0xfbbf24` at init (L169 `k`), lerp to `0xffffff` over .6 s when its route fires (`SS.sigC` for finish-line); the others stay amber through the film.
12. **0–4 s · no lamps, no mark.** Correct not to open on a wordmark; wrong to open on nothing. See Signature move 1.
13. **Format readiness (L14, L31, L33, L304).** Everything is absolute px at 1920×1080; four chips span ~1400 px; the terminal sits at left 1190. A 9:16 cut needs `--u = min(W/1920, H/1080)` scaling on all overlay px, cam.fov 44→62 for 1080×1920, the terminal moved below the rail, chips stacked. A 6-s bumper is: ident 0–2.5, install line 2.9, hold; possible only once the ident is a self-contained function `ident(t0, cx, cy, s)`.

## The ident

2.5 s. Coordinates at 1920×1080; scale everything by `s = H/1080`. Wordmark "Adage" 300px Instrument Serif, letter-spacing −.03em, width ≈ 690px, baseline y = 520, letter centres x = 690, 830, 975, 1110, 1240. Rail: a straight `TubeGeometry` (sleeve r .11 violet 0x8b7cff, core r .045 white) parented to the camera (`cam.add`) at z = −10, y so it projects onto the baseline; the head is the film's `paveHead` (sphere r .26, white, point light 6). Bloom strength .95 during the ident. Letters are per-glyph `<span>`s, two stacked per glyph: `heat` (colour #ffb08f) over `cool` (the gradient).

| ms | element | state |
|---|---|---|
| 0 | frame | #050507, nothing. Riser starts (band-pass noise 200→2 kHz, 900 ms). |
| 120 | head | appears at x = −40, y = 520; bloom halo ≈ 60 px. |
| 120–1000 | head + rail | head x: −40 → 1960, easeInOutSine; rail draw-range follows the head; sleeve 6 px on screen, core 2 px. |
| 330 / 400 / 470 / 540 / 610 | A d a g e | each glyph starts when the head is 60 px before its centre: translateY .35em→0, blur 18→0 px, opacity 0→1, 420 ms easeOutQuart; rises in `heat` orange. |
| +140 after each glyph lands | glyph | `heat` span fades 1→0 over 500 ms while `cool` fades in: the mark cools white→violet as the core passes beneath it. |
| 1000 | head | exits right. Sub hit 55 Hz, 300 ms decay. |
| 1000–1400 | rail | core opacity 1→0; sleeve becomes the brand gradient (transparent 0%, violet 20%, white 50%, violet 80%, transparent 100%) across x 160–1760; sleeve opacity .85. The "g" descender crosses the rail; leave it. |
| 1100–1300 | lamps | three unlit sockets #1c1c26, 28 px, centres (904, 640), (960, 640), (1016, 640); opacity 0→.6. |
| 1300 / 1420 / 1540 | green / amber / red | ignition: fill snaps to #4ade80 / #fbbf24 / #fb7185 in one frame; box-shadow 0 0 0 0 → 0 0 44px 8px at 90 ms → 0 0 30px 4px at 360 ms; scale .85→1 easeOutBack(1.4). Ticks: 40 ms sines 880, 1175, 740 Hz (red is the low note). |
| 1700–2100 | tagline | "Put your agent on rails." serif 64px #f4f4f7, cap top y = 720, rises through a mask 400 ms easeOutQuart. |
| 2100→ | lamps | breathe: box-shadow 30/4 → 8/0, 3.2 s ease-in-out, delays 0/.4/.8 s. Silence. |
| 2500 | hold | wordmark, rail, lamps, tagline. |

## End card spec

Four elements, centred, on the hazed city (edges opacity .2, nodes ×.3, no other rails). From T.close: ident 0–2.5 s; install line 2.9 s; fact line 3.4 s; hold to 91 s with lamps breathing and a white-hot pulse running the rail left→right (700 ms) at +4.4 s and +8.4 s.

| order | element | spec |
|---|---|---|
| 1 | Wordmark | 300px serif, gradient `#fff 15% → #c9bfff`, baseline y 520, no filter. |
| 2 | Rail + lamps | rail on the baseline (ident); lamps 28 px, c-t-c 56, y 640. |
| 3 | Tagline | "Put your agent on rails." serif 64px, y 720. Drop "Ships with instincts. Learns reflexes." (said twice as headlines already). |
| 4 | Install line | mono 32px, no pill, y 860: `$ brew install stevenvaughanbooth/tap/adage && adage init`; `$` and `&&` #7a7d8c, rest #f4f4f7. Drop the URL (the tap names the owner). |
| 5 | Fact line | mono 22px #7a7d8c, letter-spacing .04em, y 960, no borders: `Apache-2.0 · one 10 MB Rust binary · any harness with hooks · text only, fail-open`. In 9:16, wrap to two lines at 1080 wide. |

## Three signature moves

1. **Lamp bookend.** 0.0 s: the three lamps sit lit in the black, centred, 28 px. At 0.50 / 0.59 / 0.68 s they die red, amber, green (fill → #1c1c26, glow → 0 in 90 ms; three descending clicks). Then "Your agent has no reflexes." rises at 1.0 s. At 80.9 s they relight green, amber, red in the ident. The lamps become the film's punctuation and the bumper's whole story.
2. **The head through the lens.** At 79.2 s the same `paveHead` that paved the rail at 17 s runs the rail toward the camera (camera at rail height, lookAt along the spine), passes the lens with an 80 ms white flash (a full-screen div, opacity 1→0, 80 ms), and on the far side it is the ident head drawing the rail under the letters. One object from world to mark: the reflex became the logo.
3. **Heat cools inside the mark.** The letters rise GPU-orange and cool to white-violet as the rail's core passes beneath them (ident table, +140 ms rows). The brand book names orange-to-violet as the only thing Adage owns; put it in the logo reveal so the 6-second bumper carries the film's argument without a word of copy.
