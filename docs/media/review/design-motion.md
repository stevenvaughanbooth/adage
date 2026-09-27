# Design and motion review: Adage film v4

## Verdict

The world is right: one violet rail on near-black, Instrument Serif headlines, real footage as proof. It is not pitch-grade yet: the second half carries a visible rendering bug in every frame, three beats have text colliding with 3D or other text, and the type runs eleven sizes where a launch film needs four. Fix the collisions, halve beat 3, cut beat 5 to three headlines, and it holds a room.

## Top findings

1. **t048 to t079, every frame: a stray leader.** A white dot with a 1.5 px tail hangs at (1310, 612) at 1080p through the route cards, the die, 10× / −61% / 100%, the wordmark and the CTA: a `leader()`/`dot()` whose opacity is never reset once its beat's `upd` stops. Fix: zero all leaders on beat exit.
2. **t053.5, t056: captions behind the die.** "The first sessions are paid for on the GPU…" and "No model. 0.24 s. One 10 MB binary." (`top:250`) project where the die sits. Fix: caption at `top:206`, die 120 px lower.
3. **t023, t079: rail through the brand.** At the reveal the rail crosses the baseline of "Adage" and the red spur cuts the "A"; at the CTA the rail and its amber lamp bleed through the 78%-alpha pill. Fix: rail emissive 25% on both beats, pill 96% alpha.
4. **t033 to t044.5: five zones at once.** Headline, two callouts, transcript, watch panel, turn counter, and the graph. Linear and Stripe never exceed two. Fix: drop `adage watch` from beat 3 (a two-second hero at 46 s instead), transcript 22 px, one callout at a time.
5. **t036 to t044.5: truncated proof.** Watch lines clip at the panel edge ("✓ followed lookup-wc"); at t027 the init panel clips "Found her…". Truncated text reads as fake, and the panel edge (x=1860) is outside 5% title-safe. Fix: shorter strings, panel ends at 1800.
6. **t048 to t050: cards overlap.** `dead-end-d41b1e` (dy −200) lands on the evidence row of `read-linked-files`. Fix: anchor it to the spur's far end.
7. **Type scale.** Serif at 380/320/300/200/170/140/130/120/96/92/84 px; the 84 px "Deterministic…" reads as a demotion at the strongest claim. Fix: 380 hero number, 140 headline, 96 stat, 34 caption, nothing under 20 px (`.hud`, `.ms`, `.tb` at 14 and `.route em` at 13 are 3 px on a phone).
8. **Eyebrow contrast.** `--ink3` on #050507 is about 2.9:1; "MEASURED · 18 TASKS · CLAUDE HAIKU 4.5" at t068.5 is the provenance line and the faintest text in the film. Fix: `--ink2`, 26 px.
9. **Bloom clips the brand.** Threshold 0.28, strength 1.25: the rail core is pure white at t019.5, t042, t066; the green lamp renders teal, the die peach. Fix: threshold 0.6, strength 0.8.
10. **Beat 5 pacing and italics.** Five headlines in 13 s; twelve italicised words in 81 s. Fix: three headlines at four seconds, "Deterministic…" as a CTA chip, italics only on records, predicts, learns.

## Visual system for instincts vs reflexes

- **Two axes, not four colours.** Aspect stays a lamp (green do, amber consider, red avoid). Origin is a stroke: an **instinct** is a filled glyph, a **reflex** an outline whose ring fills with evidence (N of M as arc sweep). A shared track adopted as an instinct: filled glyph, dotted outer ring.
- **Lamps as a signal head.** Stack them vertically (red bottom, green top) like a railway colour-light signal. Horizontal dots read as macOS window controls; the film's own terminal chrome uses the same three dots.
- **Mark.** A monoline switch: one rail, one diverging point, a lamp at the junction. Ident: the wander line straightening into rail. Wordmark stays Instrument Serif, mark left, 0.9 cap-height.
- **On the rail.** Instinct segments are paved from frame one (zero history). Reflex segments are the desire path being paved, the existing 13 to 21 s motif. A dead end is a red spur that recoils: the flinch.
- **In the UI.** Card header: lamp, name, origin glyph, evidence. Replace OBSERVE/REGENERATE/VERIFY with "REFLEX · 23 of 33" / "INSTINCT · shipped". Watch line: "◆ lookup-work-item" vs "◇ 23/33".
- **Copy.** Beat 2 "Ships with instincts.", beat 4 "Learns reflexes.", beat 5 "Reflexes skip the brain."

## Reference points

- **Apple silicon films**: die reveals whose bloom never clips to white; stats held two seconds each.
- **Linear 2024 launch films**: UI at 1.5× and cropped, never shrunk; nothing below 22 px (beat 3).
- **Stripe Sessions openers**: type-first, one italic per film, captions never over 3D (beats 1, 5).
- **Vercel Ship**: tracked mono eyebrows at legible contrast (provenance line).
- **Raycast, Warp**: terminal as hero at 24 to 28 px.

## Questions for the founder

1. Will this run on phones or social? Beat 3 cannot survive 9:16.
2. Is origin (instinct/reflex) or aspect (green/amber/red) the primary axis in the product UI?
3. "Yellow" (transcript) or "amber" (cards)?
4. Does the pitch room have sound?
5. Should the film match `adage graph --serve` so the demo continues it?
6. Mark for the pitch, or wordmark only?

## Must change before the pitch

1. Kill the stray leader (1).
2. Captions out from behind the die (2).
3. Dim the rail under wordmark and pill (3).
4. Fix truncation (5) and card overlap (6).
5. Provenance eyebrow contrast (8).
6. Retune bloom (9).
7. Copy to instincts and reflexes on beats 2, 4, 5.
