# v6 · audio note (audio5.py → soundtrack5.wav)

Stereo, 16-bit, 48 kHz, exactly 90.000 s, deterministic (seed 2026, byte-identical across runs), renders in ~28 s on CPU with numpy + scipy only. `T` and `SS` are unchanged from audio4.py; every cue in the v5 cue sheet lands at the same time. Both true silences (53.036–53.571, 80.357–81.429) are digital zero, forced after the master chain so no reverb tail leaks into them.

## What was built

A small production toolkit instead of naive oscillators:

- **Drums.** Kick = pitched sub sine (3x pitch drop over 24 ms, soft-clipped) + noise click + 240 Hz body burst; soft variant for the wander/session pulse; every kick gets ±1 % pitch and ±5 % velocity so no two are identical. Clap = four staggered bursts through a 1.5 kHz band-pass + a 320 Hz body; finger snap = 2.4 kHz + 5.2 kHz bursts + a 190 Hz knock; hats = high-passed noise with a 10.5 kHz peak, ±12 % velocity, ±2 ms timing, 3 % swing on the off-beats, an open hat on the last 8th of each bar.
- **Synths.** Pads are six detuned saw+triangle voices per note (−11/−3/+6 cents left, −7/+2/+11 right, DPW anti-aliased) through a resonant RBJ low-pass whose cutoff opens across each chord (or closes, for the cooling city), sine attack / cosine release. Arp is a synth pluck (two detuned saws + sub-octave triangle, fast-closing resonant filter), velocity ±8 % with accented downbeats, humanized ±2.5 ms, 4–6 % swing, panned across the field. Lead is a 2-op FM "glass" tone, detuned ±5 cents L/R. Bells and ticks are FM. Sub bass is a soft-clipped sine with a 2nd harmonic, one note per bar following the chord roots, sidechained.
- **FX.** Riser = noise band sweeping 260 Hz → 6.8 kHz + a detuned saw rising 55 → 440 Hz through an opening low-pass + 40 accelerating ticks. Reverse cymbal = bright noise + five inharmonic partials with a rising envelope that ends at zero. Three hits, each distinct: HIT 1 (56 Hz, 2.6 s, bright 5.2 kHz air, medium grit), HIT 2 (50 Hz, 3.5 s, widest air with an 11 ms Haas offset, full grit, the only one that fully pins the limiter), HIT 3 (52 Hz, 3.0 s, softer click, a 220 Hz tonal bloom, longest hall). The 40 Hz thud at 42.328 is a fourth, small "hit" with almost no air.
- **Space.** Two convolution reverbs from synthesized IRs: hall 3.2 s (22 ms pre-delay, three bands with their own decay so the tail darkens, six early reflections) and room 0.7 s for drums; a dotted-eighth (0.402 s) ping-pong delay on the arp and lead. Stereo width from L/R detune spread, Haas offsets on hit air, mid/side widening on the drop, fleet and close pads. Sub and kick stay mono and center.
- **Master.** Everything except sub/kick is high-passed at 58 Hz (wet returns at 130 Hz), −1.5 dB mud cut at 300 Hz, tilt (−1.5 dB shelf at 220 Hz, +1.5 dB shelf at 3.4 kHz), gentle bus compressor (1.5:1, −20 dB, 100 ms RMS detector so transients pass), then a 4x-oversampled lookahead brickwall limiter at −1 dBTP. Sidechain envelopes: 0.3 under the lift, 0.18 under the session, 0.5 then 0.25 in the drop.

## The motif

E4 – A4 – C5 – B4 (up a fourth, up a minor third, step down onto the tension note). It appears:

- 19.29 (reflex + 1 bar) on the glass lead over the lift, and answered at 25.71 (ring launch) as E–A–C–**E5**, ending high and unresolved.
- 34.29 (hop1) and 40.71 (sigC) as a muted, half-speed quote under the session; in the second quote the B (the tension note) lands at 42.32 exactly as the control agent dies on the 40 Hz thud, and the ding at done (43.39) is A5: the motif resolves as our agent finishes.
- 55.71 (drop, bar 2) in two octaves; the drop's 16th-note arp is built from the same A-minor spelling.
- The close: the three lamp bells at 82.73/82.85/82.97 are E5–A5–C6 (the motif's head), then the lead plays E (83.57) – A (84.64) – C (85.71, on the F chord) – B (86.79) – **A** (87.86, on the C chord), held to the clean stop. The film ends on the resolution.

## Mix decisions

- Integrated loudness −17.6 LUFS (BS.1770) with the loud sections at −14 to −16 LUFS short-term / −15 to −16.5 dBFS full-band RMS; the film's dynamic shape is intact (cold open −28 LUFS, trust −32 LUFS, drop −14). If the presenter's room is loud, raise `MASTER` in audio5.py by up to 0.1; the limiter holds the ceiling.
- Hits peak at −1.0 / −1.6 / −1.3 dBFS (true peak −1.0 dBTP); the limiter works only on the hits (max 1.1 dB), the compressor glues the drop by ≤ 5 dB on the hit moment and 1–2 dB elsewhere.
- Low end is deliberately dominant but not everything: bus trims keep < 150 Hz at about −1 to −3 dB of the section energy in the loud sections instead of −0.5 dB (the first pass was 90 % sub).
- Chords are voiced higher than audio4 (Am = A2 E3 A3 C4 E4, D = D3 A3 D4 F#4 A4 so the lift actually lifts); the sub carries the roots (A1 55, C2 65, G1 49, D2 73, F1 44 Hz).
- The reverb is a send, not an insert: pads ~ −8 dB wet, the close ~ −2 dB wet (the wordmark blooms), the numbers pad has no hall so the cut at 75.0 is dry and hard.

## What the picture can sync to (new or changed)

- **Backbeat.** Claps on beats 2 and 4 from 19.29 to 30.0 (soft) and 53.57 to 57.86 (full). Nothing in the picture needs to change; if a signal flash wants a beat, use these.
- **Control agent's steps are now audible**: a dull wood tick every 0.402 s from 30.67, 29 ticks, panned left; the 30th step is the thud at 42.328. The picture already steps at 0.402 s so this syncs by construction; keep `.67` and `.402` exact.
- **Hats out for the thud bar**: 41.79–43.93 (session b22–b26), as the review asked.
- **Done ding** at 43.393 is now a pitched A5 bell + snap (was a 220 Hz sine); h3 at 43.93 lands inside its ring.
- **Fleet bells** at 62.14 + j·0.268 rise A5 → A6 (880 → 1760 Hz); the counter 1→9 on 8ths matches them.
- **The cooling**: the pad low-pass closes from 57.86 to 60.0 and the reverse cymbal ends exactly at 60.0; the hats and arp stop at 57.86.
- **The close**: the motif's resolution is at 87.86 (close b12, the C chord). If the chips (b8, 85.71) or the URL want a final settle, 87.86 is the note the room hears as "done"; the 0.6 s fade at 89.4–90.0 is the only fade in the film.
- The trust section is a 55 Hz sine only (attack 1 bar) plus one bell at 77.14; it ends at 80.357, two beats of nothing, HIT 3 at 81.429 with the wordmark. Unchanged, and the silence is real.
