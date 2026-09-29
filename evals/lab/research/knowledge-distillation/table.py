import json
r = json.load(open((__import__("sys").argv[1] if len(__import__("sys").argv) > 1 else "results") + "/heldout.json"))
def f(d): return f"{d['mean']:.2f} [{d['ci90'][0]:.2f},{d['ci90'][1]:.2f}]"
def g(d): return f"{d['mean']:.1%} [{d['ci90'][0]:.1%},{d['ci90'][1]:.1%}]"
print("method | calls obviated | calls net | $ net/session | frac $")
for m, d in r["pooled"].items():
    print(f"{m:12s} | {f(d['calls'])} | {f(d['calls_net'])} | {d['usd']['mean']:.4f} [{d['usd']['ci90'][0]:.4f},{d['usd']['ci90'][1]:.4f}] | {g(d['frac_usd'])}")
for repo, dd in r["per_repo"].items():
    print(repo, " ".join(f"{m}={dd[m]['calls_net']['mean']:.2f}/{dd[m]['frac_usd']['mean']:.1%}" for m in dd if isinstance(dd[m], dict) and 'calls_net' in dd[m]), dd["card"].get("card_tokens"), dd["card"].get("n_facts"))
g_ = sorted(r["grid"].items(), key=lambda x: -x[1])
print(g_[:12])
