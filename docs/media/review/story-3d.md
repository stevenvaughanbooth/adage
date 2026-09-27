## Verdict
The film has a real spine (a recorded session beating its control) and one graphic idea worth keeping: sessions trampling a desire path that gets paved into a rail. It is three films (railway world, silicon props, terminal chrome), states its proof three times, and has no lighting, so the 3D never reads as frontier-grade. Rebuild the middle around "reflexes skip the brain" as one shot on one map.

## The arc as it stands
Want: finish W-2045. Obstacle (4–13 s): 24 wandering turns, pytest dead end. Turn (13–21 s): records, predicts. Proof (30–46 s): first edit at turn 4, restated as route cards (46–52), GPU→CPU (52–65), 10×/−61%/100% (67–74). Ask (74–81). It peaks at 21 s and never tops the 45 s "Not 24" line.

## Story findings
1. **Proof triple-stated.** Turns/seconds/cost appear at 56–66 s and again as numerals at 68–73 s; "5 ms" at 27, 33, 42 and 59 s. Each number once, big.
2. **Two title cards.** ADAGE + lamps + tagline at 21.4 s (t023) and 74 s (t076). The first ends the film early. Keep the closer.
3. **Wrong cold open.** "Your agent forgets everything" is the generic memory-tool claim and invites "CLAUDE.md exists." The film shows thinking about every step; open on "Your agent has no reflexes."
4. **Comet duplicates the session.** 18–21 s predicts the route the 30–45 s session replays. Cut it, or fold its mechanic into the session.
5. **Three worlds.** Grid, desire path, rail, lamps (railway aspects) and "signal box" are one railway world. Die and chip (52–65) are a second; terminals with traffic-light dots (t027, t031) a third, colliding with the brand lamps (t023, t076).
6. **Primitives as spine.** I: no reflexes, the map burns. II: `adage init` lays instincts on a dark map. III: a trampled path becomes a reflex; the pytest flinch too. IV: reflexes skip the brain, the map cools. V: numbers, ask.
7. **Orphan trust line.** "Deterministic. Text only. Never runs a thing." (65 s) has no picture; dock it on the rail as a `<adage-signal>` block.

## Proposed beat list
| Time | Beat | On-screen line | What the 3D does |
|---|---|---|---|
| 0–4 | Cold open | Your agent has no reflexes. | Black; grid fades in. |
| 4–12 | Wander | Every step is a thought. | Chase cam; each turn flares a node orange. |
| 12–17 | Same maze | Same maze. Every session. | Crane up; 16 ghosts in 1.5 s; heat piles into one trench. |
| 17–22 | Reflex | Adage learns a reflex. | Pave wavefront start→goal; signals rise; red barrier locks. |
| 22–26 | Instincts | Ships with instincts. Learns reflexes. | A second rail snaps on across the dark map. |
| 26–30 | Install | Two commands. | Init wizard as a world-space plane; hooks snap on like ties. |
| 30–46 | Session | Known pattern → First edit at turn 4. Not 24. | Follow cam; the segment ahead ignites before the agent moves. |
| 46–52 | Skip the brain | Reflexes skip the brain. Known routes skip the GPU. | Map cools from orange to violet rail; one stat row crossfades. |
| 52–58 | Evidence | Every reflex carries its evidence. | Overhead; four cards; bars grow on the hit. |
| 58–64 | Compounding | More sessions. Less GPU. | Counter 1→54; heat decays per session; chart. |
| 64–72 | Measured | 10× / −61% / 100% | Top-down over the rail, one numeral each. |
| 72–77 | Trust | Deterministic. Text only. Fail-open. | `<adage-signal>` block on the rail. |
| 77–85 | Ask | ADAGE · lamps · pill · URL | Rail runs into the wordmark; lamps go green on beat. |

## 3D set-piece critique
Every mesh is `MeshBasicMaterial`, no lights, shadows or ground; bloom does all the lighting, so the rail clips to white (t019.5) and chip pins read as a comb (t059). Camera is all 3/4 crane, eased per segment, so it stop-starts (three swings at 13–18 s).

**Wander (t006, t010).** Works: real tool tags, ticking counter. Fails: one smooth ribbon, no cost felt; 14 px tags. Build: rail-height chase cam; each turn an orange flare that lingers.

**Fusion (t014, t016).** Works: the fan is the strongest frame. Fails: ghosts stagger 3.5 s (t010 is empty); overlap reads as lasers, not trampling. Build: fixed top-down, all ghosts in 1.5 s, additive heat on one trench.

**Paving (t019.5).** Works: snappy reveal. Fails: 0.7 s is too fast; core blows out. Build: a lit head runs the pave, camera racks with it, signals rise mechanically.

**Comet (t019.5–t023).** A caterpillar of 12 spheres, and a duplicate. Cut.

**Docked panels (t027–t044.5).** Works: real footage. Fails: flat DOM, no parallax; init panel mostly empty at t027; each event shown three times. Build: transcript on a CSS3D plane; callouts only; watch as a ticker.

**Routes overhead (t048, t050).** Cards collide (read-linked-files over dead-end). Build: collision-avoiding layout.

**Die and chip (t053.5–t066).** Fails: a floating LED billboard, no scale or ground contact; captions and chart overlap the die (t053.5, t056, t062); rail glow eats the "GPU" label (t066). Build: no props; the map is the silicon: nodes burn orange under inference, cool to violet under the rail.

**Closing (t076, t079).** Pill covers the rail and the amber lamp; lamps read as window chrome. Build: rail ends in the wordmark, lamps go green in sequence.

## Three shots that would make people share it
1. **The cooling city.** Top-down; 54 sessions replay in 3 s, the grid glows orange; as rails lock in, heat drains until only violet rail remains.
2. **Lights ahead of the train.** Rail-level camera behind the agent; the signal ahead flips green and the next segment ignites before the agent moves.
3. **Instincts on day one.** Dark map; `adage init` lands and rails snap on in a wavefront; then a trampled path becomes a rail.

## Questions for the founder
1. Which demo routes are instincts vs reflexes; what ships for a fresh repo?
2. Are the lamps intentionally railway aspects; may the film say "aspect"?
3. Training cost $4.87. Can we show break-even?

## Must change before the pitch
- Fix the leaked leader line and dot at (873, 408) from 46.5 s on: leaders live in `LEAD`, not the beat node, so the goal peek never hides.
- Light the scene: key light, ground plane, emissive rail, higher bloom threshold.
- One wordmark card, at the close; each number once; copy on the primitives; fix caption occlusion, card overlap, pill-over-rail.
