# v5 · edit and sound review

Grid: 112 BPM → beat 0.5357 s, bar 2.1429 s. All times below are seconds; "b" = beat index inside a section.

## Verdict

91 s is the right ceiling for a live pitch opener; the cut should be exactly 42 bars = 90.0 s, not 91.0. The length is fine, the spending is wrong: the film rushes its punchlines (cold open holds 0.8 s; "First edit at turn 3" never reaches full opacity before it starts leaving) and sags on static holds (evidence 4 s of nothing new, trust 3.6 s, end card 6.4 s + a fade nobody asked for). None of the twelve `T` constants sits on the 0.536 s grid, and because each score section re-anchors its kick loop at its own `T`, the pulse stumbles at 17.0 (a 0.34 s gap between kicks), 27.6 and 50.0. Every transition is an eased camera spline; the film has no hard cut, so the three moments that deserve one (the drop, session→evidence, numbers→trust) land soft. Thirteen near-identical hits in 91 s flatten the hierarchy. The drop at 50 s is not earned: the riser is 2.2 s, the hit lands on an empty frame (ghosts start at 50.3, headline 50.5, the actual cooling at 53.0), and it is 0.18 s off the beat. There is no true silence anywhere; the "silence before the ask" is a −28 dB pad under two overlapping hit tails.

## Findings

1. **50.00 · the drop is off-grid and unsynced.** `T.cool=50.0` = beat 93.3; the picture has nothing on the hit. Fix: cool at 53.57 (bar 25), hard camera cut on the downbeat (`KEYS`: add `[T.cool-.001, V3(-1,22,17), V3(-1,0,-3)]`), heat flood starts at `coolStart = T.cool` (index5.html L151, drop the `+.3`), headline at b1. Precede with a 7-beat riser and one beat of digital silence (see cue sheet).
2. **42.2–43.4 · "First edit at turn 3" is destroyed.** L310 `h3.play(tl, SS.hold=14.6, .25, 1.0, 15.3)`: in-animation ends 15.85, out starts 15.3. Fix: session = 8 bars (30.00–47.14); `done` at b25 (13.39), h3 at b26 (13.93), out b31 (16.61); play `(tl, 13.93, .18, .7, 16.61)`. 2.7 s on screen, 1.7 s full.
3. **3.8/17.0/27.6/50.0 · kick grid re-anchors per section** (audio3.py L76, L92, L95, L112: `np.arange(T[x], …, beat)`). Fix: one global grid, `grid=lambda a,b: np.arange(np.ceil(a/beat)*beat, b, beat)`, and snap every `T`/`SS` to the table below so section starts fall on kicks.
4. **0–3.8 · cold open holds 0.8 s.** Fix: open = 3 bars; L249 `b.play(tl, 1.07, .54, .9, 5.9)`; world fade (L375) `P(t, 4.29, 2.0)`; black + one tone for the first bar.
5. **64–72.6 · camera descends through the far city during the numbers** (t069, t071: giant spheres and rails behind "10×" and "0"). Fix: hold `[T.numbers, V3(0,160,60)]` by adding `[T.trust-.001, V3(0,160,60), V3(0,0,-20)]`, then cut to the trust key. Three number cards on bar downbeats (1 bar, 1 bar, 2 bars for "0").
6. **22.4 · instinct hit lands on nothing** (pill fades .2–.7, ring launches at 23.2). Fix: instinct at 23.57; pill at b0; ring (L395) `P(t, T.instinct+1.071, 2.143)` launched by a 2-beat reverse cymbal resolving to a snap; headline at bar 2 downbeat (`a.play(tl, 2.143, …)`).
7. **43.4 · session→evidence is a 0.6 s crane blended with the follow-cam release.** Fix: cut. L228 `wf` release → `P(t, T.evidence-.02, .02)`; `KEYS` L208 becomes `[T.evidence-.001, …]` with the follow-cam end pose, L209 `[T.evidence, V3(-1,22,17)…]`. Cards then land on b1–b4 with ticks.
8. **56.6–57.3 · "Known routes skip the GPU" dissolves over the fleet pull-back** (t057). Fix: L334 out at `T.fleet-T.cool-.6`; camera pull-back is 1 bar (`[T.fleet, V3(0,38,12)]`, `[T.fleet+2.143, V3(10,120,110)]`).
9. **Letterbox 70 px reads as a mistake** (2.04:1). Fix (L419): 138 px (2.39:1); open: `138*(1-P(t,.54,1.6))`; numbers only: `138*eo(P(t,T.numbers,.54))*(1-P(t,T.trust,1.07))` so the bars retract over the first 2 beats of trust, on the cut. Move L345 caption `top:104→172`. Drop bars from trust.
10. **4.1–11.3 · hero steps every 0.30 s fight the 0.536 grid.** Fix: step = beat/2 (0.268): L148 `k*.268-.268`, L255 `/.268`, audio L82 `k*beat/2`. 24 steps = 3 bars exactly; the last step lands on the maze downbeat as the crane completes.
11. **28.6–41.1 · control agent steps every 0.43 s, an accidental near-beat.** Fix: 0.402 s (dotted eighth, 3:4 against the pulse) starting at `T.session+.67`, dying at b23 (12.32) with the 40 Hz thud; L138/L150/L402 `.43→.402`, `1.0→.67`.
12. **Hit inflation.** 13 `hit()` calls, 4 identical risers. Keep three true hits (reflex, cool, close), each with its own low frequency and tail; the rest become snaps, thumps, a reverse cymbal or silence (cue sheet).
13. **79.6 · the ask has no silence before it.** Trust pad (L124) and numbers pad tail (L122, runs to trust+1) overlap; riser at 78.4. Fix: everything stops on the cut at 75.0 except a 55 Hz sine; that stops at 80.36; 2 beats of true silence; hit at 81.43 with the wordmark on `P(tl, 0, 1.07)` (L365, currently `.3`).
14. **88–91 · 3 s linear fade-out.** A pitch film ends on the card and the presenter speaks into a clean stop. Fix: L131 fade → 0.6 s tail at 90.0; let the final C chord's own release do the rest.
15. **24.6–27.1 · "Rails on day one…" (10 words) full for 1.5 s.** Covered by instinct = 3 bars; sub at b6, out b11.

## Revised timeline

| Bar | Start | Len | Beat | Transition in | Holds / internal beats (s, relative) | Sound |
|---|---|---|---|---|---|---|
| 0 | 0.00 | 3 bars | Cold open | from black | line in 1.07→2.7, full to 5.9, world fades 4.29–6.29 | 55 Hz tone at b0; nothing else |
| 3 | 6.43 | 3 | Wander | chase cam blends in over 2 beats | 24 steps × 0.268; counter to 24 at 12.86 | soft kick on 1 & 3, ticks on 8ths |
| 6 | 12.86 | 2 | Maze | crane up (last 2 beats of wander) | ghosts 0–2.1 (stagger .067); counter 16 by b4; trench glows b4–b8 | rcymbal 1 bar into b0; full kick every beat; hats on 8ths |
| 8 | 17.14 | 3 | Reflex | continuous (descent) | pave b0–b4 (1 bar); signals b2,b3,b4,b5; barrier b6; "Nobody wrote a word" b3 | HIT 1 (56 Hz) on b0; riser 1 bar before; pad lift Am–C–G–D; arp 8ths |
| 11 | 23.57 | 3 | Instinct | crane to overhead over 2 beats | pill b0; ring b2→b6; headline b4; sub b6; out b11 | rcymbal 2 beats into b2 + snap; kick continues; arp 16ths from b4 |
| 14 | 30.00 | 8 | Session | dive, 2 beats | see session schedule below | soft kick every beat; snaps on reflexes; thud b23; ding b25 |
| 22 | 47.14 | 3 | Evidence | HARD CUT | headline b0.5; cards b1–b4; caption b6; dim to 40 % during b11 | pad only + kick 1&3; riser b4→b11; SILENCE b11 |
| 25 | 53.57 | 3 | Cool (drop) | HARD CUT | flood b0; headline b1; stats b4–b6; cooling b8→b12; out b11 | HIT 2 (50 Hz); full kick; hats 8ths; sidechain .5; LPF closes b8–b12; hats out b8 |
| 28 | 60.00 | 3 | Fleet | pull-back, 1 bar | headline b4; pulses from b2; counter 1→9 on 8ths b4–b8; sub-line b8 | no kick; rcymbal 1 bar into b0; sub swell attack 1 bar; wide pad |
| 31 | 66.43 | 4 | Numbers | static high wide; bars in 2 beats | −61 % b0, 10× b4, 0 b8 (holds 2 bars); caption b1 | thumps 58/52/46 Hz on b0/b4/b8; low pad |
| 35 | 75.00 | 3 | Trust | HARD CUT; bars retract 2 beats | headline b1; sig box b4; sub b8; out b10–b11 | 55 Hz sine only, attack 1 bar; tick at b4; sine stops b10; SILENCE b10–b12 |
| 38 | 81.43 | 4 | Close | continuous drift | wordmark b0; lamps b2; tag b3; sub b4; pill b6; url b7; chips b8; hold to 90.0 | HIT 3 (52 Hz); Am b0, F b8, C b12; ticks on lamps; clean stop |
| 42 | 90.00 | — | End | cut to presenter | — | — |

Session schedule (relative to 30.00): prompt 0.00 (b0) · stepin 1.07 (b2, green reflex + h1 in at b3, out b10) · red reflex 2.14 (b4) · hop1 4.29 (b8) · tool peek 4.82 · sigB 5.89 (b11) · hop2 7.50 (b14) · hop3 9.11 (b17) · sigC 10.71 (b20, h2 in, out b25) · hop4 11.79 (b22) · control dies 12.32 (b23) · hop5 12.86 (b24) · done 13.39 (b25) · h3 13.93 (b26, out b31) · cut 17.14.

`T = {open:0, wander:6.429, maze:12.857, reflex:17.143, instinct:23.571, session:30.0, evidence:47.143, cool:53.571, fleet:60.0, numbers:66.429, trust:75.0, close:81.429, end:90.0}` · `SS = {prompt:0, stepin:1.071, hop1:4.286, sigB:5.893, hop2:7.5, hop3:9.107, sigC:10.714, hop4:11.786, hop5:12.857, done:13.393, hold:13.929}` in both files.

## Sound cue sheet

| Time | Cue | Instrument · gain | audio3.py |
|---|---|---|---|
| 0.00 | cold tone | pad([55,110], 6.0, .2, .05) | L74: start 0.0, dur 6.0 |
| 6.43–12.86 | half-time pulse | kick(soft) on beats 1,3; tick on 8ths (24), 1400–1800 Hz, .10 | L76 filter `k%2==0`; L82 `k*beat/2` |
| 10.71→12.86 | reveal | rcymbal(4*beat) g .3 | new `rcymbal(dur)`: hat-style noise, env `(x/dur)**3`, ends at 0 |
| 12.86 | maze downbeat | kick + hats 8ths (acc every 4) | delete L83 hit; hats from maze not wander |
| 15.00→17.14 | riser 1 | riser(4*beat) g 1.0 | L85: start `T.reflex-4*beat` |
| 17.14 | HIT 1 | hit(1.0, 56) + air .3 | keep; add `air` param |
| 17.14–30.0 | lift | pad Am,C,G,D one bar each, open .9, g .05; arp 8ths .05 → 16ths .04 from 25.71 | L86–93 on grid |
| 24.64→25.71 | ring launch | rcymbal(2*beat) g .25 → snap at 25.71 | replace L90 hit with `snap()` = tick(2600,.22)+kick(soft) |
| 30.00 | dive | kick(soft) + sub(55, 1.0, g .25) | new `sub(f,dur,attack,g)`: sine, linear attack |
| 31.07 / 32.14 / 35.89 / 40.71 | reflexes fire | snap() (2600 Hz tick .22 + soft kick) | replace L100–101 |
| 34.29, 37.5, 39.11, 41.79, 42.86 | hops | tick(900, .12) on grid | L102 with new SS |
| 42.32 | control dies | hit(.45, 40) — the one thud | L105 → `T.session+12.321` |
| 43.39 | done | snap + sine 220 Hz .4 s g .12 | replace L103 hit |
| 47.14–53.04 | evidence | pad Am/F .04, kick(soft) on 1,3; ticks .08 on card beats | L107–109 + 4 ticks |
| 49.29→53.04 | riser 2 | riser(7*beat) g 1.0, pads at .04 | L111 start `T.cool-8*beat`, dur 7*beat |
| 53.04–53.57 | SILENCE | `out[int(53.036*sr):int(53.571*sr)] = 0` after all adds, before master | new, L129 |
| 53.57 | HIT 2 | hit(1.0, 50), tail 3.5 s | L111; `L=int(sr*3.5)` for this call |
| 53.57–60.0 | drop | kick every beat; hats 8ths, out at 57.86; pad open 1.0 g .06; arp 16ths .05 out at 57.86; sidechain .5 to 57.86 then .25 to 60.0 | L112–118: end at `T.fleet` not `+4`; two sidechain calls |
| 57.86→60.0 | cooling | pad_close: `cut = .012 + .09*(1 - x/dur)` (LPF closes as coolU rises); rcymbal(4*beat) g .3 into 60.0 | new pad variant; add rcymbal |
| 60.00 | fleet | no kick; sub(55, 6.4, attack 2.14, g .35); pad Am–F–C–G open 1.0 g .07 | replace L119 hit; keep pad chain |
| 62.14–64.29 | repos arrive | 8 ticks on 8ths, 900→1600 Hz, g .10 | new |
| 66.43 / 68.57 / 70.71 | numbers | thump(f) = kick + sine f .5 s; f = 58, 52, 46; g .6/.5/.4 | replace L121; L122 pad ends at `T.trust` exactly |
| 75.00 | cut | all pads end here (dur so env closes at 75.0); sub(55, 5.36, attack 2.14, g .18) | L124 replace |
| 77.14 | sig box lands | tick(2200, .16) | new |
| 80.36–81.43 | TRUE SILENCE | `out[int(80.357*sr):int(81.429*sr)] = 0` | new; delete L126 riser |
| 81.43 | HIT 3 | hit(1.0, 52) + sine 220 Hz g .1 | L126 |
| 81.43–90.0 | close | pad Am (4.29), F at 85.71 (2.14), C at 87.86 (2.14 + 1.2 s release); ticks on lamps at 82.5 + j*.134 | L127–128 |
| 90.00 | stop | `fade[-int(sr*.6):]` only | L131 |

## Three signature moments

1. **The pull-out (53.04–53.57).** After a 7-beat riser, one full beat of digital silence: every card and rail on screen dims to 40 %, the counter freezes, the frame goes still. Then the hit, the hard cut to the high wide, and 54 sessions flood the city on frame one. The silence is the drop; the hit is confirmation.
2. **Off the grid (30.0–42.3).** Our agent hops on downbeats; the control agent walks on dotted eighths (0.402 s), a 3:4 polyrhythm the room feels as "wrong" before it can say why. Its 30th step lands on beat 23 with the 40 Hz thud and the hats drop out for that bar; two beats later our goal ring bursts on a clean beat, and the line lands on the next.
3. **The silent downbeat (80.36–81.43).** No riser into the ask. The 55 Hz sine that carried "Deterministic. Text only. Fail-open." cuts out; two beats of nothing; the wordmark and the hit arrive together at 81.43 and the chord walks Am → F → C to a clean stop at 90.0, so the presenter speaks into a resolved room rather than a fade.
