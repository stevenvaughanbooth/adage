# v5 · camera, lensing, lighting, atmosphere

## Verdict

The world is built; the photography is not. One lens (fov 44) for 91 s, a camera that comes to a full stop at all 17 keys (`keyCam` eases `eio` per segment), three lights that cannot shape anything (nodes, rails and agents are `MeshBasicMaterial`; only the flat ground takes light, and a directional on a plane is a constant), fog that is a switch not a grade, and a close where three horizontal rails saw through the wordmark. The 57–73 s fleet is the weakest reel: nine dotted rugs in a void, then an 8.6 s descent through 0.34-radius "far" geometry that turns 69–72 s into a wall of grey balls. Fix the rig (30 lines), give every beat its own lens and fog, replace the descent with a cut, and land the close on the rail axis. Nothing here needs assets.

## Findings

1. **64.0–72.6 s · descent through far-scale geometry.** `KEYS` goes (0,160,60) → (goal−6, 4.5, goal+9) over 8.6 s while `far` (line 406) keeps 0.34 spheres and 0.34 tubes until `T.trust−.2`. t069/t071 are unreadable. Fix: hold high, dolly-zoom (finding 4), then **hard cut** at `T.trust`: insert `[T.trust−.001, V3(0,212,339), V3(0,0,−8)]` and `[T.trust, V3(goal.x−7, 5.5, goal.z+8.5), V3(goal.x, −.3, goal.z−1)]`; in `keyCam`, when `b−a < .01` return the later key with no interpolation. Swap `far` at `T.trust−.001`.
2. **All keys · stop-start.** `s = (i+eio(u))/(n−1)` zeroes velocity at every key (visible at 20.2, 22.4, 26.8, 55.5, 58.1, 64). Fix: mark each row `hold:true|false`; for through-keys use `u` (linear in spline parameter), for hold-keys `eio`; and set `posCurve/lookCurve` to `"centripetal", .35`. Holds: 0, T.maze, T.instinct+4.4, T.cool+5.5, T.numbers, T.close.
3. **57–64 s · cities read as rugs.** Camera (10,120,110) at 47° elevation, fog .002, no floor, no height, arcs peak +6–10. Fix: elevation 32° and a long lens: `[T.fleet+1.5, V3(0,150,240), V3(0,0,−8)]` fov 28; fog .0045 (far row at ~40%: depth cue); a fleet floor: `PlaneGeometry(1400,1400)` `MeshBasicMaterial({map: canvas radial 0x120f28 → 0x050507, transparent})` at y −.4, `visible = fleet.visible`; a beacon per city: `CylinderGeometry(.5,.5,16)` `MeshBasicMaterial({color:0x8b7cff, transparent, opacity:.22, blending:AdditiveBlending})` at `nodes[K.start]`, scaleY = `eo(arrive)`; link arcs `mid.y = 18 + rnd()*8`, tube radius .45; far node colors ×1.6 (`tmp.multiplyScalar(a*1.6)` in the FL loop). Start the crane at `T.cool+5.5` so it takes 3.5 s not 1.5 (t057 currently shows the crane whipping through the "GPU" title).
4. **57–64 s · no dolly-zoom where the film is asking for one.** "A reflex learned once travels everywhere": pull back along the camera axis while narrowing fov so the hero city holds size and the eight neighbours slide in. `d·tan(fov/2)` constant: `[T.fleet+1.5, (0,150,240)] fov 28` → `[T.numbers, (0,212,339)] fov 20`, linear in `t`, look fixed (0,0,−8). Hold that frame for the numbers.
5. **4–12 s · rigid chase.** `chaseCam` tethers 5.2 back / 2.8 up on the current point, so the agent sits at the same pixel (640,470) for 8 s and clips white (t009). Fix: sample the curve 0.6 s late for the camera, now for the look: `p = heroCurve.getPointAt(clamp(hu−.035))`, `pos = p − dir·4.2 + (0,1.6,0) + right·1.4`, `look = heroNow + dir·1.2`; fov 56 (28 mm: speed, no foresight). Agent core `0xf2eeff`, halo opacity `.2·a` not `.25`.
6. **0–4 s · horizon through the headline.** t003: horizon at 44% of frame, "has" sits on it. Fix: `[0, V3(start.x−2, 3.2, start.z+8), V3(start.x+6, −.2, start.z−3)]` fov 38 → horizon at ~13% (140 px), type on fogged ground. Same rule for 17–22 and 73–80: keep the horizon above 9% or out of frame (pitch > fov/2). Trust key above (finding 1) pitches 26° at fov 36: horizon out.
7. **28–43 s · the pool of light.** `lamp` 3.0 at 1.4 m, distance 14, on `0x0a0a12` metalness .05: a hard violet disc that owns the left half (t031–t041) and swallows the counters. Fix: 1.8 at height 2.0, distance 10, `0xb8acff`; ground `0x07070d`, roughness 1, metalness 0; ground `map` = 1024² canvas radial (255 centre → 40 edge) so the pool reveals a floor gradient not a disc. Wander lamp: `0xff9a60`, 1.8, height 1.6 (the pink-brown blob at t007 is warm lamp × blue hemisphere; drop hemisphere sky to `0x1c1838`).
8. **17–22 s · pave head over-lit.** `lamp.intensity = 6` (line 391) plus threshold .6 = a blown blue moon (t019). 3.5, height 1.4, distance 12, bloom threshold .78.
9. **Bloom.** .75/.6/.6 clips every white core (agent, paveHead, rail `core`, fleet `pulse` r 1.1). Default `.55/.45/.78`; cool→numbers `.75/.6/.70`; fleet `.70/.70/.65`. Set `bloom.threshold` in `world()` alongside strength.
10. **Session push too small to read.** `cam.fov = 44 − 3·push`: 7%. Make the session lens 40 with a 40→35 push over .6 s at `SS.stepin`, return over 1.0 s at `SS.hop1`; add a 2° kick (35→37→35, .35 s) on each `HOPS` entry.
11. **Near-lens blobs.** t003 bottom-left, t069: nodes inside 2 units of the lens. `cam.near = .5`; in the node loop scale the instance to 0 when `nodes[i].distanceTo(cam.position) < 2.5`.
12. **80–91 s · rails through the wordmark.** Three horizontal rails cross "Adage", the lamps and the tagline; the red spur is the most saturated thing in the end card. See Signature 2.

## Lighting and grade rig

```
renderer.toneMappingExposure = 1.0
HemisphereLight(0x1c1838, 0x000000, .28)
key  = SpotLight(0xd8d2ff, 1.4, 90, .55, .8, 1.6) at (-10,34,8), target (2,0,0)   // the map has a centre
back = SpotLight(0x6a5cff, .5, 120, .7, 1, 1.4)  at (10,12,-40), target (0,0,0)   // cool halo on far ground
lamp: wander 0xff9a60 1.8 d10 h1.6 · pave 0xd6ccff 3.5 d12 h1.4 · session 0xb8acff 1.8 d10 h2.0 · trust/close static 0xb8acff 1.2 at goal+(0,2,0)
ground: MeshStandardMaterial({ color:0x0e0e18, map: radial canvas 255→40, roughness:1, metalness:0 })  y −.25
fleet floor: MeshBasicMaterial radial 0x120f28→0x050507, 1400², y −.4
bloom: .55/.45/.78 default · .75/.6/.70 cool→numbers · .70/.70/.65 fleet
agent core 0xf2eeff, halo .2·a
fog (FogExp2 0x050507) per beat: open .035 · wander .040 · maze .010 · reflex .022 · init .008 · session .020→.045 (line 410: lerp over 28–41, so the control agent vanishes into the dark) · evidence .010 · cool .008 · fleet .0045 · numbers .0035 · trust .026 · close .018; lerp between beats over .8 s
```

## Camera plan

| Beat | fov | Move | Position | LookAt |
|---|---|---|---|---|
| 0–3.8 open | 38 | static, horizon 13% | (start.x−2, 3.2, start.z+8) | (start.x+6, −.2, start.z−3) |
| 3.8–12 wander | 56 | lagged chase (0.6 s), hero left-of-centre | heroCurve(hu−.035) − dir·4.2 + (0,1.6,0) + right·1.4 | hero + dir·1.2 |
| 12–17 maze | 34 | slow plan push, through-key | (0,30,6) → (0,28,5) | (0,0,−1) |
| 17–20.2 reflex | 50 | on the rail, head comes at lens | (goal.x−3, 1.0, goal.z+1.6) | (start.x+4, .3, start.z+.6) |
| 20.2–22.4 | 50→44 | hold, slight lift | (goal.x−3, 1.6, goal.z+2.2) | same |
| 22.4–27.6 init | 30 | plan, hold at 26.8 | (0,42,4) → (−1,38,6) | (0,0,−1) |
| 27.6–43.4 session | 40, push 35 | followCam offset (1.2,6.4,10.2), look +(4.2,0,−1) | rail point | rail point |
| 43.4–50 evidence | 34 | rise, through | (−1,24,15) | (−1,0,−2) |
| 50–55.5 cool | 30 | plan, hold | (0,36,10) → (0,40,9) | (0,0,−1) |
| 55.5–58.1 crane | 30→28 | 3.5 s crane up-back | → (0,150,240) | (0,0,−8) |
| 58.1–64 fleet | 28→20 | dolly-zoom, linear | (0,150,240) → (0,212,339) | (0,0,−8) |
| 64–72.6 numbers | 20 | hold | (0,212,339) | (0,0,−8) |
| 72.6 cut / trust | 36 | static, horizon out | (goal.x−7, 5.5, goal.z+8.5) | (goal.x, −.3, goal.z−1) |
| 79.6–91 close | 32 | 12 s creep down the rail axis | (goal.x+8, 1.6, goal.z+.6) → (goal.x+6.4, 1.6, goal.z+.5) | (start.x+6, 2.6, start.z) |

## Three signature shots

1. **Headlight (17.0–19.0).** Camera sits on the rail at head height behind the goal, looking back down the trench. The pave head comes straight at the lens with the lamp at 3.5, bloom .78 threshold so it flares only in the last metre, passes under the camera at 18.6 s (`eo3(u)=.9`), and the lamp cuts to 0 as it clears; the four signals light up in the vanishing point behind it. A train passing a platform.
2. **On rails (79.6–91).** The close camera lies on the rail axis 8 units past the goal, pitched 1.4° up, so both the spine and the instinct rails converge to one vanishing point at y 585 px, exactly where the three lamps sit. The wordmark is on black sky, the rail runs under it into the dark, and the red spur is a small mark off to the left. Twelve-second creep, no keys in between.
3. **The world gathers (58.1–64).** The dolly-zoom: hero city locked, eight cities and their rails slide inward as the lens goes 28→20 and the fog thins .0045→.0035. The "9 repos on rails" counter lands as the last city settles.
