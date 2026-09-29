"""Pooled ablation table (markdown): per config x mode, all four repos pooled (task-clustered bootstrap,
90% percentile CI, 2000 draws), plus vue-core alone."""
import glob, json, os, random, math

S = os.path.dirname(os.path.abspath(__file__))
REPOS = ['click', 'attrs', 'marshmallow', 'vue-core']


def wilson_lb(k, n, z=1.6448536):
    if n == 0:
        return 0.0
    p = k / n
    c = p + z * z / (2 * n)
    m = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n))
    return max(0.0, (c - m) / (1 + z * z / n))


def boot(rows):
    by = {}
    for r in rows:
        by.setdefault(r['task'], []).append(r['net'])
    tasks = list(by.values())
    mean = sum(r['net'] for r in rows) / len(rows)
    rng = random.Random(7)
    bs = []
    for _ in range(2000):
        s = n = 0
        for _ in tasks:
            v = tasks[rng.randrange(len(tasks))]
            s += sum(v); n += len(v)
        bs.append(s / n)
    bs.sort()
    return mean, bs[99], bs[1899]


def load(cfg, m, repos):
    rows, dirs, useful, harm, calls = [], 0, 0, 0, 0
    for r in repos:
        f = f'{S}/abl/{cfg}-{r}-{m}.json'
        if not os.path.exists(f) or os.path.getsize(f) == 0:
            return None
        d = json.load(open(f))
        for x in d['sessions']:
            rows.append({'task': r + ':' + x['task'], 'net': x['net']})
        s = d['summary']
        dirs += s['directives']; useful += s['useful']; harm += s['harmful']; calls += s['calls']
    return rows, dirs, useful, harm, calls


print('| Embedder | Case store | Mode | Scope | Ŝ calls/session [90% CI] | Directives | Useful (Wilson90 LB) | Harmful |')
print('| --- | --- | --- | --- | --- | ---: | --- | ---: |')
for e in ['hashed', 'bundled', 'potion-code-16m-v2']:
    for c in ['off', 'on', 'ungated']:
        for m in ['post-hoc', 'pre-emptive']:
            for scope, repos in [('all 4 (82 sessions)', REPOS), ('vue-core (16)', ['vue-core'])]:
                x = load(f'{e}-{c}', m, repos)
                if not x:
                    continue
                rows, d, u, h, calls = x
                mean, lo, hi = boot(rows)
                print(f'| {e} | {c} | {m} | {scope} | **{mean:+.2f}** [{lo:+.2f}, {hi:+.2f}] | {d} | {u} ({wilson_lb(u, d):.2f}) | {h} |')
