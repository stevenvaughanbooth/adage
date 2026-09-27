"""Score for the Adage vision cut. 112 BPM, A minor. Locked to the film's timeline constants.
Sections: a low pulse under the maze, a lift when the reflex is learned, a drop on the cooling city,
a wide pad under the fleet, silence before the ask."""
import numpy as np, wave, sys
sr = 48000; bpm = 112; beat = 60 / bpm; DUR = 91.0; n = int(sr * DUR); out = np.zeros(n)
rng = np.random.default_rng(7)
T = dict(open=0, wander=3.8, maze=12.0, reflex=17.0, instinct=22.4, session=27.6, evidence=43.4, cool=50.0, fleet=56.6, numbers=64.0, trust=72.6, close=79.6, end=91.0)
SS = dict(prompt=.2, stepin=1.3, hop1=4.3, sigB=6.0, hop2=7.4, hop3=9.0, sigC=10.6, hop4=12.0, hop5=13.3, done=14.1)

def add(t, sig, gain=1.0):
    i = int(t * sr); L = min(len(sig), n - i)
    if i >= 0 and L > 0: out[i:i + L] += sig[:L] * gain

def env(L, a, d):
    x = np.arange(L) / sr; return np.minimum(1, x / max(a, 1e-4)) * np.exp(-np.maximum(0, x - a) / d)

def kick(soft=False):
    L = int(sr * .55); x = np.arange(L) / sr; f = 58 * np.exp(-x * 11) + 44
    return (0.33 if soft else 0.62) * np.sin(2 * np.pi * np.cumsum(f) / sr) * np.exp(-x * 6.5)

def hat(acc=False):
    L = int(sr * .06); x = np.arange(L) / sr; nz = rng.standard_normal(L)
    return (0.06 if acc else 0.035) * np.diff(nz, prepend=0) * np.exp(-x * 80)

def tick(freq=1800.0, g=0.16):
    L = int(sr * .06); x = np.arange(L) / sr
    return g * np.sin(2 * np.pi * freq * x) * np.exp(-x * 90)

def hit(depth=1.0, low=56):
    L = int(sr * 2.6); x = np.arange(L) / sr
    body = 0.9 * np.sin(2 * np.pi * (low * np.exp(-x * 2.4) + 40) * x) * np.exp(-x * 1.7)
    nz = rng.standard_normal(L); air = np.zeros(L); a = 0.0
    for j in range(L):
        a += 0.07 * (nz[j] - a); air[j] = a
    return depth * (body + 0.22 * air * np.exp(-x * 5))

def riser(dur):
    L = int(sr * dur); x = np.arange(L) / sr; nz = rng.standard_normal(L); b = np.zeros(L); a = 0.0
    for j in range(L):
        a += (0.002 + 0.18 * (x[j] / dur) ** 2) * (nz[j] - a); b[j] = a
    tone = np.sin(2 * np.pi * np.cumsum(110 + 440 * (x / dur) ** 2) / sr) * 0.12 * (x / dur)
    return (b * 0.9 + tone) * (x / dur) ** 1.7

def pad(chord, dur, open_=1.0, g=0.045):
    L = int(sr * dur); x = np.arange(L) / sr; seg = np.zeros(L)
    for f in chord:
        for det in (-0.5, 0.5):
            ph = 2 * np.pi * (f + det) * x; seg += (2 * ((ph / (2 * np.pi)) % 1) - 1) * g
    cut = 0.012 + open_ * 0.09 * np.clip(x / dur, 0, 1); y = np.zeros(L); a = 0.0
    for j in range(L):
        a += cut[j] * (seg[j] - a); y[j] = a
    e = np.clip(x / 0.8, 0, 1) * np.clip((dur - x) / 1.2, 0, 1)
    return y * e

def arp(notes, t0, t1, rate=beat / 2, g=0.07):
    t = t0; k = 0
    while t < t1:
        f = notes[k % len(notes)]; L = int(sr * rate * 1.6); x = np.arange(L) / sr
        s = (np.sin(2 * np.pi * f * x) + 0.4 * np.sin(2 * np.pi * f * 2 * x)) * env(L, .004, .12) * g
        add(t, s); t += rate; k += 1

def sidechain(t0, t1, depth=.55):
    """duck everything between t0 and t1 on each beat (the drop)"""
    b = t0
    while b < t1:
        i = int(b * sr); L = int(sr * beat); x = np.arange(L) / sr
        g = 1 - depth * np.exp(-x * 9); L = min(L, n - i)
        out[i:i + L] *= g[:L]; b += beat

# --- chords (A minor drift): Am, F, C, G  and the lift: Am, C, G, D
Am = [110, 130.8, 164.8]; F = [87.3, 110, 130.8]; C = [130.8, 164.8, 196]; G = [98, 123.5, 146.8]; D = [73.4, 110, 146.8]

# 1 · cold open: a single low tone
add(0.4, pad([55, 110], 4.0, .2, .05))
# 2 · wander + maze: pulse, hats, dark pads
for b in np.arange(T['wander'], T['reflex'], beat): add(b, kick(soft=b < T['maze']))
for b in np.arange(T['wander'] + beat / 2, T['reflex'], beat): add(b, hat(acc=int(round((b - T['wander']) / beat)) % 4 == 3))
t = T['wander']; k = 0
while t < T['reflex']:
    add(t, pad([Am, F][k % 2], beat * 8 + .8, .4)); t += beat * 8; k += 1
# every thought: a tick on each hero step, sharpening as the maze fills
for k in range(24): add(T['wander'] + k * .3, tick(1400 + (k % 3) * 200, .1))
add(T['maze'] - 1.2, riser(1.2)); add(T['maze'], hit(.6))
# 3 · the reflex: lift
add(T['reflex'] - 1.8, riser(1.8)); add(T['reflex'], hit(1.0))
t = T['reflex']; k = 0
while t < T['session']:
    add(t, pad([Am, C, G, D][k % 4], beat * 4 + .6, .9, .05)); t += beat * 4; k += 1
arp([220, 261.6, 329.6, 392, 329.6, 261.6], T['reflex'] + .4, T['instinct'], beat / 2, .05)
add(T['instinct'], hit(.7, 64))
arp([220, 329.6, 440, 523.3, 440, 329.6], T['instinct'] + .8, T['session'], beat / 4, .04)
for b in np.arange(T['reflex'], T['session'], beat): add(b, kick())
for b in np.arange(T['reflex'] + beat / 2, T['session'], beat): add(b, hat(True))
# 4 · session: pulse continues, ticks on hook events, hops
for b in np.arange(T['session'], T['evidence'], beat): add(b, kick(soft=True))
for b in np.arange(T['session'] + beat / 2, T['evidence'], beat): add(b, hat())
t = T['session']; k = 0
while t < T['evidence']:
    add(t, pad([Am, F, C, G][k % 4], beat * 4 + .6, .6, .045)); t += beat * 4; k += 1
add(T['session'] + SS['stepin'], hit(.55, 70))
for dt in (SS['stepin'], SS['sigB'], SS['sigC']): add(T['session'] + dt, tick(2200, .2))
for dt in (SS['hop1'], SS['hop2'], SS['hop3'], SS['hop4'], SS['hop5']): add(T['session'] + dt, tick(900, .12))
add(T['session'] + SS['done'], hit(.5, 80))
# the control agent dies at the cap: a low thud
add(T['session'] + 1.0 + 29 * .43, hit(.45, 40))
# 5 · evidence: quieter, pad only
t = T['evidence']; k = 0
while t < T['cool']:
    add(t, pad([Am, F][k % 2], beat * 4 + .6, .5, .04)); t += beat * 4; k += 1
# 6 · the cooling city: the drop
add(T['cool'] - 2.2, riser(2.2)); add(T['cool'], hit(1.0, 50))
for b in np.arange(T['cool'], T['fleet'] + 4, beat): add(b, kick())
for b in np.arange(T['cool'] + beat / 2, T['fleet'] + 4, beat / 2): add(b, hat(int(round((b - T['cool']) / (beat / 2))) % 4 == 0))
t = T['cool']; k = 0
while t < T['numbers']:
    add(t, pad([Am, C, G, D][k % 4], beat * 4 + .6, 1.0, .06)); t += beat * 4; k += 1
arp([110, 164.8, 220, 261.6, 329.6, 261.6, 220, 164.8], T['cool'] + .2, T['numbers'], beat / 4, .05)
sidechain(T['cool'], T['fleet'] + 4, .5)
add(T['fleet'] + 1.4, hit(.8, 60))
# 7 · numbers: three hits, pad held
for j in range(3): add(T['numbers'] + .5 + j * 2.6, hit(.7 - j * .1, 58))
add(T['numbers'], pad([55, 110, 164.8], T['trust'] - T['numbers'] + 1, .3, .05))
# 8 · trust: near silence, one tone
add(T['trust'] + .3, pad([110, 164.8], 5.5, .3, .04))
# 9 · close: the ask
add(T['close'] - 1.2, riser(1.2)); add(T['close'], hit(1.0, 52))
add(T['close'] + .2, pad(Am, 6.5, .8, .06)); add(T['close'] + 6.7, pad(F, 5.0, .6, .05))
for j in range(3): add(T['close'] + 1.2 + j * .14, tick(1800 + j * 300, .14))
# master: gentle drive, soft clip, fade out
out = np.tanh(out * 1.25) * 0.92
fade = np.ones(n); fo = int(sr * 3.0); fade[-fo:] = np.linspace(1, 0, fo); out *= fade
w = wave.open(sys.argv[1] if len(sys.argv) > 1 else "soundtrack3.wav", "wb"); w.setnchannels(1); w.setsampwidth(2); w.setframerate(sr)
w.writeframes((out * 32767).astype(np.int16).tobytes()); w.close(); print("score ok, peak", round(float(np.abs(out).max()), 2))
