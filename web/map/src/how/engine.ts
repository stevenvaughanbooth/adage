// The "How it works" stage: a small three.js scene that chapters drive with declarative states.
//
// A chapter adds dots (spheres with a glow), links (line segments between dots or fixed points)
// and labels (HTML, projected every frame), then describes each step as a `SceneState`: where
// every dot is and how it looks, how visible each link group is, which labels show, and where the
// camera is. `to()` tweens from whatever is on screen to that state. Nothing loops: the stage
// renders while a tween, a drag or a resize is in progress, and then stops.

import {
  AdditiveBlending,
  BufferAttribute,
  BufferGeometry,
  CanvasTexture,
  Color,
  LineBasicMaterial,
  LineSegments,
  Mesh,
  MeshBasicMaterial,
  NormalBlending,
  PerspectiveCamera,
  Scene,
  SphereGeometry,
  Sprite,
  SpriteMaterial,
  Vector3,
  WebGLRenderer,
} from "three";

/** Colours a chapter can ask for, read from the map's tokens (night or day). */
export type Role =
  | "ink1"
  | "ink2"
  | "ink3"
  | "accent"
  | "soft"
  | "fire"
  | "prime"
  | "inhibit"
  | "cost"
  | "line";

export interface Palette {
  dark: boolean;
  role: Record<Role, Color>;
  /** A categorical colour, the same hue in both themes, lighter at night. */
  hue(deg: number): Color;
  css(role: Role): string;
}

export type V3 = [number, number, number];

export interface DotLook {
  pos: V3;
  /** A role, a categorical hue (resolved per theme), or a fixed colour. */
  color: Role | { hue: number } | Color;
  alpha?: number;
  scale?: number;
  glow?: number;
}

export interface CamSpec {
  target: V3;
  yaw: number;
  pitch: number;
  dist: number;
}

export interface SceneState {
  dots: Record<string, DotLook>;
  links?: Record<string, number>;
  labels?: string[];
  cam: CamSpec;
}

interface Dot {
  id: string;
  mesh: Mesh<SphereGeometry, MeshBasicMaterial>;
  glow: Sprite;
  size: number;
  cur: Look | null;
  from: Look | null;
  to: Look | null;
}

interface Look {
  pos: Vector3;
  color: Color;
  alpha: number;
  scale: number;
  glow: number;
}

type End = string | V3;

interface Links {
  pairs: [End, End][];
  geo: BufferGeometry;
  mat: LineBasicMaterial;
  obj: LineSegments;
  role: Role;
  op: number;
  from: number;
  to: number;
}

interface Label {
  id: string;
  el: HTMLDivElement;
  anchor: End;
  dx: number;
  dy: number;
  center: boolean;
  on: boolean;
}

const EASE = (t: number) => 1 - Math.pow(1 - t, 4);
const TWEEN_MS = 1100;

function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

function isDark(): boolean {
  const t = document.documentElement.getAttribute("data-theme");
  if (t === "night") return true;
  if (t === "day") return false;
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function readPalette(): Palette {
  const dark = isDark();
  const vars: Record<Role, string> = {
    ink1: "--color-ink-1",
    ink2: "--color-ink-2",
    ink3: "--color-ink-3",
    accent: "--color-accent-base",
    soft: "--color-accent-soft",
    fire: "--color-lamp-fire",
    prime: "--color-lamp-prime",
    inhibit: "--color-lamp-inhibit",
    cost: "--color-cost",
    line: "--color-ink-3",
  };
  const role = {} as Record<Role, Color>;
  for (const k of Object.keys(vars) as Role[]) {
    const v = cssVar(vars[k]);
    role[k] = new Color(v && v.startsWith("#") ? v : "#8b8e9c");
  }
  return {
    dark,
    role,
    hue: (deg) => new Color().setHSL(deg / 360, dark ? 0.55 : 0.5, dark ? 0.68 : 0.42),
    css: (r) => `#${role[r].getHexString()}`,
  };
}

function glowTexture(): CanvasTexture {
  const c = document.createElement("canvas");
  c.width = c.height = 64;
  const g = c.getContext("2d");
  if (g) {
    const gr = g.createRadialGradient(32, 32, 0, 32, 32, 32);
    gr.addColorStop(0, "rgba(255,255,255,1)");
    gr.addColorStop(0.25, "rgba(255,255,255,0.45)");
    gr.addColorStop(1, "rgba(255,255,255,0)");
    g.fillStyle = gr;
    g.fillRect(0, 0, 64, 64);
  }
  return new CanvasTexture(c);
}

/** Whether this browser can draw the stage at all. */
export function webglAvailable(): boolean {
  try {
    const c = document.createElement("canvas");
    return Boolean(c.getContext("webgl2") ?? c.getContext("webgl"));
  } catch {
    return false;
  }
}

export class Stage {
  readonly palette: () => Palette;
  private pal: Palette;
  private renderer: WebGLRenderer;
  private scene = new Scene();
  private camera = new PerspectiveCamera(40, 1, 0.1, 400);
  private tex = glowTexture();
  private dots = new Map<string, Dot>();
  private links = new Map<string, Links>();
  private labels = new Map<string, Label>();
  private camFrom: CamSpec = { target: [0, 0, 0], yaw: 0, pitch: 0.5, dist: 20 };
  private camTo: CamSpec = this.camFrom;
  private camCur: CamSpec = this.camFrom;
  private user = { yaw: 0, pitch: 0, zoom: 1 };
  private t0 = 0;
  private dur = 0;
  private raf = 0;
  private dragging: { x: number; y: number; yaw: number; pitch: number; touch: boolean } | null = null;
  private last: SceneState | null = null;
  private reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  private cleanup: (() => void)[] = [];
  private tmp = new Vector3();
  private onRetheme: (() => void) | null = null;
  private inset = 0;

  constructor(
    private canvas: HTMLCanvasElement,
    private layer: HTMLElement,
  ) {
    this.renderer = new WebGLRenderer({ canvas, antialias: true, alpha: true });
    this.renderer.setPixelRatio(Math.min(2, window.devicePixelRatio || 1));
    this.pal = readPalette();
    this.palette = () => this.pal;

    const listen = <K extends keyof HTMLElementEventMap>(el: HTMLElement, type: K, fn: (e: HTMLElementEventMap[K]) => void, opts?: AddEventListenerOptions) => {
      el.addEventListener(type, fn as EventListener, opts);
      this.cleanup.push(() => el.removeEventListener(type, fn as EventListener, opts));
    };
    listen(canvas, "pointerdown", (e) => {
      this.dragging = { x: e.clientX, y: e.clientY, yaw: this.user.yaw, pitch: this.user.pitch, touch: e.pointerType === "touch" };
      canvas.setPointerCapture(e.pointerId);
    });
    listen(canvas, "pointermove", (e) => {
      const d = this.dragging;
      if (!d) return;
      this.user.yaw = d.yaw - (e.clientX - d.x) * 0.006;
      // On touch, vertical drags scroll the page; only the yaw follows the finger.
      if (!d.touch) this.user.pitch = Math.max(-0.5, Math.min(0.7, d.pitch + (e.clientY - d.y) * 0.004));
      this.kick();
    });
    const end = () => (this.dragging = null);
    listen(canvas, "pointerup", end);
    listen(canvas, "pointercancel", end);
    listen(
      canvas,
      "wheel",
      (e) => {
        e.preventDefault();
        this.user.zoom = Math.max(0.55, Math.min(1.7, this.user.zoom * (1 + e.deltaY * 0.001)));
        this.kick();
      },
      { passive: false },
    );
    listen(canvas, "dblclick", () => this.resetView());

    const ro = new ResizeObserver(() => this.kick());
    ro.observe(canvas);
    this.cleanup.push(() => ro.disconnect());
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const retheme = () => this.retheme();
    mq.addEventListener("change", retheme);
    this.cleanup.push(() => mq.removeEventListener("change", retheme));
    const mo = new MutationObserver(retheme);
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    this.cleanup.push(() => mo.disconnect());
  }

  /** Called after a theme change, once the stage has recoloured (for 2D figures to redraw). */
  onTheme(fn: () => void) {
    this.onRetheme = fn;
  }

  /** Pixels on the left that the caption covers: the scene centres in the space right of it. */
  setInset(px: number) {
    if (Math.abs(px - this.inset) < 1) return;
    this.inset = px;
    this.kick();
  }

  resetView() {
    this.user = { yaw: 0, pitch: 0, zoom: 1 };
    this.kick();
  }

  // ------------------------------------------------------------------ building

  dot(id: string, size = 0.17): void {
    const mesh = new Mesh(new SphereGeometry(size, 20, 14), new MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0 }));
    const glow = new Sprite(
      new SpriteMaterial({ map: this.tex, transparent: true, depthWrite: false, blending: this.pal.dark ? AdditiveBlending : NormalBlending, opacity: 0 }),
    );
    mesh.visible = glow.visible = false;
    this.scene.add(mesh, glow);
    this.dots.set(id, { id, mesh, glow, size, cur: null, from: null, to: null });
  }

  /** A group of segments between dots (by id) or fixed points; its opacity is set per step. */
  link(name: string, pairs: [End, End][], role: Role = "line"): void {
    const geo = new BufferGeometry();
    geo.setAttribute("position", new BufferAttribute(new Float32Array(Math.max(1, pairs.length) * 6), 3));
    const mat = new LineBasicMaterial({ color: this.pal.role[role], transparent: true, opacity: 0, depthWrite: false });
    const obj = new LineSegments(geo, mat);
    obj.visible = false;
    this.scene.add(obj);
    this.links.set(name, { pairs, geo, mat, obj, role, op: 0, from: 0, to: 0 });
  }

  /** An HTML label anchored to a dot or a point, offset in pixels; `center` centres it. */
  label(id: string, html: string, anchor: End, opts: { cls?: string; dx?: number; dy?: number; center?: boolean } = {}): HTMLDivElement {
    const el = document.createElement("div");
    el.className = `how-label ${opts.cls ?? ""}`;
    el.innerHTML = html;
    el.style.opacity = "0";
    this.layer.appendChild(el);
    this.labels.set(id, { id, el, anchor, dx: opts.dx ?? 0, dy: opts.dy ?? 0, center: opts.center ?? false, on: false });
    return el;
  }

  /** Remove everything a chapter added. */
  clear(): void {
    for (const d of this.dots.values()) {
      this.scene.remove(d.mesh, d.glow);
      d.mesh.geometry.dispose();
      d.mesh.material.dispose();
      d.glow.material.dispose();
    }
    for (const l of this.links.values()) {
      this.scene.remove(l.obj);
      l.geo.dispose();
      l.mat.dispose();
    }
    for (const l of this.labels.values()) l.el.remove();
    this.dots.clear();
    this.links.clear();
    this.labels.clear();
    this.last = null;
    this.resetView();
  }

  // ------------------------------------------------------------------ states

  private look(l: DotLook): Look {
    const c = l.color;
    const color = typeof c === "string" ? this.pal.role[c].clone() : c instanceof Color ? c.clone() : this.pal.hue(c.hue);
    return { pos: new Vector3(...l.pos), color, alpha: l.alpha ?? 1, scale: l.scale ?? 1, glow: l.glow ?? 0.18 };
  }

  /** Tween to `s`. Dots it does not mention fade out where they are. */
  to(s: SceneState, instant = false): void {
    this.last = s;
    for (const d of this.dots.values()) {
      const spec = s.dots[d.id];
      const to = spec ? this.look(spec) : d.cur ? { ...d.cur, pos: d.cur.pos.clone(), color: d.cur.color.clone(), alpha: 0, glow: 0 } : null;
      if (!to) continue;
      if (!d.cur) d.cur = { ...to, pos: to.pos.clone(), color: to.color.clone(), alpha: 0, glow: 0 };
      d.from = { ...d.cur, pos: d.cur.pos.clone(), color: d.cur.color.clone() };
      d.to = to;
    }
    for (const [name, l] of this.links) {
      l.from = l.op;
      l.to = s.links?.[name] ?? 0;
    }
    const on = new Set(s.labels ?? []);
    for (const l of this.labels.values()) l.on = on.has(l.id);
    this.camFrom = { ...this.camCur, target: [...this.camCur.target] as V3 };
    this.camTo = s.cam;
    this.t0 = performance.now();
    this.dur = instant || this.reduced ? 0 : TWEEN_MS;
    this.kick();
  }

  private retheme(): void {
    this.pal = readPalette();
    const blend = this.pal.dark ? AdditiveBlending : NormalBlending;
    for (const d of this.dots.values()) {
      d.glow.material.blending = blend;
      d.glow.material.needsUpdate = true;
    }
    for (const l of this.links.values()) l.mat.color.copy(this.pal.role[l.role]);
    if (this.last) this.to(this.last, true);
    this.onRetheme?.();
  }

  // ------------------------------------------------------------------ drawing

  private kick(): void {
    if (!this.raf) this.raf = requestAnimationFrame((t) => this.frame(t));
  }

  private endPos(e: End): Vector3 {
    if (typeof e === "string") return this.dots.get(e)?.mesh.position ?? this.tmp.set(0, 0, 0);
    return new Vector3(...e);
  }

  private frame(now: number): void {
    this.raf = 0;
    const w = this.canvas.clientWidth;
    const h = this.canvas.clientHeight;
    if (!w || !h) return;
    const pr = this.renderer.getPixelRatio();
    if (this.canvas.width !== Math.floor(w * pr) || this.canvas.height !== Math.floor(h * pr)) {
      this.renderer.setSize(w, h, false);
      this.camera.aspect = w / h;
      this.camera.updateProjectionMatrix();
    }
    const k = this.dur ? EASE(Math.min(1, (now - this.t0) / this.dur)) : 1;
    const lerp = (a: number, b: number) => a + (b - a) * k;
    const glowScale = this.pal.dark ? 1 : 0.5;

    for (const d of this.dots.values()) {
      if (!d.from || !d.to || !d.cur) continue;
      const c = d.cur;
      c.pos.lerpVectors(d.from.pos, d.to.pos, k);
      c.color.copy(d.from.color).lerp(d.to.color, k);
      c.alpha = lerp(d.from.alpha, d.to.alpha);
      c.scale = lerp(d.from.scale, d.to.scale);
      c.glow = lerp(d.from.glow, d.to.glow);
      const visible = c.alpha > 0.01;
      d.mesh.visible = visible;
      d.glow.visible = visible && c.glow > 0.01;
      d.mesh.position.copy(c.pos);
      d.mesh.scale.setScalar(c.scale);
      d.mesh.material.color.copy(c.color);
      d.mesh.material.opacity = c.alpha;
      d.glow.position.copy(c.pos);
      d.glow.scale.setScalar(d.size * 7.6 * c.scale);
      d.glow.material.color.copy(c.color);
      d.glow.material.opacity = c.glow * glowScale;
    }
    for (const l of this.links.values()) {
      l.op = lerp(l.from, l.to);
      l.mat.opacity = l.op;
      l.obj.visible = l.op > 0.005;
      if (!l.obj.visible) continue;
      const arr = l.geo.attributes.position!.array as Float32Array;
      l.pairs.forEach(([a, b], i) => {
        const A = this.endPos(a);
        arr[i * 6] = A.x;
        arr[i * 6 + 1] = A.y;
        arr[i * 6 + 2] = A.z;
        const B = this.endPos(b);
        arr[i * 6 + 3] = B.x;
        arr[i * 6 + 4] = B.y;
        arr[i * 6 + 5] = B.z;
      });
      l.geo.attributes.position!.needsUpdate = true;
    }

    const cf = this.camFrom;
    const ct = this.camTo;
    const cc: CamSpec = {
      target: [lerp(cf.target[0], ct.target[0]), lerp(cf.target[1], ct.target[1]), lerp(cf.target[2], ct.target[2])],
      yaw: lerp(cf.yaw, ct.yaw),
      pitch: lerp(cf.pitch, ct.pitch),
      dist: lerp(cf.dist, ct.dist),
    };
    this.camCur = cc;
    const yaw = cc.yaw + this.user.yaw;
    const pitch = cc.pitch + this.user.pitch;
    // Pull back so the scene fits beside the caption, not under it.
    const fit = this.inset > 0 && this.inset < w * 0.6 ? Math.min(1.3, w / (w - this.inset)) : 1;
    const dist = cc.dist * this.user.zoom * (w < 640 ? 1.6 : fit);
    this.camera.position.set(
      cc.target[0] + dist * Math.cos(pitch) * Math.sin(yaw),
      cc.target[1] + dist * Math.sin(pitch),
      cc.target[2] + dist * Math.cos(pitch) * Math.cos(yaw),
    );
    this.camera.lookAt(...cc.target);
    if (this.inset > 0 && this.inset < w * 0.6) this.camera.setViewOffset(w, h, -this.inset / 2, 0, w, h);
    else this.camera.clearViewOffset();
    this.renderer.render(this.scene, this.camera);

    for (const l of this.labels.values()) {
      const p = this.tmp.copy(this.endPos(l.anchor)).project(this.camera);
      const show = l.on && p.z < 1;
      l.el.style.opacity = show ? "1" : "0";
      if (show) {
        const x = (p.x * 0.5 + 0.5) * w + l.dx;
        const y = (-p.y * 0.5 + 0.5) * h + l.dy;
        l.el.style.transform = `translate(${x}px, ${y}px)${l.center ? " translateX(-50%)" : ""}`;
      }
    }

    if (k < 1 || this.dragging) this.kick();
  }

  dispose(): void {
    cancelAnimationFrame(this.raf);
    this.clear();
    for (const f of this.cleanup) f();
    this.tex.dispose();
    this.renderer.dispose();
  }
}
