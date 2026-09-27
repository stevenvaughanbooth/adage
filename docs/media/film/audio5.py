#!/usr/bin/env python3
"""Score for the Adage film, v6 generator (audio5.py).

112 BPM, 42 bars = 90.0 s, one global beat grid, three true hits, two true silences.
Everything is synthesized here: no samples, no soundfonts, no external files. Deterministic (seeded),
48 kHz, STEREO. Timeline constants `T` and session schedule `SS` must match index6.html exactly.

Production toolkit (all vectorized, numpy + scipy.signal):
  oscillators   DPW saw (2nd-order anti-aliased), triangle, sine, FM bell
  filters       RBJ biquads (lp/hp/bp/shelves/peak) via sosfilt, block-wise time-varying sweeps
  drums         layered kick (pitched sub + click + body noise), clap, finger snap, shaped hats
  synths        detuned-unison pad through a resonant low-pass with envelopes, synth pluck arp
                (velocity + swing + humanize), FM glass lead, sub bass with sidechain
  fx            riser (noise band sweep + rising tone + accelerating ticks), reverse cymbal, 3 distinct hits
  space         two convolution reverbs from synthesized IRs (hall 3.2 s, room 0.7 s), ping-pong delay,
                Haas offsets and L/R detune spread, mid/side width on pads
  master        bus HPF, mud cut, two-band tilt, gentle bus compressor, 4x-oversampled lookahead
                brickwall limiter at -1 dBTP, true silences forced to digital zero after the master.

Gain convention: every one-shot instrument is peak-normalized to 1.0, pads are RMS-normalized to 1.0,
so the numbers in the compose section are dBFS-readable (0.5 = -6 dB peak, pad g .07 = -23 dB RMS)
before the per-bus TRIM and MASTER in the master section. Run: python3 audio5.py soundtrack5.wav
"""
import numpy as np, wave, sys, time
from scipy import signal
from scipy.ndimage import maximum_filter1d, uniform_filter1d

t_start = time.time()
sr = 48000; bpm = 112; beat = 60 / bpm; bar = beat * 4; DUR = 90.0; n = int(sr * DUR)
T = dict(open=0, wander=6.429, maze=12.857, reflex=17.143, instinct=23.571, session=30.0, evidence=47.143,
         cool=53.571, fleet=60.0, numbers=66.429, trust=75.0, close=81.429, end=90.0)
SS = dict(prompt=0, stepin=1.071, sigR=2.143, hop1=4.286, sigB=5.893, hop2=7.5, hop3=9.107, sigC=10.714,
          hop4=11.786, hop5=12.857, done=13.393)
SEED = 2026
rng = np.random.default_rng(SEED)
TAU = 2 * np.pi
grid = lambda a, b, step=beat: np.arange(np.ceil(a / step - 1e-6) * step, b - 1e-6, step)
m2f = lambda m: 440.0 * 2 ** ((m - 69) / 12)
tl = lambda L: np.arange(L) / sr
db = lambda x: 10 ** (x / 20)
nrm = lambda x: x / (np.abs(x).max() + 1e-12)

# ------------------------------------------------------------------ buses
BUS = {k: np.zeros((n, 2)) for k in ('sub', 'kick', 'drums', 'music', 'lead', 'fx')}
SEND = {k: np.zeros((n, 2)) for k in ('hall', 'room', 'delay')}

def stereo(sig, pan=0.0):
    if sig.ndim == 2: return sig
    a = (pan + 1) * np.pi / 4
    return np.stack([sig * np.cos(a), sig * np.sin(a)], axis=1)

def place(dst, t, st, gain=1.0):
    i = int(round(t * sr)); j0 = 0
    if i < 0: j0, i = -i, 0
    L = min(len(st) - j0, n - i)
    if L > 0: dst[i:i + L] += st[j0:j0 + L] * gain

def add(bus, t, sig, gain=1.0, pan=0.0, hall=0.0, room=0.0, delay=0.0):
    st = stereo(sig, pan) * gain
    place(BUS[bus], t, st)
    if hall: place(SEND['hall'], t, st, hall)
    if room: place(SEND['room'], t, st, room)
    if delay: place(SEND['delay'], t, st, delay)

def widen(st, w):
    m = (st[:, 0] + st[:, 1]) * .5; s = (st[:, 0] - st[:, 1]) * .5 * w
    return np.stack([m + s, m - s], axis=1)

# ------------------------------------------------------------------ filters
def rbj(kind, fc, q=0.707, gdb=0.0):
    fc = min(max(float(fc), 10.0), sr * 0.45); w = TAU * fc / sr; c = np.cos(w); s = np.sin(w)
    al = s / (2 * q); A = 10 ** (gdb / 40); sA = np.sqrt(A)
    if kind == 'lp': b = [(1 - c) / 2, 1 - c, (1 - c) / 2]; a = [1 + al, -2 * c, 1 - al]
    elif kind == 'hp': b = [(1 + c) / 2, -(1 + c), (1 + c) / 2]; a = [1 + al, -2 * c, 1 - al]
    elif kind == 'bp': b = [al, 0, -al]; a = [1 + al, -2 * c, 1 - al]
    elif kind == 'pk': b = [1 + al * A, -2 * c, 1 - al * A]; a = [1 + al / A, -2 * c, 1 - al / A]
    elif kind == 'ls':
        b = [A * ((A + 1) - (A - 1) * c + 2 * sA * al), 2 * A * ((A - 1) - (A + 1) * c), A * ((A + 1) - (A - 1) * c - 2 * sA * al)]
        a = [(A + 1) + (A - 1) * c + 2 * sA * al, -2 * ((A - 1) + (A + 1) * c), (A + 1) + (A - 1) * c - 2 * sA * al]
    elif kind == 'hs':
        b = [A * ((A + 1) + (A - 1) * c + 2 * sA * al), -2 * A * ((A - 1) + (A + 1) * c), A * ((A + 1) + (A - 1) * c - 2 * sA * al)]
        a = [(A + 1) - (A - 1) * c + 2 * sA * al, 2 * ((A - 1) - (A + 1) * c), (A + 1) - (A - 1) * c - 2 * sA * al]
    else: raise ValueError(kind)
    return np.array([[b[0] / a[0], b[1] / a[0], b[2] / a[0], 1.0, a[1] / a[0], a[2] / a[0]]])

def filt(x, kind, fc, q=0.707, gdb=0.0):
    return signal.sosfilt(rbj(kind, fc, q, gdb), x, axis=0)

def butter_hp(x, fc, order=2):
    return signal.sosfilt(signal.butter(order, fc, 'hp', fs=sr, output='sos'), x, axis=0)

def sweep(x, fc, q=1.0, kind='lp', block=192):
    """Time-varying RBJ biquad: fc is a per-sample cutoff array (or scalar); x is (L,) or (L, 2)."""
    L = len(x); fc = np.broadcast_to(np.asarray(fc, float), (L,))
    y = np.empty_like(x); zi = np.zeros((1, 2) + x.shape[1:])
    for s in range(0, L, block):
        e = min(L, s + block)
        y[s:e], zi = signal.sosfilt(rbj(kind, fc[(s + e) // 2], q), x[s:e], axis=0, zi=zi)
    return y

# ------------------------------------------------------------------ envelopes, oscillators
def env(L, a, r, hold):
    """sine-shaped attack over a s, sustain to hold s, cosine release over r s (reaches 0 at hold + r)."""
    t = tl(L)
    att = np.sin(np.clip(t / max(a, 1e-4), 0, 1) * np.pi / 2) ** 2
    rel = np.cos(np.clip((t - hold) / max(r, 1e-4), 0, 1) * np.pi / 2) ** 2
    return att * rel

def noise(L, ch=1):
    return rng.standard_normal((L, ch)) if ch > 1 else rng.standard_normal(L)

def phase(f, L, ph0=0.0):
    f = np.broadcast_to(np.asarray(f, float), (L,))
    return (ph0 + np.cumsum(f) / sr) % 1.0

def saw(f, L, ph0=None):
    """Differentiated-parabolic-waveform saw: cheap, vectorized, aliasing far below a naive saw."""
    ph0 = rng.random() if ph0 is None else ph0
    f = np.broadcast_to(np.asarray(f, float), (L,))
    p = 2 * phase(f, L, ph0) - 1; y = p * p
    d = np.empty(L); d[1:] = np.diff(y); d[0] = 0.0
    return d * sr / (4 * f)

def tri(f, L, ph0=None):
    ph0 = rng.random() if ph0 is None else ph0
    return 2 * np.abs(2 * phase(f, L, ph0) - 1) - 1

def sine(f, L, ph0=0.0):
    return np.sin(TAU * phase(f, L, ph0))

# ------------------------------------------------------------------ instruments (one-shots peak-normalized)
def kick(vel=1.0, soft=False, f0=50.0):
    """Layered kick: pitched sub sine (fast pitch envelope, soft-clipped) + noise click + short body."""
    L = int(sr * .6); t = tl(L); f0 = f0 * (1 + rng.normal(0, .012))
    f = f0 * (1 + 3.0 * np.exp(-t / .024))
    sub = np.sin(TAU * np.cumsum(f) / sr) * np.exp(-t / (.17 if soft else .30))
    sub = np.tanh(1.7 * sub) / np.tanh(1.7)
    nz = noise(L)
    body = filt(nz, 'bp', 240, 1.3) * np.exp(-t / .032) * .45
    clk = filt(nz, 'hp', 2600, .8) * np.exp(-t / .0028) * .55 + np.sin(TAU * 1500 * t) * np.exp(-t / .0022) * .3
    y = sub * (.72 if soft else 1.0) + (body + clk) * (.3 if soft else 1.0)
    return nrm(y) * vel

def clap(vel=1.0):
    L = int(sr * .38); t = tl(L); nz = noise(L)
    e = np.zeros(L)
    for k, off in enumerate((0.0, .0085, .0175, .026)):
        e += np.where(t >= off, np.exp(-(t - off) / .0055), 0) * (1 - .12 * k)
    e += np.where(t >= .026, np.exp(-(t - .026) / .085), 0) * .55
    x = filt(nz, 'bp', 1500, .9) * e + filt(nz, 'bp', 320, 2.0) * np.exp(-t / .045) * .4
    return nrm(filt(x, 'hp', 400, .7)) * vel

def snap():
    """Finger snap: sharp band-passed burst + tiny click + a little low body."""
    L = int(sr * .2); t = tl(L); nz = noise(L)
    x = filt(nz, 'bp', 2400, 1.8) * np.exp(-t / .018) * 1.3
    x += filt(nz, 'bp', 5200, 2.0) * np.exp(-t / .006) * .5
    x += np.sin(TAU * 190 * t) * np.exp(-t / .025) * .35
    return nrm(x)

def hat(vel=1.0, open_=False):
    L = int(sr * (.32 if open_ else .075)); t = tl(L)
    x = filt(noise(L), 'hp', 7800, .9) * (1 + .25 * np.sin(TAU * 5300 * t))
    x = filt(x, 'pk', 10500, 1.0, 3.0)
    return nrm(x * np.exp(-t / (.11 if open_ else .019)) * np.minimum(1, t / .0008)) * vel

def bell(f, dur=.6, ratio=3.5, index=1.2, vel=1.0, tau=None):
    """2-op FM bell with decaying index and a second harmonic ring."""
    L = int(sr * dur); t = tl(L); tau = tau or dur / 4
    mod = index * np.exp(-t / (tau * .6)) * np.sin(TAU * f * ratio * t)
    y = np.sin(TAU * f * t + mod) * np.exp(-t / tau)
    y += .25 * np.sin(TAU * f * 2.003 * t + .5 * mod) * np.exp(-t / (tau * .45))
    return nrm(y * np.minimum(1, t / .0015)) * vel

def tick(f=1800.0, g=.16, dur=.16):
    return bell(f, dur, ratio=2.4, index=.7, tau=dur / 5) * g

def subnote(f, dur, attack=.01, release=.15, g=1.0):
    L = int(sr * (dur + release)); t = tl(L)
    y = np.sin(TAU * f * t) + .12 * np.sin(TAU * 2 * f * t)
    y = np.tanh(1.3 * y) / np.tanh(1.3)
    return nrm(y) * env(L, attack, release, dur) * g

def pad(notes, dur, open_=0.7, g=1.0, close=False, attack=0.8, release=1.2, q=1.1, lo=170, hi=2600, lfo=0.0):
    """Six detuned saw+triangle voices per note spread L/R, through a resonant low-pass whose cutoff
    follows the chord (opens over the note, or closes when close=True), with a sine attack / cosine release.
    RMS-normalized over the sustained part, so g is the sustained RMS."""
    L = int(sr * (dur + release)); t = tl(L); ch = np.zeros((L, 2))
    for m in notes:
        f = m2f(m)
        for det, c in ((-11, 0), (-3, 0), (6, 0), (-7, 1), (2, 1), (11, 1)):
            ff = f * 2 ** (det / 1200)
            ch[:, c] += saw(ff, L) * .55 + tri(ff, L) * .45
    prog = np.clip(t / dur, 0, 1)
    shape = (1 - prog) ** 1.6 * .9 + .1 if close else .35 + .65 * np.sin(prog * np.pi / 2)
    cut = lo + (hi - lo) * open_ * shape
    if lfo: cut = cut * (1 + .22 * np.sin(TAU * lfo * t + 1.0))
    cut = cut * np.clip(t / (attack * .7 + 1e-3), .35, 1)
    y = sweep(ch, cut, q)
    e = env(L, attack, release, dur)
    mask = e > .5 if (e > .5).any() else np.ones(L, bool)
    y /= np.sqrt(np.mean(y[mask] ** 2)) + 1e-9
    return y * e[:, None] * g

def pluck(f, vel=1.0, dur=.5):
    """Synth pluck: two detuned saws + sub-octave triangle through a fast-closing resonant low-pass."""
    L = int(sr * dur); t = tl(L)
    x = saw(f, L) * .5 + saw(f * 2 ** (7 / 1200), L) * .35 + tri(f * .5, L) * .18
    cut = 380 + 6200 * vel * (1 + rng.normal(0, .08)) * np.exp(-t / .075)
    y = sweep(x, cut, 1.9)
    y *= np.minimum(1, t / .0015) * np.exp(-t / .16) * np.cos(np.clip((t - dur + .06) / .06, 0, 1) * np.pi / 2) ** 2
    return nrm(y) * vel

def lead(m, dur, vel=1.0, bright=1.0):
    """FM 'glass' lead: two carriers detuned L/R, index decays over the note; slow attack, soft release."""
    f = m2f(m); L = int(sr * (dur + .6)); t = tl(L); out = np.zeros((L, 2))
    for c, det in ((0, -5), (1, 5)):
        ff = f * 2 ** (det / 1200)
        idx = (.9 * np.exp(-t / .6) + .35) * bright
        mod = idx * np.sin(TAU * ff * 2 * t) + .3 * idx * np.sin(TAU * ff * 3.01 * t + .7)
        out[:, c] = np.sin(TAU * ff * t + mod) + .18 * np.sin(TAU * ff * 2 * t) * np.exp(-t / .4)
    return nrm(out * env(L, .035, .55, dur)[:, None]) * vel

def wood(vel=1.0):
    """Dull, low, muted tick for the control agent's dotted-eighth steps."""
    L = int(sr * .06); t = tl(L)
    return nrm(filt(noise(L), 'bp', 520, 4.0) * np.exp(-t / .012)) * vel

def riser(dur, g=1.0):
    """Noise band sweeping up + rising detuned tone through an opening low-pass + accelerating ticks. Peak = g."""
    L = int(sr * dur); t = tl(L); u = t / dur
    nz = noise(L, 2)
    band = sweep(nz, 260 * (26 ** u), .8, 'bp') * (u ** 2)[:, None] * 1.4
    fr = 55 * 8 ** u
    tone = np.stack([saw(fr * 2 ** (-6 / 1200), L), saw(fr * 2 ** (6 / 1200), L)], axis=1)
    tone = sweep(tone, 250 + 5500 * u ** 2, 1.4) * (u ** 1.6)[:, None] * .45
    tk = np.zeros((L, 2)); N = 40
    for k in range(1, N + 1):
        tt = dur * np.sqrt(k / N) * .995; i = int(tt * sr); s = tick(1100 + 900 * (tt / dur), .3 * (tt / dur), .08)
        Ls = min(len(s), L - i)
        if Ls > 0: tk[i:i + Ls] += stereo(s[:Ls], .5 * np.sin(k * 2.1))
    return nrm(band + tone + tk) * g

def rcymbal(dur, g=.3):
    """Reverse cymbal: bright noise + inharmonic partials with a rising envelope that ends at zero. Peak = g."""
    L = int(sr * dur); t = tl(L); nz = noise(L, 2)
    x = filt(nz, 'hp', 4200, .7) * .8
    am = 1 + .6 * filt(noise(L), 'lp', 30, .7)
    for k, pf in enumerate((3120, 4730, 6190, 7890, 9720)):
        x[:, k % 2] += np.sin(TAU * pf * t + .3 * k) * .12 * am
    e = np.exp((t - dur) / (dur * .26)); e[-int(.004 * sr):] *= np.linspace(1, 0, int(.004 * sr))
    return nrm(x * e[:, None]) * g

def hit(low, tail, air, grit, bright, tone=None, click=1.0):
    """Returns (sub mono, body stereo), both peak 1. sub: pitched boom; body: saturated mid grit + wide
    down-sweeping air (Haas-offset) + click (+ a tonal bloom for HIT 3)."""
    L = int(sr * tail); t = tl(L)
    f = low * (1 + 2.2 * np.exp(-t / .07))
    sub = np.sin(TAU * np.cumsum(f) / sr) * np.exp(-t / (tail / 3.2)) * np.minimum(1, t / .002)
    sub = np.tanh(1.6 * sub) / np.tanh(1.6)
    mid = filt(saw(low * 2, L) * np.exp(-t / .35), 'lp', 340, .9); mid = np.tanh(mid * 4) * grit * .35
    fc = bright * np.exp(-t / .5) + 220
    airs = sweep(noise(L, 2), fc, .8, 'bp') * np.exp(-t / (tail * .3))[:, None] * air * 1.2
    airs[:, 1] = np.roll(airs[:, 1], int(sr * .011)); airs[:int(sr * .011), 1] = 0
    clk = filt(noise(L), 'hp', 3000, .7) * np.exp(-t / .004) * .6 * click
    body = stereo(mid + clk) + airs
    if tone:
        tn = np.sin(TAU * tone * t) * .6 + np.sin(TAU * tone * 2 * t) * .2
        body += stereo(tn * env(L, .08, tail * .45, tail * .4)) * .22
    return nrm(sub), nrm(body)

def play_hit(t, kind):
    # hit gains sit above 1.0 on purpose: the master trim brings them to the limiter ceiling, so the three
    # hits are the only events that touch the brickwall (1-2 dB of GR), everything else stays dynamic
    if kind == 1: sub, body = hit(56, 2.6, .30, .6, 5200, click=1.0); hg, bg, hall = 2.0, .95, .35
    elif kind == 2: sub, body = hit(50, 3.5, .36, 1.0, 7500, click=1.2); hg, bg, hall = 2.0, 1.0, .45
    else: sub, body = hit(52, 3.0, .22, .3, 3000, tone=220, click=.45); hg, bg, hall = 2.0, .85, .6
    add('kick', t, sub, hg); add('fx', t, body, bg, hall=hall)

def thump(f, g=.6):
    L = int(sr * .5); t = tl(L)
    return nrm(kick(1.0, f0=f)[:L] * .8 + .6 * np.sin(TAU * f * t) * np.exp(-t / .22)) * g

# ------------------------------------------------------------------ patterns
def hats(t0, t1, g=1.0, swing=.03, sec=None):
    sec = t0 if sec is None else sec
    for b in grid(t0, t1, beat / 2):
        e = int(round((b - sec) / (beat / 2)))
        acc = e % 4 == 0; op = e % 8 == 7
        vel = np.clip((1.0 if acc else .7) * (1 + rng.normal(0, .12)), .4, 1.3)
        dt = (swing * beat / 2 if e % 2 else 0) + rng.normal(0, .0022)
        add('drums', b + dt, hat(vel, op), g * (.7 if op else 1), pan=.22 + rng.normal(0, .04), room=.2)

def kicks(t0, t1, g=.5, soft=False, every=1):
    for j, b in enumerate(grid(t0, t1)):
        if j % every == 0: add('kick', b, kick(np.clip(1 + rng.normal(0, .05), .8, 1.15), soft), g, room=.1)

def claps(t0, t1, g=.2):
    for b in grid(t0, t1):
        e = int(round((b - t0) / beat))
        if e % 4 in (1, 3): add('drums', b + rng.normal(0, .0015), clap(np.clip(1 + rng.normal(0, .1), .7, 1.2)), g, room=.35, hall=.18)

def arp(notes, t0, t1, rate, g, swing=.0, hall=.15, delay=.3, octave=0):
    t = t0; k = 0
    while t < t1 - 1e-6:
        m = notes[k % len(notes)] + octave
        vel = np.clip((1.0 if k % 4 == 0 else .82) * (1 + rng.normal(0, .08)), .6, 1.15)
        dt = (swing * rate if k % 2 else 0) + rng.normal(0, .0025)
        add('music', t + dt, pluck(m2f(m), vel), g, pan=.4 * np.sin(k * 1.7), hall=hall, delay=delay)
        t += rate; k += 1

def chords(seq, t0, nbars, open_, g, hall=.35, attack=.5, release=.7, roots=None, subg=.15, q=1.1, hi=2600, lfo=0.0, wide=1.0):
    for k in range(nbars):
        t = t0 + k * bar
        p = pad(seq[k % len(seq)], bar, open_, g, attack=attack, release=release, q=q, hi=hi, lfo=lfo)
        add('music', t, widen(p, wide) if wide != 1 else p, hall=hall)
        if roots: add('sub', t, subnote(roots[k % len(roots)], bar - .06, .012, .09), subg)

MOTIF = [64, 69, 72, 71]  # E4 A4 C5 B4: up a fourth, up a minor third, step down onto the tension note

def motif(t0, seq=MOTIF, beats=(0, .5, 1, 2), lens=(.5, .5, 1, 1.6), g=1.0, bright=1.0, octave=0, hall=.4, delay=.3):
    for m, bt, ln in zip(seq, beats, lens):
        add('lead', t0 + bt * beat, lead(m + octave, ln * beat, bright=bright), g, hall=hall, delay=delay)

SC = np.ones(n)
def sidechain(t0, t1, depth, step=beat):
    for b in grid(t0, t1, step):
        i = int(round(b * sr)); L = min(int(sr * step), n - i); x = tl(L)
        SC[i:i + L] *= 1 - depth * np.exp(-x / .11) * (1 - np.exp(-x / .004))

# ------------------------------------------------------------------ harmony
Am = [45, 52, 57, 60, 64]; F = [41, 48, 53, 57, 60]; C = [48, 55, 60, 64, 67]; G = [43, 50, 55, 59, 62]; D = [50, 57, 62, 66, 69]
rA, rC, rG, rD, rF = 55.0, 65.41, 49.0, 73.42, 43.65

# ================================================================== compose
# 0 · cold open: one tone (55 Hz sub + a faint A3/E4 pad), three soft ticks as the line types
add('sub', 0.0, subnote(55, 5.4, attack=1.8, release=1.0), .11)
add('music', 0.0, pad([57, 64], 5.2, .25, .03, attack=2.6, release=1.2, lo=120, hi=1400), hall=.4)
for j in range(3): add('drums', .5 + j * .09, tick(740 + j * 120, .05), pan=-.25 + j * .25, hall=.35)

# 1 · wander: half-time pulse (soft kick on 1 and 3), the hero's 24 steps as ticks on 8ths, Am pad
kicks(T['wander'], T['maze'], .3, soft=True, every=2)
for k in range(24):
    vel = np.clip((.85 if k % 2 else 1.0) * (1 + rng.normal(0, .1)), .6, 1.2)
    add('drums', T['wander'] + k * beat / 2, tick(1400 + (k % 3) * 200, .07 * vel), pan=.3 * np.sin(k * np.pi / 3), room=.3, hall=.1)
add('music', T['wander'], pad(Am, T['maze'] - T['wander'], .4, .045, attack=1.2, release=1.0), hall=.35)
add('sub', T['wander'], subnote(rA, T['maze'] - T['wander'] - .3, .6, .3), .12)

# 2 · maze: reverse cymbal into the downbeat, full kick, hats on 8ths, F pad, riser into HIT 1
add('fx', T['maze'] - bar, rcymbal(bar, .25), hall=.35)
kicks(T['maze'], T['reflex'], .55)
hats(T['maze'] + beat / 2, T['reflex'], .13, sec=T['maze'])
add('music', T['maze'], pad(F, T['reflex'] - T['maze'], .6, .055, attack=.6, release=.9), hall=.3)
add('sub', T['maze'], subnote(rF, T['reflex'] - T['maze'] - .3, .05, .2), .16)
add('fx', T['reflex'] - bar, riser(bar, .6), hall=.2)

# 3 · reflex: HIT 1, the lift Am–C–G–D, arp 8ths → 16ths, kick every beat, hats, soft claps; the motif
play_hit(T['reflex'], 1)
chords([Am, C, G, D], T['reflex'], 6, .9, .07, roots=[rA, rC, rG, rD], subg=.18)
arp([57, 64, 69, 72, 71, 69], T['reflex'], T['instinct'] + 4 * beat, beat / 2, .17, swing=.06)
arp([69, 72, 76, 81, 76, 72], T['instinct'] + 4 * beat, T['session'], beat / 4, .14, swing=.04)
kicks(T['reflex'], T['session'], .55)
hats(T['reflex'] + beat / 2, T['session'], .13, sec=T['reflex'])
claps(T['reflex'] + bar, T['session'], .18)
motif(T['reflex'] + bar, g=.16)                                   # first statement
motif(T['instinct'] + 4 * beat, seq=[64, 69, 72, 76], g=.16)      # answer, ends high (unresolved)
sidechain(T['reflex'], T['session'], .3)

# 4 · instinct: 2-beat reverse cymbal into the ring launch, snap on the launch
add('fx', T['instinct'], rcymbal(2 * beat, .2), hall=.3)
add('drums', T['instinct'] + 2 * beat, snap(), .3, room=.3, hall=.25); add('kick', T['instinct'] + 2 * beat, kick(1.0, soft=True), .3)

# 5 · session: soft pulse, snaps on the four reflex fires, ticks on the five hops, the control agent's
#     dotted-eighth walk (3:4 against the pulse), its death as the 40 Hz thud, ding at done; motif quoted
add('sub', T['session'], subnote(55, .9, .05, .3), .14)
kicks(T['session'], T['evidence'], .3, soft=True)
chords([Am, F, C, G], T['session'], 8, .6, .05, roots=[rA, rF, rC, rG], subg=.12, attack=.6)
for k, b in enumerate(grid(T['session'] + beat / 2, T['evidence'], beat / 2)):
    if T['session'] + 22 * beat - 1e-3 <= b < T['session'] + 26 * beat - 1e-3: continue  # hats out for the thud bar
    add('drums', b + rng.normal(0, .002), hat(np.clip(.6 + rng.normal(0, .1), .3, .9)), .05, pan=.25, room=.15)
for dt in (SS['stepin'], SS['sigR'], SS['sigB'], SS['sigC']):
    add('drums', T['session'] + dt, snap(), .3, room=.35, hall=.25); add('kick', T['session'] + dt, kick(1.0, soft=True), .3)
for dt in (SS['hop1'], SS['hop2'], SS['hop3'], SS['hop4'], SS['hop5']):
    add('drums', T['session'] + dt, tick(900, .12), room=.3, hall=.2)
for k in range(29):
    add('drums', T['session'] + .67 + k * .402, wood(np.clip(1 + rng.normal(0, .12), .6, 1.3)), .055, pan=-.35, room=.25)
sub_t, body_t = hit(40, 1.4, .1, .2, 1500, click=.4)
add('kick', T['session'] + .67 + 29 * .402, sub_t, .5); add('fx', T['session'] + .67 + 29 * .402, body_t, .3, hall=.3)
motif(T['session'] + SS['hop1'], beats=(0, 1, 2, 3), lens=(.9, .9, .9, 1.8), g=.1, bright=.45, hall=.5, delay=.35)
motif(T['session'] + SS['sigC'], beats=(0, 1, 2, 3), lens=(.9, .9, .9, 1.4), g=.11, bright=.5, hall=.5, delay=.35)
add('drums', T['session'] + SS['done'], snap(), .28, room=.3, hall=.3)
add('lead', T['session'] + SS['done'], bell(880, 1.4, 3.5, 1.0, .5), .14, hall=.6)   # the ding = A, the motif resolves
sidechain(T['session'], T['evidence'], .18)

# 6 · evidence: pad Am/F, kick on 1 and 3, ticks on the four cards, 7-beat riser, then SILENCE
chords([Am, F], T['evidence'], 3, .5, .045, attack=.5, release=.6, roots=[rA, rF], subg=.11)
kicks(T['evidence'], T['cool'] - beat, .3, soft=True, every=2)
for i in range(4): add('drums', T['evidence'] + (1 + i) * beat, tick(1200 + i * 200, .08), pan=-.3 + .2 * i, hall=.3)
add('fx', T['cool'] - 8 * beat, riser(7 * beat, .7), hall=.25)

# 7 · cool: HIT 2, the drop: kick every beat, hats + claps to b8, wide pads, motif arp 16ths, sidechain;
#     then the low-pass closes as the city cools, reverse cymbal into the fleet
play_hit(T['cool'], 2)
chords([Am, C], T['cool'], 2, 1.0, .085, roots=[rA, rC], subg=.2, hi=3200, wide=1.3)
add('music', T['cool'] + 8 * beat, widen(pad(Am, 4 * beat, 1.0, .08, close=True, attack=.4, release=.8, hi=3200), 1.3), hall=.4)
add('sub', T['cool'] + 8 * beat, subnote(rA, 4 * beat - .1, .02, .3), .16)
arp([45, 52, 57, 60, 64, 60, 57, 52], T['cool'], T['cool'] + 8 * beat, beat / 4, .17, swing=.04, octave=12, delay=.35)
kicks(T['cool'], T['fleet'], .6)
hats(T['cool'] + beat / 2, T['cool'] + 8 * beat, .15, sec=T['cool'])
claps(T['cool'], T['cool'] + 8 * beat, .25)
motif(T['cool'] + bar, g=.2, bright=1.1); motif(T['cool'] + bar, g=.1, bright=.8, octave=12, hall=.5)
add('fx', T['fleet'] - bar, rcymbal(bar, .25), hall=.35)
sidechain(T['cool'], T['cool'] + 8 * beat, .5); sidechain(T['cool'] + 8 * beat, T['fleet'], .25)

# 8 · fleet: no kick; sub swell (1-bar attack), wide pads Am–F–C–G, eight bells as the repos arrive
add('sub', T['fleet'], subnote(55, 6.0, bar, .4), .18)
chords([Am, F, C, G], T['fleet'], 3, 1.0, .085, attack=.9, release=1.0, hi=3000, lfo=.12, wide=1.5, hall=.45)
for j, fz in enumerate((880, 988, 1047, 1175, 1319, 1397, 1568, 1760)):
    add('lead', T['fleet'] + 4 * beat + j * beat / 2, bell(fz, .7, 3.5, 1.1, .5), .14, pan=-.5 + j / 7, hall=.5, delay=.2)

# 9 · numbers: three thumps (58/52/46 Hz), a low breathing pad that closes exactly on the cut at 75.0
for dt, fz, g in ((0, 58, .5), (4 * beat, 52, .42), (8 * beat, 46, .35)):
    add('kick', T['numbers'] + dt, thump(fz, g), 1.0, room=.15)
add('music', T['numbers'], pad([45, 57, 64], T['trust'] - T['numbers'] - 1.2, .3, .045, attack=1.0, release=1.2, lo=140, hi=1600, lfo=.09))
add('sub', T['numbers'], subnote(55, T['trust'] - T['numbers'] - .5, .8, .5), .1)

# 10 · trust: a lone 55 Hz sine (1-bar attack), one bell when the signal box lands, then TRUE SILENCE
add('sub', T['trust'], subnote(55, 80.357 - T['trust'] - .2, bar, .2), .13)
add('lead', T['trust'] + 4 * beat, bell(2200, .8, 3.5, .9, .5), .12, hall=.4)

# 11 · close: HIT 3 with the wordmark, Am → F → C, three lamp bells (the motif's head), the motif resolves E–A–C–B–A
play_hit(T['close'], 3)
add('music', T['close'], widen(pad(Am, 4.286, .8, .075, attack=.5, release=.7, hi=2400), 1.3), hall=.45)
add('music', T['close'] + 4.286, widen(pad(F, 2.143, .6, .07, attack=.4, release=.6), 1.3), hall=.45)
add('music', T['close'] + 6.429, widen(pad(C, 2.143, .6, .07, attack=.4, release=1.2), 1.3), hall=.45)
add('sub', T['close'], subnote(rA, 4.286 - .08, .05, .15), .16); add('sub', T['close'] + 4.286, subnote(rF, 2.143 - .08, .03, .15), .16)
add('sub', T['close'] + 6.429, subnote(rC, 2.143, .03, .5), .16)
for j, (tt, fz) in enumerate(zip((82.73, 82.85, 82.97), (659.3, 880.0, 1046.5))):
    add('lead', tt, bell(fz, .9, 3.5, 1.0, .5), .14, pan=-.4 + .4 * j, hall=.5)
motif(T['close'], seq=[64, 69, 72, 71, 69], beats=(4, 6, 8, 10, 12), lens=(2, 2, 2, 2, 3.6), g=.16, bright=.9, hall=.5, delay=.25)

# ================================================================== space
def make_ir(rt, pre, hf=.32, lf=.85, early=True):
    """Synthesized impulse response: exponentially decaying stereo noise, three bands with their own decay
    (the tail darkens), a short density build, sparse early reflections, pre-delay; unit energy."""
    L = int(sr * rt * 1.15); t = tl(L); nz = noise(L, 2)
    lo = filt(nz, 'lp', 350, .7); hi = filt(nz, 'hp', 3200, .7); mid = nz - lo - hi
    dec = lambda r: np.exp(-6.91 * t / (rt * r))[:, None]
    ir = lo * dec(lf) * .8 + mid * dec(1.0) + hi * dec(hf) * .9
    ir *= np.minimum(1, t / .012)[:, None]
    if early:
        ref = np.sqrt(np.mean(ir[:int(sr * .05)] ** 2)) * 6
        for tt, gg, ch in ((.007, .5, 0), (.011, .42, 1), (.019, .35, 1), (.027, .3, 0), (.037, .22, 1), (.044, .18, 0)):
            ir[int(tt * sr), ch] += gg * ref
    ir = np.concatenate([np.zeros((int(pre * sr), 2)), ir])
    return ir / np.sqrt(np.sum(ir ** 2, axis=0).mean())

def convolve(x, ir):
    y = np.zeros((n, 2))
    for c in (0, 1): y[:, c] = signal.fftconvolve(x[:, c], ir[:, c])[:n]
    return y

def pingpong(x, dt=beat * .75, fb=.42, taps=7, damp=3400):
    """Dotted-eighth ping-pong delay with progressive high-cut; the send is mono-summed and low-cut."""
    d = int(dt * sr); y = np.zeros((n, 2)); cur = filt(x.mean(axis=1), 'hp', 260, .7)
    for k in range(1, taps + 1):
        cur = filt(cur, 'lp', damp * (.9 ** k), .7) * (.8 if k == 1 else fb)
        y[k * d:, 1 if k % 2 else 0] += cur[:n - k * d]
    return y

# ================================================================== dynamics
def decay_max(x, c):
    """y[i] = max_k (x[i-k] - k*c): peak hold with a linear (in dB) release, by prefix doubling."""
    y = x.copy(); k = 1; rngx = float(x.max() - x.min()) + 1e-9
    while k < len(x) and k * c < rngx:
        y[k:] = np.maximum(y[k:], y[:-k] - k * c); k *= 2
    return y

def compressor(x, thr=-20.0, ratio=1.5, att=.03, rel=.4, knee=6.0):
    """Gentle bus 'glue': 100 ms RMS detector (so transients pass to the limiter), soft knee, log-domain
    release via decay_max, box-smoothed attack."""
    p = uniform_filter1d(np.mean(x ** 2, axis=1), int(sr * .1))
    over = 20 * np.log10(np.sqrt(p) + 1e-9) - thr
    red = np.where(over < -knee / 2, 0.0, np.where(over > knee / 2, over * (1 - 1 / ratio), (1 - 1 / ratio) * (over + knee / 2) ** 2 / (2 * knee)))
    red = decay_max(red, 10 / (rel * sr))
    red = uniform_filter1d(red, max(1, int(att * sr)))
    return x * db(-red)[:, None], red

def limiter(x, ceiling=db(-1.0), la=.003, rel=.12):
    """Lookahead brickwall limiter with a true-peak (4x oversampled) detector; gain is smoothed with a box
    no wider than the lookahead so the ceiling is never exceeded at the sample level."""
    L = int(la * sr)
    up = signal.resample_poly(x, 4, 1, axis=0)
    pk = np.max(np.abs(up), axis=1)[:4 * n].reshape(n, 4).max(axis=1) + 1e-9
    need = np.maximum(0, 20 * np.log10(pk / ceiling))
    need = maximum_filter1d(need, size=2 * L + 1)
    need = decay_max(need, 10 / (rel * sr))
    need = uniform_filter1d(need, L)
    return x * db(-need)[:, None], need

# ================================================================== master
MASTER = 0.8
TRIM = dict(sub=.6, kick=.72, drums=1.7, music=1.5, lead=1.4, fx=1.15)   # bus trims: keep the low end from owning the mix
for k in ('music', 'lead', 'sub'): BUS[k] *= SC[:, None]
for k in SEND: SEND[k] *= SC[:, None]
for k in BUS: BUS[k] *= TRIM[k]
IR_HALL = make_ir(3.2, .022); IR_ROOM = make_ir(.7, .004, hf=.5, lf=.7)
hall = convolve(SEND['hall'], IR_HALL); room = convolve(SEND['room'], IR_ROOM); dly = pingpong(SEND['delay'])
low = butter_hp(BUS['sub'] + BUS['kick'], 22)
music = butter_hp(BUS['music'] + BUS['lead'] + BUS['drums'] + BUS['fx'], 58)
music = filt(music, 'pk', 300, 1.0, -1.5)
wet = butter_hp(hall * .6 + room * .5 + dly * .7, 130)
mix = (low + music + wet) * MASTER
mix = filt(mix, 'ls', 220, .7, -1.5); mix = filt(mix, 'hs', 3400, .7, 1.5)
mix, red_c = compressor(mix)
mix, red_l = limiter(mix)
# the two true silences: digital zero, after everything
mix[int(53.036 * sr):int(T['cool'] * sr)] = 0.0
mix[int(80.357 * sr):int(T['close'] * sr)] = 0.0
fo = int(sr * .6); mix[-fo:] *= np.linspace(1, 0, fo)[:, None]
out = np.clip(mix, -1, 1)

# ================================================================== write + check
path = sys.argv[1] if len(sys.argv) > 1 else "soundtrack5.wav"
pcm = np.round(out * 32767).astype(np.int16)
w = wave.open(path, "wb"); w.setnchannels(2); w.setsampwidth(2); w.setframerate(sr); w.writeframes(pcm.tobytes()); w.close()

def lufs(x, a=None, b=None):
    """ITU-R BS.1770 integrated loudness (K-weighting, 400 ms blocks, absolute + relative gating)."""
    x = x[int((a or 0) * sr):int((b or DUR) * sr)]
    k1 = np.array([[1.53512485958697, -2.69169618940638, 1.19839281085285, 1, -1.69065929318241, 0.73248077421585]])
    k2 = np.array([[1.0, -2.0, 1.0, 1, -1.99004745483398, 0.99007225036621]])
    y = signal.sosfilt(k2, signal.sosfilt(k1, x, axis=0), axis=0)
    B = int(.4 * sr); H = B // 4; nb = (len(y) - B) // H + 1
    if nb < 1: return -np.inf
    ms = np.array([np.mean(y[i * H:i * H + B] ** 2, axis=0).sum() for i in range(nb)])
    lk = -.691 + 10 * np.log10(ms + 1e-12)
    keep = lk > -70
    if not keep.any(): return -np.inf
    rel = -.691 + 10 * np.log10(ms[keep].mean()) - 10
    keep &= lk > rel
    return -.691 + 10 * np.log10(ms[keep].mean())

tp = np.abs(signal.resample_poly(out, 4, 1, axis=0)).max()
print(f"wrote {path}: {len(out) / sr:.3f} s stereo 16-bit {sr} Hz in {time.time() - t_start:.1f} s")
print(f"sample peak {20 * np.log10(np.abs(out).max()):.2f} dBFS   true peak (4x) {20 * np.log10(tp):.2f} dBTP   "
      f"comp GR max {red_c.max():.1f} dB   limiter GR max {red_l.max():.1f} dB")
print(f"integrated loudness {lufs(out):.1f} LUFS (BS.1770, gated)")
names = list(T); print("section        start   end    RMS dBFS   peak dBFS   LUFS")
for a, b in zip(names[:-1], names[1:]):
    seg = out[int(T[a] * sr):int(T[b] * sr)]; rms = np.sqrt(np.mean(seg ** 2)); pk = np.abs(seg).max()
    print(f"{a:10s} {T[a]:7.3f} {T[b]:7.3f}   {20 * np.log10(rms + 1e-12):6.1f}     {20 * np.log10(pk + 1e-12):6.1f}   {lufs(out, T[a], T[b]):6.1f}")
s1 = pcm[int(53.036 * sr):int(T['cool'] * sr)]; s2 = pcm[int(80.357 * sr):int(T['close'] * sr)]
print(f"silence 53.036-53.571: {len(s1)} samples, max |x| = {np.abs(s1).max()}  -> {'DIGITAL ZERO' if np.abs(s1).max() == 0 else 'NOT SILENT'}")
print(f"silence 80.357-81.429: {len(s2)} samples, max |x| = {np.abs(s2).max()}  -> {'DIGITAL ZERO' if np.abs(s2).max() == 0 else 'NOT SILENT'}")
loud = np.concatenate([out[int(T['reflex'] * sr):int(T['session'] * sr)], out[int(T['cool'] * sr):int(T['fleet'] * sr)]])
print(f"loud-section RMS (reflex + cool): {20 * np.log10(np.sqrt(np.mean(loud ** 2))):.1f} dBFS   corr L/R {np.corrcoef(out[:, 0], out[:, 1])[0, 1]:.2f}")
for nm, tt in (('HIT 1', T['reflex']), ('HIT 2', T['cool']), ('HIT 3', T['close'])):
    seg = out[int(tt * sr):int((tt + .5) * sr)]; print(f"{nm} at {tt:.3f}: peak {20 * np.log10(np.abs(seg).max()):.2f} dBFS")
print("band balance (dB re section RMS): <150 Hz | 150-2k | >2k")
for a, b in zip(names[:-1], names[1:]):
    seg = out[int(T[a] * sr):int(T[b] * sr)].mean(axis=1); sp = np.abs(np.fft.rfft(seg)) ** 2; fr = np.fft.rfftfreq(len(seg), 1 / sr)
    tot = sp.sum() + 1e-12; bands = [sp[fr < 150].sum(), sp[(fr >= 150) & (fr < 2000)].sum(), sp[fr >= 2000].sum()]
    print(f"{a:10s} " + "  ".join(f"{10 * np.log10(bv / tot + 1e-12):6.1f}" for bv in bands))
