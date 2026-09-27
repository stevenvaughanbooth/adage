# v5 · colour, contrast, grade, material

Probed the stills (ffmpeg → RGB). All hexes below are measured or computed against index5.html.

## Verdict

The film is graded like a good UI, not a launch film. Every 3D colour goes through `lin()` then ACES, and ACES lifts and desaturates the midtones: brand violet #8b7cff lands on screen at #a895d4 (h258 s30 v83, lavender); heat 0xff6a2a lands at #c8935a (caramel) and, where sixteen lines overlap (t015–t019), #834f32 (brown). The white cores never reach white (ACES caps 1.0 at 230). Meanwhile the fog and clear colour skip `lin()`, so the "sky" renders at #111218 while the lit floor is #03040f: the void above is lighter than the ground below (t003–t011, t073–t075). The DOM lamps, type and wordmark are exact brand values, so the overlay layer is more saturated and whiter than the world behind it and floats off it. The close (t081–t089) is the best-graded stretch, true black, clean rail; the cold open and wander, the twelve seconds where the room decides, are the cheapest. The fix is one principle and one pass: pre-compensate every scene colour for ACES (HDR triples, skip `lin()`), fix fog/clear/ground, and add a short grade ShaderPass.

## Findings

1. **0–12 s, 73–75 s · lifted sky, inverted horizon.** Fog and clear colour 0x050507 are treated as linear and encode to #111218 pre-vignette; the ground (0x0a0a12, also raw) is a blue albedo that goes darker than the sky. Fix line 99 and 101: `renderer.setClearColor(new THREE.Color(.007,.007,.010))`, `scene.fog = new THREE.FogExp2(new THREE.Color(.007,.007,.010).getHex(), .028)` (renders #050507). Ground line 112: `color: new THREE.Color(.05,.048,.05), roughness: .55, metalness: 0`. Hemi (line 107) sky 0x1a1630, ground 0x050508, intensity .25. The floor then reads as a floor because light pools sit on it, not because it is ambiently grey.
2. **5–11 s · the heat pool is maroon (#4b2230, h340).** Orange lamp 0xffb08f on the blue albedo gives magenta. Line 382: lamp `setRGB(1.0,.32,.10)`, intensity 1.8, distance 12, decay 2, and the neutral ground above. Target pool: #5a2a12 ember-brown, not wine.
3. **17–89 s · violet reads lavender.** Rail sleeve 0x8b7cff → #a895d4 on screen. Line 106/166: `railMat.color.setRGB(.19,.13,1.35)` (no `lin()`; luminance .23, below bloom threshold so the sleeve never blooms), instinct rails `setRGB(.12,.08,.90)`, `cCool.setRGB(.12,.09,1.0)`, fleet links the same as the rail. On-screen ≈ #9176f0 (h249 s51). Core `setRGB(1.6,1.58,1.7)`, radius .045 → .035: white stays a thread inside the violet instead of bleaching it.
4. **4–17 s, 50–55 s · heat is caramel/brown, so ember-vs-violet is pastel-vs-pastel.** Both colours sit at v60–80 s30–55 and the "cooling" is a hue change with no temperature. Line 162–163: hero `setRGB(1.5,.14,.03)` opacity .9; ghosts `setRGB(1.1,.10,.02)` opacity .55 (overlap then sums to ember, not mud); control `setRGB(1.4,.16,.04)`; ghost agents `setRGB(1.6,.4,.15)`. On-screen ≈ #f06e2c (h20 s83). DOM: `--heat` and the stat colour "#ffb08f" (line 334) → brand #ff7a59.
5. **19–27 s, 43–49 s, 73–81 s · 3D lamps are matte discs.** Green sphere #3f8f5f flat, red barrier salmon (#e67c8f). Pre-compensate: green `setRGB(.05,.95,.14)`, amber `setRGB(1.3,.38,.03)`, red `setRGB(1.5,.12,.16)` (land on #4ade80/#fbbf24/#fb7185); add a white-green core r .08 `setRGB(1.6,1.8,1.6)` inside each signal so it blooms like the DOM lamp does.
6. **57–72 s · the fleet looks cheaper than the street.** Fog .002 kills depth so every city is the same v60 grey-lavender; bloom .95 smears thin rails to milk; far nodes carry a pink confetti (`cHeat` at .55, line 407); t071 flies through faceted .34 spheres with no black in frame. Fix: fog .006 at fleet, .0045 at numbers (with the fog fix, far cities fade to black = depth); bloom (.6,.9,.72) there; per-city `a *= 1 - .45*clamp((f.dist-40)/120)`; heat lerp .25 with the ember triple; switch `far` on `cam.position.y > 30`, not on time.
7. **whole film · vignette and grain sit above the type.** DOM order (line 78) puts `#vig` and `#grain` over `#ov`: the counter at right:120/top:92 and the bottom stats lose ~.3 stop, and overlay grain sparkles on white glyph edges. Move both between `#gl` and `#lead`. Vignette: `radial-gradient(1700px 1100px at 50% 50%, transparent 55%, rgba(0,0,0,.35) 100%)` (−0.6 stop at the corner instead of −1.15). Grain: static tile = dirty screen; make it noise 0–28, `mix-blend-mode: screen`, opacity .6, `background-size: 384px`, and in `seek(t)` offset `backgroundPosition` by a seeded per-frame jitter. Black floor becomes #070709 with live grain, corners back to #050507.
8. **type white vs scene white.** `--ink` #f4f4f7 vs 230 scene white. The HDR cores (finding 3) close the gap; keep `--ink`, drop `.serif` text-shadow to `rgba(139,124,255,.18)`, and the halo spheres (line 180) `.25*a` → `.12*a`, tinted to the lamp colour, so the agent is a bloomed point, not a flat white disc (t007–t011).
9. **bloom.** (.75,.6,.6) → base (.55,.85,.72); wander (.6,.7,.65); cool/fleet (.6,.9,.72). Higher threshold plus low-luminance violet means only cores, lamps and ember bloom. Exposure stays .95.

## The grade

| role | now (input) | on screen now | new input (linear, no `lin()`) | on screen target |
|---|---|---|---|---|
| night / fog / clear | 0x050507 raw | #111218 | setRGB(.007,.007,.010) | #050507 |
| ground | 0x0a0a12 r.9 m.05 | #03040f / maroon pools | setRGB(.05,.048,.05) r.55 m0 | #06060a, ember/violet pools |
| rail sleeve | 0x8b7cff | #a895d4 | setRGB(.19,.13,1.35) op 1.0 | #9176f0 |
| rail core | 0xffffff | #e2e0fc | setRGB(1.6,1.58,1.7) r.035 | #f4f4f7 + bloom |
| instinct rail | 0x8b7cff op .7 | #74689c | setRGB(.12,.08,.90) op .8 | #6f5cff |
| heat line | 0xff8a4a / 0xff6a2a | #c8935a / #834f32 | setRGB(1.5,.14,.03) / (1.1,.10,.02) | #f06e2c |
| heat lamp | 0xffb08f i2.4 | maroon | setRGB(1.0,.32,.10) i1.8 d12 | #5a2a12 pool |
| session lamp | 0xc9bfff i3.0 | #4a4586 pool | setRGB(.55,.45,1.2) i2.6 | #3d3390 pool |
| green / amber / red (3D) | brand hex | #3f8f5f / – / #e67c8f | (.05,.95,.14) / (1.3,.38,.03) / (1.5,.12,.16) | #4ade80 / #fbbf24 / #fb7185 |
| DOM heat | #ff8a5c, #ffb08f | as is | #ff7a59 | brand GPU |
| exposure | .95 | | .95 | |
| bloom (str, rad, thr) | .75/.6/.6 · .95 cool | | .55/.85/.72 · .6/.9/.72 cool–fleet | |
| fog density | .028 → .012 → .002 | | .028 → .012 → .006 fleet → .0045 numbers | |
| grain | overlay .07 static | invisible in blacks | screen .6, noise 0–28, 384px, jittered | #070709 floor |
| vignette | .55 at 100% (−1.15 st) | | .35 at 100%, 1700×1100, from 55% (−0.6 st) | |

Grade pass, after bloom (`composer.addPass(new THREE.ShaderPass(GRADE))`, uniforms `tDiffuse`, `uCool` 0→1 across T.cool+3 … +5.2):

```glsl
uniform sampler2D tDiffuse; uniform float uCool; varying vec2 vUv;
const vec3 LIFT=vec3(.004,.003,.010), GAMMA=vec3(.96,.96,.94), GAIN=vec3(1.04,1.03,1.06);
const vec3 SHADOW=vec3(.05,.03,.14), HIGH=vec3(1.0,.99,.98);   // violet-black toe, warm-neutral white
void main(){
  vec2 d=vUv-.5; float r2=dot(d,d);
  vec3 c; c.r=texture2D(tDiffuse,vUv+d*r2*.004).r; c.g=texture2D(tDiffuse,vUv).g; c.b=texture2D(tDiffuse,vUv-d*r2*.004).b; // 1.5 px CA at the corner, 0 at centre
  float l=dot(c,vec3(.2126,.7152,.0722));
  float mid=smoothstep(0.,.25,l)*(1.-smoothstep(.55,.95,l));
  c=mix(vec3(l),c,1.+.35*mid);                                   // give back what ACES took, midtones only
  c=pow(max(GAIN*(c+LIFT*(1.-c)),0.),1./GAMMA);                   // lift / gamma / gain
  c+=SHADOW*(1.-smoothstep(0.,.18,l))*.6;                          // split tone: shadows
  c*=mix(vec3(1.),HIGH,smoothstep(.6,1.,l));                       //             highlights
  vec3 warm=vec3(.94,.43,.17), cold=vec3(.55,.49,1.0);             // ember → violet two-tone for the cooling city
  float w=smoothstep(.1,.5,c.r-c.b);                                // warm mask
  c=mix(c,mix(c,l*cold/.6,w),uCool);
  gl_FragColor=vec4(c,1.);
}
```

No lens dirt: on a clean-black deterministic frame it reads as a texture overlay. CA at 1.5 px corner-only and the streak below are enough "lens".

## Three signature looks

1. **Light on black.** Cold open and wander with hemi at 0, sky and fog at true #050507; the only light in the first eleven seconds is the ember lamp carving a pool on the floor under the agent, the floor exists only where a thought lands. Ramp hemi .0→.25 over T.maze…T.maze+3 as the camera lifts. The Apple move: the product is the first light source.
2. **Ember to ice.** The cooling city (50–57 s) becomes a literal temperature change through `uCool` in the grade pass: warm mask hue-rotates to violet, the toe turns violet-black, the numerals cross-fade #ff7a59 → #c9bfff on the same curve. One frame, two colours, one story.
3. **Anamorphic thread.** A second ShaderPass, 9-tap horizontal blur of luminance >.72, ±14 px, tinted toward #8b7cff at the tails, added at .35: every white core, lamp and pave head grows a short violet streak, so the rail reads as a filament, the lamps as lamps, and the wordmark's bloomed neighbours match its DOM glow.
