"""Render results/*.json as the markdown tables used in README.md (results/tables.md)."""
from __future__ import annotations

import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
R = os.path.join(HERE, "results")


def f(x, d=2):
    return f"{x:.{d}f}" if isinstance(x, float) else str(x)


def main():
    m = json.load(open(os.path.join(R, "metrics.json")))
    out = []
    fx = m["facts"]
    out.append(f"Sessions {fx['sessions']}, tool calls {fx['calls']}, Bash calls {fx['bash']}, test runs {fx['test_runs']}: "
               f"test-run strings distinct {fx['test_distinct_share']:.0%} (seen once {fx['test_unique_share']:.0%}), "
               f"narrowed {fx['test_narrowed']:.0%}, piped {fx['test_piped']:.0%}.\n")
    out.append("| Method | Distinct labels | Labels covering 90% of calls | Labels in ≥ 2 tasks | Calls whose label is in ≥ 2 tasks | Labels in ≥ 2 repos |")
    out.append("|---|---:|---:|---:|---:|---:|")
    for k, v in m["methods"].items():
        s = v["static"]
        out.append(f"| {k} | {s['distinct']} | {s['cover90']} | {s['recur']:.0%} | {s['recur_calls']:.0%} | {s['xrepo']:.0%} |")
    out.append("")
    out.append("| Method | CV seen | CV acc | CV base | CV κ | CV ctx acc | CV ctx IG (bits) | T→E seen | T→E acc | T→E κ | T→E ctx acc | T→E ctx IG |")
    out.append("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
    for k, v in m["methods"].items():
        a, c, t, tc = v["cv5"], v["cv5_ctx"], v["train_eval"], v["train_eval_ctx"]
        out.append(f"| {k} | {a['seen']:.2f} | {a['acc']:.2f} | {a['base']:.2f} | {a['kappa']:.2f} | {c['acc']:.2f} | {c['ig']:.2f} | "
                   f"{t['seen']:.2f} | {t['acc']:.2f} | {t['kappa']:.2f} | {tc['acc']:.2f} | {tc['ig']:.2f} |")
    out.append("")
    cb = json.load(open(os.path.join(R, "compile_back.json")))
    keys = ["points", "compiled", "compiled_file_level", "safe", "agree_agent", "agree_agent_narrowed", "agree_map_only", "gold_hit", "gold_hit_map_only",
            "agent_env_wrong", "preempted", "lgg_bindable", "lgg_agree", "lgg_env_project", "emb_assigned", "emb_agree", "emb_env_project"]
    repos = [k for k in cb if k != "lgg_dominant_train_template"]
    out.append("| Measure | " + " | ".join(repos) + " |")
    out.append("|---|" + "---:|" * len(repos))
    for k in keys:
        out.append(f"| {k} | " + " | ".join(f(cb[r].get(k, ''), 2) for r in repos) + " |")
    out.append("")
    vp = os.path.join(R, "verify.json")
    if os.path.exists(vp):
        v = json.load(open(vp))
        out.append("| Repo | Directives executed | Ran as expected |")
        out.append("|---|---:|---:|")
        for r, x in v.items():
            out.append(f"| {r} | {x['checked']} | {x['ok']} |")
    txt = "\n".join(out) + "\n"
    open(os.path.join(R, "tables.md"), "w").write(txt)
    print(txt)


if __name__ == "__main__":
    main()
