// The hero: a brain and spinal cord drawn with three.js. Context lives in the brain as named regions
// joined by tracts; instincts and reflexes are arcs down the cord. Ported from the hero prototype
// (three r128 via CDN) to the module build; the look is unchanged.
import * as THREE from "three";
import { EffectComposer } from "three/examples/jsm/postprocessing/EffectComposer.js";
import { RenderPass } from "three/examples/jsm/postprocessing/RenderPass.js";
import { UnrealBloomPass } from "three/examples/jsm/postprocessing/UnrealBloomPass.js";

THREE.ColorManagement.enabled = false;

/* Improved Perlin noise (Ken Perlin, 2002), seeded: the same folds on every load. */
function makeNoise(seed) {
  const p = new Uint8Array(512);
  const base = [...Array(256).keys()];
  let s = seed;
  const rnd = () => ((s = (s * 16807) % 2147483647) / 2147483647);
  for (let i = 255; i > 0; i--) { const j = Math.floor(rnd() * (i + 1)); [base[i], base[j]] = [base[j], base[i]]; }
  for (let i = 0; i < 512; i++) p[i] = base[i & 255];
  const fade = (t) => t * t * t * (t * (t * 6 - 15) + 10);
  const lerp = (a, b, t) => a + t * (b - a);
  const grad = (h, x, y, z) => {
    const u = (h & 15) < 8 ? x : y, v = (h & 15) < 4 ? y : (h & 15) === 12 || (h & 15) === 14 ? x : z;
    return ((h & 1) ? -u : u) + ((h & 2) ? -v : v);
  };
  return (x, y, z) => {
    const X = Math.floor(x) & 255, Y = Math.floor(y) & 255, Z = Math.floor(z) & 255;
    x -= Math.floor(x); y -= Math.floor(y); z -= Math.floor(z);
    const u = fade(x), v = fade(y), w = fade(z);
    const A = p[X] + Y, AA = p[A] + Z, AB = p[A + 1] + Z, B = p[X + 1] + Y, BA = p[B] + Z, BB = p[B + 1] + Z;
    return lerp(lerp(lerp(grad(p[AA], x, y, z), grad(p[BA], x - 1, y, z), u), lerp(grad(p[AB], x, y - 1, z), grad(p[BB], x - 1, y - 1, z), u), v),
      lerp(lerp(grad(p[AA + 1], x, y, z - 1), grad(p[BA + 1], x - 1, y, z - 1), u), lerp(grad(p[AB + 1], x, y - 1, z - 1), grad(p[BB + 1], x - 1, y - 1, z - 1), u), v), w);
  };
}

/**
 * Build the scene into `canvas`, with HTML labels in `labels`.
 * @param {{ canvas: HTMLCanvasElement, labels: HTMLElement, onLost: () => void, light: boolean, held: boolean, reduced: boolean }} opts
 */
export function build(opts) {
  const { canvas, onLost } = opts;
  let reduced = !!opts.reduced;
  const small = matchMedia("(max-width: 860px)").matches || (navigator.hardwareConcurrency || 8) <= 4;
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: false });
  // The palette is authored as display colours, drawn as-is (three's pre-r152 behaviour).
  renderer.outputColorSpace = THREE.LinearSRGBColorSpace;
  renderer.setPixelRatio(Math.min(1.5, devicePixelRatio || 1));
  let light = !!opts.light, held = !!opts.held;
  const sc = new THREE.Scene();
  const cam = new THREE.PerspectiveCamera(36, 1, 0.1, 200);
  const col = (h) => new THREE.Color(h);
  // Night glows add light; the day palette is darker ink on a pale ground, drawn with normal blending.
  const PAL = {
    dark: { ground: "#050507", memory: "#8b7cff", soft: "#c9bfff", fire: "#4ade80", prime: "#fbbf24", inhibit: "#fb7185", ink: "#f4f4f7", nerve: "#6f6a8f" },
    light: { ground: "#ececef", memory: "#5b4bd6", soft: "#6a5ae0", fire: "#16a34a", prime: "#d97706", inhibit: "#e11d48", ink: "#131320", nerve: "#aaa6c4" },
  };
  const C = {}; for (const k of Object.keys(PAL.dark)) C[k] = col(PAL.dark[k]);
  const keyOf = (c) => Object.keys(C).find((k) => C[k] === c);
  const glow = () => (light ? THREE.NormalBlending : THREE.AdditiveBlending);

  // Bloom is optional: if it cannot be set up, the scene still draws without it.
  function makeComposer() {
    try {
      const rt = new THREE.WebGLRenderTarget(512, 512, { type: THREE.HalfFloatType, samples: 4 });
      const c = new EffectComposer(renderer, rt);
      c.addPass(new RenderPass(sc, cam));
      c.addPass(new UnrealBloomPass(new THREE.Vector2(512, 512), 0.75, 0.5, 0.5));
      return c;
    } catch (e) { console.warn("bloom off", e); return null; }
  }
  const composer = small ? null : makeComposer();

  let seed = 11;
  const rnd = () => ((seed = (seed * 16807) % 2147483647) / 2147483647);
  const noise = makeNoise(7);
  const ridged = (x, y, z, f) => 1 - Math.abs(noise(x * f, y * f, z * f) * 1.6);

  /* --- tissue: a lit, translucent surface with a violet rim; cavities darken --- */
  const tissue = (base, rim, opacity, rimAmt) => new THREE.ShaderMaterial({
    uniforms: { uBase: { value: col(base) }, uRim: { value: col(rim) }, uOp: { value: opacity }, uRimAmt: { value: rimAmt }, uFade: { value: 1 } },
    vertexShader: `
      attribute float cav; varying vec3 vN; varying vec3 vV; varying float vCav;
      void main(){ vec4 mv = modelViewMatrix * vec4(position, 1.0); vN = normalize(normalMatrix * normal); vV = normalize(-mv.xyz); vCav = cav; gl_Position = projectionMatrix * mv; }`,
    fragmentShader: `
      uniform vec3 uBase; uniform vec3 uRim; uniform float uOp; uniform float uRimAmt; uniform float uFade;
      varying vec3 vN; varying vec3 vV; varying float vCav;
      void main(){
        vec3 n = normalize(vN);
        vec3 L = normalize(vec3(-0.35, 0.75, 0.55));
        float diff = max(dot(n, L), 0.0);
        float wrap = max(dot(n, L) * 0.5 + 0.5, 0.0);
        float fres = pow(1.0 - max(dot(n, vV), 0.0), 2.4);
        float ao = mix(0.28, 1.0, smoothstep(-0.9, 0.7, vCav));
        vec3 H = normalize(L + vV);
        float spec = pow(max(dot(n, H), 0.0), 36.0) * 0.35;
        vec3 c = uBase * (0.18 + 0.75 * wrap + 0.35 * diff) * ao + uRim * fres * uRimAmt * (0.4 + 0.6 * ao) + vec3(spec) * ao + uRim * pow(1.0 - wrap, 3.0) * 0.25;
        float a = uOp * (0.62 + 0.38 * fres) * uFade;
        gl_FragColor = vec4(c, a);
      }`,
    transparent: true, depthWrite: true,
  });
  const cortexMat = tissue("#7a70a8", "#8b7cff", 0.9, 0.9);
  const cerebMat = tissue("#6a6096", "#8b7cff", 0.9, 0.8);
  const stemMat = tissue("#6a6392", "#a79cff", 0.82, 0.7); stemMat.depthWrite = false;
  const boneMat = tissue("#8e8aa0", "#c9bfff", 0.22, 0.6); boneMat.depthWrite = false;
  const discMat = tissue("#7c6fd6", "#c9bfff", 0.3, 0.8); discMat.depthWrite = false;
  // [material, night base, rim, opacity, rim amount, day base, rim, opacity, rim amount]
  const TISSUE = [
    [cortexMat, "#7a70a8", "#8b7cff", 0.9, 0.9, "#b3aad6", "#5b4bd6", 0.92, 0.55],
    [cerebMat, "#6a6096", "#8b7cff", 0.9, 0.8, "#a39ac9", "#5b4bd6", 0.92, 0.5],
    [stemMat, "#6a6392", "#a79cff", 0.82, 0.7, "#a9a1d0", "#5b4bd6", 0.85, 0.45],
    [boneMat, "#8e8aa0", "#c9bfff", 0.22, 0.6, "#8f8ba3", "#4a4d5c", 0.3, 0.45],
    [discMat, "#7c6fd6", "#c9bfff", 0.3, 0.8, "#8a7ee6", "#4a3bc4", 0.35, 0.5],
  ];

  const BY = 0.62; // the brain's centre height

  /* --- the hemispheres: a sphere shaped into lobes, then folded --- */
  function hemisphere(side) {
    const g = new THREE.SphereGeometry(1, small ? 150 : 240, small ? 110 : 170);
    const pos = g.attributes.position, cav = new Float32Array(pos.count), d = new THREE.Vector3(), q = new THREE.Vector3();
    for (let i = 0; i < pos.count; i++) {
      d.fromBufferAttribute(pos, i);
      const lat = d.x * side; // > 0: the outer face
      let sx = lat < 0 ? 0.33 : 0.8, sy = d.y > 0 ? 0.9 : 0.5, sz = 1.4;
      sx *= 1 - 0.2 * Math.max(0, -d.z) - 0.12 * Math.max(0, d.z - 0.45);
      sy *= 1 - 0.14 * Math.max(0, -d.z - 0.3);
      q.set(side * 0.36 + d.x * sx, BY + d.y * sy, d.z * sz);
      // The temporal lobe: low, lateral, forward of centre.
      const tl = Math.max(0, lat - 0.1) * Math.max(0, 0.2 - d.y) * Math.exp(-((d.z - 0.25) ** 2) / 0.28);
      q.y -= tl * 0.55; q.x += side * tl * 0.12;
      // The frontal base tucks up; the occipital pole points back.
      if (d.y < -0.2 && d.z > 0.3) q.y += 0.08 * (d.z - 0.3);
      // Folds: two octaves of ridged noise, gentler on the medial wall and underside.
      // Sulci are the zero-set of domain-warped noise: narrow grooves, broad rounded gyri.
      const wx = q.x + 0.35 * noise(q.x * 1.3, q.y * 1.3 + 9, q.z * 1.3), wy = q.y + 0.35 * noise(q.x * 1.3 + 4, q.y * 1.3, q.z * 1.3), wz = q.z + 0.35 * noise(q.x * 1.3, q.y * 1.3, q.z * 1.3 + 7);
      const n1 = noise(wx * 2.7 + 3, wy * 2.7, wz * 2.7), n2 = noise(wx * 5.6, wy * 5.6 + 5, wz * 5.6);
      const s1 = Math.exp(-((n1 / 0.075) ** 2)), s2 = Math.exp(-((n2 / 0.06) ** 2));
      const soft = lat < -0.35 ? 0.45 : d.y < -0.55 ? 0.6 : 1;
      // The lateral (Sylvian) fissure, above the temporal lobe; the central sulcus over the top.
      const syl = lat > 0.2 && d.z > -0.45 ? Math.exp(-(((d.y - (-0.05 + 0.3 * d.z)) / 0.06) ** 2)) * Math.min(1, (d.z + 0.45) / 0.25) : 0;
      const cen = d.y > 0.05 && lat > -0.2 ? Math.exp(-(((d.z - (0.08 - 0.22 * d.y)) / 0.05) ** 2)) : 0;
      const groove = (s1 * 0.8 + s2 * 0.35) * soft * (1 - syl) * (1 - cen);
      q.addScaledVector(d, -groove * 0.075 - syl * 0.16 - cen * 0.08 + 0.01);
      pos.setXYZ(i, q.x, q.y, q.z);
      cav[i] = 0.6 - groove * 2.6 - syl * 3 - cen * 2.2;
    }
    g.setAttribute("cav", new THREE.BufferAttribute(cav, 1));
    g.computeVertexNormals();
    g.computeBoundingBox(); const ctr = g.boundingBox.getCenter(new THREE.Vector3()); g.translate(-ctr.x, -ctr.y, -ctr.z);
    const m = new THREE.Mesh(g, cortexMat); m.position.copy(ctr); m.renderOrder = 3; sc.add(m);
    return m;
  }
  const hemis = [hemisphere(-1), hemisphere(1)];

  /* --- the cerebellum: finely foliated, tucked under the occipital lobes --- */
  const cereb = (() => {
    const g = new THREE.SphereGeometry(1, small ? 90 : 130, small ? 60 : 90);
    const pos = g.attributes.position, cav = new Float32Array(pos.count), d = new THREE.Vector3();
    for (let i = 0; i < pos.count; i++) {
      d.fromBufferAttribute(pos, i);
      const CY = BY - 0.58, CZ = -0.84;
      const x = d.x * 0.74 * (1 - 0.1 * Math.abs(d.y)), y = d.y * (d.y > 0 ? 0.24 : 0.34) + CY, z = d.z * 0.46 + CZ;
      const vermis = Math.exp(-((d.x / 0.16) ** 2)) * (0.1 * Math.max(0, -d.z) + 0.06 * Math.max(0, -d.y));
      const phi = Math.atan2(d.y, -d.z);
      const folia = Math.sin(phi * 30 + noise(d.x * 2.5, d.y * 2.5, d.z * 2.5) * 1.6);
      const hfis = Math.exp(-(((phi - 0.1) / 0.05) ** 2)) * (1 - Math.exp(-((d.x / 0.2) ** 2)));
      const k = folia * 0.5;
      const rr = 1 + k * 0.018 - vermis - hfis * 0.05;
      pos.setXYZ(i, x * rr, CY + (y - CY) * rr, CZ + (z - CZ) * rr);
      cav[i] = k * 1.4 - vermis * 12 - hfis * 3;
    }
    g.setAttribute("cav", new THREE.BufferAttribute(cav, 1));
    g.computeVertexNormals();
    g.computeBoundingBox(); const ctr = g.boundingBox.getCenter(new THREE.Vector3()); g.translate(-ctr.x, -ctr.y, -ctr.z);
    const m = new THREE.Mesh(g, cerebMat); m.position.copy(ctr); m.renderOrder = 3; sc.add(m);
    return m;
  })();

  /* --- brainstem and spinal cord: one tapering tube --- */
  const cordAt = (y) => {
    if (y > -1.15) { const t = (0.2 - y) / 1.35; return new THREE.Vector3(0, y, -0.34 - 0.24 * t); }
    return new THREE.Vector3(0, y, -0.58 - 0.035 * Math.sin((y + 1.15) * 0.9) + (y + 1.15) * 0.012);
  };
  const cordR = (y) => (y > -0.35 ? 0.26 - 0.06 * ((0.2 - y) / 0.55) : y > -1.15 ? 0.2 - 0.07 * ((-0.35 - y) / 0.8) + 0.035 * Math.exp(-(((y + 0.55) / 0.18) ** 2)) : 0.125 - 0.02 * Math.min(1, (-1.15 - y) / 3.2));
  const CORD_BOTTOM = -4.55;
  const stem = (() => {
    const segs = 220, radial = 28, verts = [], cavs = [], idx = [];
    for (let i = 0; i <= segs; i++) {
      const y = 0.25 + (CORD_BOTTOM - 0.25) * (i / segs), c = cordAt(y), r = cordR(y);
      for (let j = 0; j <= radial; j++) {
        const a = (j / radial) * Math.PI * 2;
        const fiss = Math.exp(-(((Math.cos(a) - 0) ** 2 + (Math.sin(a) - 1) ** 2) / 0.02)) * 0.012; // the anterior fissure
        const pons = 0.11 * Math.exp(-(((y + 0.12) / 0.15) ** 2)) * Math.max(0, Math.sin(a)) ** 1.5;
        verts.push(c.x + Math.cos(a) * (r - fiss + pons * 0.6), y, c.z + Math.sin(a) * (r - fiss + pons) * 0.92);
        cavs.push(-fiss * 60 + noise(a * 2, y * 6, 0) * 0.3);
      }
    }
    for (let i = 0; i < segs; i++) for (let j = 0; j < radial; j++) {
      const a = i * (radial + 1) + j, b = a + radial + 1;
      idx.push(a, a + 1, b, b, a + 1, b + 1);
    }
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.Float32BufferAttribute(verts, 3));
    g.setAttribute("cav", new THREE.Float32BufferAttribute(cavs, 1));
    g.setIndex(idx); g.computeVertexNormals();
    g.computeBoundingBox(); const ctr = g.boundingBox.getCenter(new THREE.Vector3()); g.translate(-ctr.x, -ctr.y, -ctr.z);
    const m = new THREE.Mesh(g, stemMat); m.position.copy(ctr); m.renderOrder = 3; sc.add(m);
    return m;
  })();

  /* --- the vertebral column: bodies in front, arches and spines behind --- */
  const VERT = [];
  const spine = new THREE.Group(); sc.add(spine);
  const bodyGeo = (s) => new THREE.LatheGeometry([[0, -0.1], [0.17, -0.1], [0.185, -0.085], [0.155, 0], [0.185, 0.085], [0.17, 0.1], [0, 0.1]].map(([x, y]) => new THREE.Vector2(x * s, y)), 28);
  for (let k = 0; k < 12; k++) {
    const y = -0.62 - k * 0.32, c = cordAt(y), s = 0.85 + k * 0.035;
    const addBone = (geo, x, yy, z, rx = 0, ry = 0, rz = 0) => {
      const cav = new Float32Array(geo.attributes.position.count).fill(0.3);
      geo.setAttribute("cav", new THREE.BufferAttribute(cav, 1));
      const m = new THREE.Mesh(geo, boneMat); m.position.set(x, yy, z); m.rotation.set(rx, ry, rz); m.renderOrder = 4; spine.add(m);
    };
    addBone(bodyGeo(s), c.x, y, c.z + 0.36 * s);
    if (k < 11) { const dg = new THREE.CylinderGeometry(0.16 * s, 0.16 * s, 0.07, 28); dg.setAttribute("cav", new THREE.BufferAttribute(new Float32Array(dg.attributes.position.count).fill(0.5), 1)); const dm = new THREE.Mesh(dg, discMat); dm.position.set(c.x, y - 0.16, c.z + 0.36 * s); dm.renderOrder = 4; spine.add(dm); }
    addBone(new THREE.TorusGeometry(0.24 * s, 0.045, 10, 30, Math.PI * 1.25), c.x, y, c.z + 0.12, Math.PI / 2, 0, Math.PI * 0.125 + Math.PI);
    addBone(new THREE.ConeGeometry(0.055, 0.34 * s, 12), c.x, y - 0.1, c.z - 0.38 * s, -Math.PI / 2 - 0.5, 0, 0);
    for (const sd of [-1, 1]) addBone(new THREE.ConeGeometry(0.045, 0.3 * s, 10), c.x + sd * 0.32 * s, y, c.z - 0.06, 0, 0, sd * Math.PI / 2);
    VERT.push(y);
  }

  /* --- neurons: faint dust inside the brain --- */
  const dust = (() => {
    const p = [];
    for (let i = 0; i < (small ? 1800 : 3200); i++) {
      const side = rnd() < 0.5 ? -1 : 1, u = rnd() * 2 - 1, th = rnd() * 6.283, rr = Math.cbrt(rnd()) * 0.8, s2 = Math.sqrt(1 - u * u);
      p.push(side * (0.4 + Math.abs(s2 * Math.cos(th)) * rr * 0.55), BY + u * rr * 0.7, s2 * Math.sin(th) * rr * 1.25);
    }
    const g = new THREE.BufferGeometry(); g.setAttribute("position", new THREE.Float32BufferAttribute(p, 3));
    const m = new THREE.Points(g, new THREE.PointsMaterial({ color: C.soft, size: 0.012, transparent: true, opacity: 0.35, blending: THREE.AdditiveBlending, depthWrite: false }));
    m.material.userData = { key: "soft", glow: true };
    m.renderOrder = 1; sc.add(m); return m;
  })();

  /* --- pathways --- */
  const groups = {};
  const glowTex = (() => { const c = document.createElement("canvas"); c.width = c.height = 64; const g = c.getContext("2d"); const gr = g.createRadialGradient(32, 32, 0, 32, 32, 32); gr.addColorStop(0, "rgba(255,255,255,1)"); gr.addColorStop(0.3, "rgba(255,255,255,.45)"); gr.addColorStop(1, "rgba(255,255,255,0)"); g.fillStyle = gr; g.fillRect(0, 0, 64, 64); return new THREE.CanvasTexture(c); })();
  function path(name, pts, color, opts = {}) {
    const c = new THREE.CatmullRomCurve3(pts, false, "centripetal");
    const r = opts.r ?? 0.012;
    const g = new THREE.TubeGeometry(c, opts.seg ?? 140, r, 6, false);
    const m = new THREE.MeshBasicMaterial({ color, transparent: true, opacity: 0, blending: THREE.AdditiveBlending, depthWrite: false });
    m.userData = { key: keyOf(color), glow: true };
    const mesh = new THREE.Mesh(g, m); mesh.renderOrder = opts.over ? 6 : 1; sc.add(mesh);
    const total = g.index ? g.index.count : g.attributes.position.count;
    const rec = { curve: c, mesh, mat: m, total, grow: opts.grow ?? 1, growTo: opts.grow ?? 1, color };
    (groups[name] = groups[name] || []).push(rec);
    return rec;
  }
  const nodes = {};
  function node(name, p, color, r = 0.05, over = false) {
    const m = new THREE.Mesh(new THREE.SphereGeometry(r, 16, 12), new THREE.MeshBasicMaterial({ color, transparent: true, opacity: 0, depthWrite: false }));
    m.position.copy(p); m.renderOrder = over ? 6 : 1; sc.add(m); m.material.userData = { key: keyOf(color) };
    const s = new THREE.Sprite(new THREE.SpriteMaterial({ map: glowTex, color, transparent: true, opacity: 0, blending: THREE.AdditiveBlending, depthWrite: false }));
    s.material.userData = { key: keyOf(color), glow: true };
    s.position.copy(p); s.scale.setScalar(r * 7); s.renderOrder = over ? 6 : 1; sc.add(s);
    (nodes[name] = nodes[name] || []).push({ m, s });
    return p;
  }

  // Memory: named regions inside the brain, joined by white-matter tracts.
  const V = (x, y, z) => new THREE.Vector3(x, y, z);
  const MEM = [
    ["CLAUDE.md", V(-0.5, 1.02, 0.78)],
    ["system prompt", V(0.48, 1.08, 0.62)],
    ["skill: deploy · description", V(-0.72, 0.62, -0.3)],
    ["skill: review-pr · description", V(0.74, 0.68, -0.18)],
    ["tools: github (mcp)", V(-0.72, 0.28, 0.52)],
    ["tools: postgres (mcp)", V(0.72, 0.26, 0.42)],
    ["AGENTS.md", V(0.4, 0.92, -0.88)],
    ["the conversation so far", V(-0.36, 0.86, -0.98)],
  ];

  MEM.forEach(([, p]) => node("memory", p, C.soft, 0.04));
  const CENTER = V(0, 0.52, -0.05);
  [[0, 1], [0, 2], [0, 4], [0, 7], [1, 3], [1, 6], [2, 7], [3, 5], [4, 5], [6, 7], [3, 6], [2, 4], [1, 5]].forEach(([a, b]) => {
    const A = MEM[a][1], B = MEM[b][1];
    const mid = A.clone().add(B).multiplyScalar(0.5).lerp(CENTER, 0.45);
    path("memory", [A, mid, B], C.memory, { r: 0.009 });
  });
  // The thalamus: where the cord's long way meets memory.
  node("memory", CENTER.clone(), C.soft, 0.035);

  // Spinal nerves: a pair between every two vertebrae.
  const nerve = (y, side) => {
    const c = cordAt(y), r = cordR(y);
    return [V(c.x + side * r * 0.9, y, c.z), V(c.x + side * 0.38, y - 0.03, c.z + 0.04), V(c.x + side * 0.72, y - 0.16, c.z + 0.18), V(c.x + side * 1.08, y - 0.34, c.z + 0.34)];
  };
  const LEVELS = VERT.map((y, k) => (k < VERT.length - 1 ? (y + VERT[k + 1]) / 2 : y - 0.2));
  LEVELS.forEach((y) => { for (const s of [-1, 1]) path("nerves", nerve(y, s), C.nerve, { r: 0.011, seg: 40 }); });

  // An arc at a level: stimulus in on the left, through the cord, response out on the right,
  // and its tract running down the cord from the brainstem to that level.
  const arcs = {};
  let lane = 0;
  function arc(name, level, color, grow) {
    const y = LEVELS[level], c = cordAt(y);
    const left = nerve(y, -1).reverse(), right = nerve(y, 1);
    const pts = [...left, V(c.x, y + 0.01, c.z + 0.02), ...right];
    const rec = path(name, pts, color, { r: 0.018, grow, over: true, seg: 120 });
    const tract = [];
    const ang = (name === "instinct" ? 2.2 : 0.95) - (lane++ % 4) * 0.16 * (name === "instinct" ? -1 : 1);
    for (let yy = 0.05; yy >= y + 0.06; yy -= 0.06) { const cc = cordAt(yy), rr = cordR(yy) * 1.06; tract.push(V(cc.x + Math.cos(ang) * rr, yy, cc.z + Math.sin(ang) * rr)); }
    tract.push(V(c.x, y, c.z + 0.02));
    const tr = path(name, tract, color, { r: 0.01, grow, over: true, seg: 160 });
    const S = left[0], R = right[right.length - 1];
    node(name, S, color, 0.05, true); node(name, R, color, 0.05, true);
    node(name, left[2], color, 0.028, true); // the dorsal-root ganglion: where the input enters
    return { rec, tract: tr, S, R, y };
  }
  const INST = [arc("instinct", 0, C.prime), arc("instinct", 1, C.prime), arc("instinct", 2, C.prime)]; lane = 0;
  node("instinct", cordAt(-0.2).add(V(-0.12, 0, 0.1)), C.prime, 0.045, true); // where the instinct tracts begin
  const RFX = [arc("reflex", 3, C.fire), arc("reflex", 4, C.fire), arc("reflex", 5, C.inhibit), arc("forming", 6, C.fire, 0)];
  // The inhibition's block on its response side.
  const blockAt = RFX[2].rec.curve.getPoint(0.74);
  const block = new THREE.Mesh(new THREE.BoxGeometry(0.02, 0.14, 0.14), new THREE.MeshBasicMaterial({ color: C.inhibit, transparent: true, opacity: 0, depthWrite: false }));
  block.material.userData = { key: "inhibit" };
  block.position.copy(blockAt); block.lookAt(RFX[2].rec.curve.getPoint(0.78)); block.renderOrder = 6; sc.add(block);

  // The long way: from a reflex's stimulus up the cord into memory and back down.
  const up = [];
  for (let yy = RFX[0].y; yy <= 0.1; yy += 0.25) { const cc = cordAt(yy); up.push(V(cc.x - 0.03, yy, cc.z + 0.05)); }
  const down = up.slice().reverse().map((p) => V(p.x + 0.06, p.y, p.z));
  const recall = path("recall", [RFX[0].S, ...nerve(RFX[0].y, -1).reverse().slice(1), ...up, CENTER, MEM[4][1], MEM[0][1], MEM[1][1], MEM[5][1], CENTER, ...down, ...nerve(RFX[0].y, 1)], C.soft, { r: 0.008, seg: 400, over: true });

  // The agent loop around it all.
  const ringPts = [];
  for (let i = 0; i < 96; i++) { const a = (i / 96) * Math.PI * 2; ringPts.push(V(Math.cos(a) * 3.8, -1.2 + Math.sin(a * 2) * 0.1, Math.sin(a) * 3.8)); }
  const ringCurve = new THREE.CatmullRomCurve3(ringPts, true);
  const ring = path("agent", ringCurve.getPoints(200), C.soft, { r: 0.01, seg: 300 });
  ring.curve = ringCurve;
  const STATIONS = [["prompt", "you ask", 0.58], ["context + inference", "the model thinks", 0.83], ["tool call", "terminal · git · browser", 0.08], ["hook", "Adage runs here", 0.33]];

  /* --- labels --- */
  const layer = opts.labels;
  const labels = [];
  const label = (html, anchor, tabs, cls = "chip", dx = 0, dy = -28, center = true) => {
    const el = document.createElement("div"); el.className = "lbl " + cls; el.innerHTML = html; layer.appendChild(el);
    labels.push({ el, anchor, tabs, dx, dy, center });
  };
  MEM.forEach(([t, p]) => label(`<i class="m">●</i> ${t}`, p, ["memory"]));
  const instNames = [["pytest fails", "Makefile", "make test"], ["npm test fails", "pnpm-lock.yaml", "pnpm test"], ["edit a generated file", "@generated marker", "edit its inputs, run make gen"]];
  INST.forEach((a, i) => {
    label(`<small>stimulus · bound from ${instNames[i][1]}</small>${instNames[i][0]}`, a.S, ["instincts"], "chip in", -10, -30);
    label(`<small>prime</small><i class="p">●</i> ${instNames[i][2]}`, a.R, ["instincts"], "chip", 10, -30);
  });
  const rfxNames = [["edit schema/*.json", "make gen", "fire", "f"], ["prompt names W-1042", "wi-lookup W-1042", "fire", "f"], ["pytest: no module", "make test instead", "inhibit", "x"], ["edit registry/*.csv", "npm run codegen", "forming", "f"]];
  RFX.forEach((a, i) => {
    const [s, r, k, c] = rfxNames[i];
    label(`<small>stimulus</small>${s}`, a.S, ["reflexes"], "chip in", 0, -30);
    label(`<small>${k}</small><i class="${c}">●</i> ${r}`, a.R, ["reflexes"], "chip", 0, -30);
  });
  label("context · recalled by inference", V(0, 1.6, 0.3), ["overview"], "", 0, -10);
  label("instincts · built in", cordAt(LEVELS[1]).add(V(0.45, 0.1, 0)), ["overview"], "", 70, -8, false);
  label("reflexes · learned", cordAt(LEVELS[4]).add(V(0.45, 0.1, 0)), ["overview"], "", 70, -8, false);
  label("the long way: up to memory and back", CENTER, ["reflexes"], "in", 0, -36);
  STATIONS.forEach(([t, s, f]) => label(`<small>${s}</small>${t}`, ringCurve.getPoint(f), ["agent"], "chip"));

  /* --- pulses: light travels --- */
  const pulses = [];
  function pulse(rec, ms, color, until = 1) {
    const trail = [];
    for (let i = 0; i < 8; i++) {
      const s = new THREE.Sprite(new THREE.SpriteMaterial({ map: glowTex, color, transparent: true, blending: glow(), depthWrite: false }));
      s.scale.setScalar(0.2 - i * 0.02); s.renderOrder = 7; sc.add(s); trail.push(s);
    }
    pulses.push({ rec, t0: performance.now(), ms, trail, until });
  }

  /* --- per-tab state --- */
  const CAM = {
    overview: { t: [0, -1.2, -0.2], yaw: 0.95, pitch: 0.1, dist: 14 },
    memory: { t: [0, 0.45, -0.15], yaw: 0.8, pitch: 0.24, dist: 7.4 },
    instincts: { t: [0, -0.8, -0.4], yaw: 0.5, pitch: 0.12, dist: 8.8 },
    reflexes: { t: [0, -1.6, -0.4], yaw: 0.4, pitch: 0.08, dist: 12 },
    agent: { t: [0, -0.9, 0], yaw: 0.6, pitch: 0.34, dist: 17.5 },
  };
  const LOOK = {
    overview: { cortex: 0.92, memory: 0.55, instinct: 0.75, reflex: 0.75, forming: 0.5, recall: 0, agent: 0, nerves: 0.28, bone: 1 },
    memory: { cortex: 0.62, memory: 1, instinct: 0.1, reflex: 0.08, forming: 0.05, recall: 0, agent: 0, nerves: 0.08, bone: 0.4 },
    instincts: { cortex: 0.85, memory: 0.1, instinct: 1, reflex: 0.12, forming: 0.05, recall: 0, agent: 0, nerves: 0.3, bone: 0.85 },
    reflexes: { cortex: 0.5, memory: 0.25, instinct: 0.2, reflex: 1, forming: 1, recall: 0.4, agent: 0, nerves: 0.3, bone: 0.85 },
    agent: { cortex: 0.9, memory: 0.2, instinct: 0.5, reflex: 0.55, forming: 0.35, recall: 0, agent: 0.8, nerves: 0.18, bone: 0.7 },
  };
  let cur = "overview", camFrom = { ...CAM.overview }, camTo = CAM.overview, camNow = { ...CAM.overview, t: [...CAM.overview.t] };
  let lookFrom = { ...LOOK.overview }, lookTo = LOOK.overview, lookNow = { ...LOOK.overview };
  let t0 = 0, asm = 1, asmT0 = 0, lastPulse = 0;
  const user = { yaw: 0, pitch: 0 };

  function go(id) {
    cur = id; t0 = performance.now();
    camFrom = { ...camNow, t: [...camNow.t] }; camTo = CAM[id];
    lookFrom = { ...lookNow }; lookTo = LOOK[id];
    for (const r of groups.forming) { if (id === "reflexes") r.grow = 0; r.growTo = 1; }
    lastPulse = 0;
    labels.forEach((l) => (l.on = l.tabs.includes(id)));
    if (typeof invalidate === "function") invalidate();
  }
  function intro() { if (reduced) return; asm = 0; asmT0 = performance.now(); }
  function theme(l) {
    light = l;
    const P = PAL[light ? "light" : "dark"];
    for (const k of Object.keys(C)) C[k].set(P[k]);
    renderer.setClearColor(C.ground, 1);
    for (const [m, ...v] of TISSUE) {
      const [base, rim, op, amt] = light ? v.slice(4) : v.slice(0, 4);
      m.uniforms.uBase.value.set(base); m.uniforms.uRim.value.set(rim); m.uniforms.uOp.value = op; m.uniforms.uRimAmt.value = amt;
    }
    sc.traverse((o) => {
      const m = o.material; if (!m || !m.userData) return;
      if (m.userData.key) m.color.copy(C[m.userData.key]);
      if (m.userData.glow) { m.blending = glow(); m.needsUpdate = true; }
    });
    for (const p of pulses) for (const t of p.trail) { t.material.blending = glow(); t.material.needsUpdate = true; }
    if (typeof invalidate === "function") invalidate();
  }
  // While the teaser plays the scene waits; when it ends, the body assembles.
  function hold(h) { held = h; if (!h) { intro(); invalidate(); } }

  let drag = null;
  canvas.addEventListener("pointerdown", (e) => { drag = { x: e.clientX, y: e.clientY, yaw: user.yaw, pitch: user.pitch, touch: e.pointerType === "touch" }; canvas.setPointerCapture(e.pointerId); });
  canvas.addEventListener("pointermove", (e) => { if (!drag) return; invalidate(); user.yaw = drag.yaw - (e.clientX - drag.x) * 0.005; if (!drag.touch) user.pitch = Math.max(-0.4, Math.min(0.5, drag.pitch + (e.clientY - drag.y) * 0.003)); });
  const end = () => (drag = null);
  canvas.addEventListener("pointerup", end); canvas.addEventListener("pointercancel", end);

  let visible = true;
  new IntersectionObserver((es) => { visible = es[0].isIntersecting; if (visible) kick(); }).observe(canvas);
  document.addEventListener("visibilitychange", () => { if (!document.hidden) kick(); });

  const ease = (x) => 1 - Math.pow(1 - x, 4);
  const lerp = (a, b, k) => a + (b - a) * k;
  const tmp = new THREE.Vector3();
  let raf = 0, lastNow = 0, dirty = true, lost = false, lostT = 0;
  const mqNarrow = matchMedia("(max-width: 860px)");
  function kick() { if (!raf) raf = requestAnimationFrame(frame); }
  const invalidate = () => { dirty = true; kick(); };
  new ResizeObserver(invalidate).observe(canvas);
  canvas.addEventListener("webglcontextlost", (e) => { e.preventDefault(); lost = true; cancelAnimationFrame(raf); raf = 0; lostT = setTimeout(() => onLost(), 3000); });
  canvas.addEventListener("webglcontextrestored", () => { lost = false; clearTimeout(lostT); invalidate(); });

  function frame(now) {
    raf = 0;
    if (held || !visible || document.hidden || lost) return;
    const dt = Math.min(100, now - (lastNow || now)); lastNow = now;
    const wide = !mqNarrow.matches;
    const pr = Math.min(wide ? 1.25 : 1.5, devicePixelRatio || 1);
    if (pr !== renderer.getPixelRatio()) { renderer.setPixelRatio(pr); if (composer) composer.setPixelRatio(pr); }
    const w = canvas.clientWidth, h = canvas.clientHeight;
    if (canvas.width !== Math.floor(w * renderer.getPixelRatio()) || canvas.height !== Math.floor(h * renderer.getPixelRatio())) {
      renderer.setSize(w, h, false); if (composer) composer.setSize(w, h);
      cam.aspect = w / h; cam.updateProjectionMatrix();
    }
    const k = reduced ? 1 : ease(Math.min(1, (now - t0) / 1500));
    for (const f of ["yaw", "pitch", "dist"]) camNow[f] = lerp(camFrom[f], camTo[f], k);
    camNow.t = camFrom.t.map((v, i) => lerp(v, camTo.t[i], k));
    for (const key of Object.keys(lookTo)) lookNow[key] = lerp(lookFrom[key], lookTo[key], k);

    // The intro: the body fades up from the spine to the cortex, then the pathways light.
    if (asm < 1) asm = Math.min(1, (now - asmT0) / 3400);
    const a1 = Math.min(1, asm / 0.55), reveal = Math.max(0, (asm - 0.6) / 0.4);
    const fadeIn = (from) => Math.min(1, Math.max(0, (a1 - from) / 0.45));
    boneMat.uniforms.uFade.value = fadeIn(0) * lookNow.bone;
    stemMat.uniforms.uFade.value = fadeIn(0.15); discMat.uniforms.uFade.value = fadeIn(0) * lookNow.bone;
    cerebMat.uniforms.uFade.value = fadeIn(0.35);
    cortexMat.uniforms.uFade.value = fadeIn(0.5) * lookNow.cortex;
    hemis.forEach((m) => (m.visible = cortexMat.uniforms.uFade.value > 0.01));
    dust.material.opacity = 0.35 * reveal * (0.4 + lookNow.memory * 0.6);

    for (const [name, recs] of Object.entries(groups)) {
      const op = (lookNow[name] ?? 0) * reveal;
      for (const r of recs) {
        r.mat.opacity = op;
        if (r.grow < r.growTo) r.grow = Math.min(r.growTo, r.grow + (reduced ? 1 : dt / 3000));
        r.mesh.geometry.setDrawRange(0, Math.max(0, Math.floor(r.grow * r.total / 3) * 3));
        r.mesh.visible = op > 0.004 && r.grow > 0;
      }
      for (const n of nodes[name] || []) { n.m.material.opacity = Math.min(1, op * 1.2); n.s.material.opacity = op * (light ? 0.28 : 0.7); n.m.visible = n.s.visible = op > 0.004; }
    }
    block.material.opacity = (lookNow.reflex ?? 0) * reveal;

    if (!reduced && reveal >= 1 && now - lastPulse > 2800) {
      lastPulse = now;
      if (cur === "memory") for (let i = 0; i < 2; i++) pulse(groups.memory[Math.floor(rnd() * groups.memory.length)], 1500, C.soft);
      if (cur === "instincts") INST.forEach((a, i) => setTimeout(() => { pulse(a.tract, 700, C.prime); setTimeout(() => pulse(a.rec, 900, C.prime), 650); }, i * 320));
      if (cur === "reflexes") { pulse(RFX[0].rec, 850, C.fire); pulse(recall, 6200, C.soft); pulse(RFX[2].rec, 850, C.inhibit, 0.78); }
      if (cur === "agent") pulse(ring, 5200, C.soft);
      if (cur === "overview") { pulse(RFX[1].rec, 950, C.fire); pulse(INST[0].rec, 950, C.prime); pulse(groups.memory[2], 1500, C.soft); }
    }
    for (let i = pulses.length - 1; i >= 0; i--) {
      const p = pulses[i], u = (now - p.t0) / p.ms;
      if (u > 1.25) { p.trail.forEach((s) => { sc.remove(s); s.material.dispose(); }); pulses.splice(i, 1); continue; }
      p.trail.forEach((s, j) => {
        const v = Math.min(p.until, Math.max(0, u - j * 0.02));
        s.position.copy(p.rec.curve.getPoint(v));
        s.material.opacity = (u > 1 ? (1.25 - u) * 4 : 1) * (1 - j / p.trail.length) * (light ? 0.55 : 0.9);
      });
    }

    const drift = reduced ? 0 : Math.sin(now / 9000) * 0.07;
    const yaw = camNow.yaw + user.yaw + drift, pitch = camNow.pitch + user.pitch;
    const dist = camNow.dist * (wide ? Math.max(1, 1320 / w) : 0.8);
    cam.position.set(camNow.t[0] + dist * Math.cos(pitch) * Math.sin(yaw), camNow.t[1] + dist * Math.sin(pitch), camNow.t[2] + dist * Math.cos(pitch) * Math.cos(yaw));
    cam.lookAt(camNow.t[0], camNow.t[1], camNow.t[2]);
    if (wide) cam.setViewOffset(w, h, -290, 0, w, h); else cam.clearViewOffset();
    // Bloom lifts light off a dark ground; on the day ground it only washes out.
    if (composer && !light) composer.render(); else renderer.render(sc, cam);

    const placed = [], minX = wide ? 540 : 8;
    for (const l of labels) {
      const drop = !wide && l.el.classList.contains("in");
      if (!(l.on && reveal >= 1) || drop) { l.el.style.opacity = "0"; continue; }
      tmp.copy(l.anchor).project(cam);
      if (tmp.z > 1) { l.el.style.opacity = "0"; continue; }
      const lw = l.el.offsetWidth, lh = l.el.offsetHeight;
      let x = (tmp.x * 0.5 + 0.5) * w + l.dx - (l.center ? lw / 2 : 0), y = (-tmp.y * 0.5 + 0.5) * h + l.dy;
      x = Math.max(minX, Math.min(w - lw - 12, x)); y = Math.max(wide ? 72 : 8, y);
      for (const p of placed) if (x < p.r + 4 && x + lw > p.l - 4 && y < p.b + 4 && y + lh > p.t - 4) y = p.b + 4;
      placed.push({ l: x, r: x + lw, t: y, b: y + lh });
      l.el.style.opacity = y + lh > h - 4 ? "0" : "1";
      l.el.style.transform = `translate(${Math.round(x)}px, ${Math.round(y)}px)`;
    }
    // Keep drawing while anything moves; otherwise wait for a change.
    const busy = !reduced || k < 1 || asm < 1 || pulses.length || drag || Object.values(groups).some((rs) => rs.some((r) => r.grow < r.growTo));
    if (busy || dirty) kick();
    dirty = false;
  }

  theme(light);
  go("overview");
  intro();
  kick();
  return { go, intro, theme, hold, motion: (r) => { reduced = r; invalidate(); } };
}
