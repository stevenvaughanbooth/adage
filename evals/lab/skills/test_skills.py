"""Offline tests for the skill lab (no sessions, no model): python3 -m pytest evals/lab/skills -q"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from abstract import Action, Session, classify_bash  # noqa: E402
from align import Model, analyse  # noqa: E402
from compile import build  # noqa: E402
from procedure import parse, validate  # noqa: E402

ACME = HERE / "skills/acme-work-item/SKILL.md"
CLICK = HERE / "skills/click-bugfix/SKILL.md"


def test_acme_procedure_graph():
    g = parse(ACME).to_json()
    assert validate(g) == []
    roles = [s["role"] for s in g["steps"]]
    assert roles == ["lookup", "regenerate", "edit", "verify", "verify"]
    assert g["slots"]["id"]["example"] == "W-2042" and g["slots"]["id"]["pattern"] == r"^W-\d{4}$"
    s2, s4, s5 = g["steps"][1], g["steps"][3], g["steps"][4]
    assert s2["precondition"] == {"kind": "after_edit", "glob": "schema/*.json", "text": ""}
    assert [a["command"] for a in s4["avoid"]] == ["python3 -m unittest", "pytest"]
    assert all(a["instead"] == "make test" for a in s4["avoid"])
    assert s4["expect"]["contains"] == ["OK"]
    # "fails ... if you skipped `make gen`" is a mention, not an instruction.
    assert [c["template"] for c in s5["commands"]] == ["make ci"]
    assert s5["precondition"]["kind"] == "before_finish"


def test_click_slots_and_gaps():
    g = parse(CLICK).to_json()
    assert g["slots"]["module"]["source"] == "edit"
    assert g["slots"]["symbol"]["source"] == "judgment"
    c = build(g, None)
    why = {x["step"]: x["why"] for x in c["gaps"]}
    assert "stem" in why["s3"] and "chosen by the agent" in why["s2"]
    assert any(r["route"]["step"]["do"] == "avoid" for r in c["reflexes"])


def a(i, role, env="make", scope="repo", cmd="", path="", failed=False):
    return Action(i, "Bash" if cmd else "Edit", role, env, scope, cmd=cmd, path=path, failed=failed)


def test_alignment_absorbs_exploration_and_flags_deviations():
    g = parse(ACME).to_json()
    s = Session("x", "Pick up W-2042", "/r", [
        a(0, "search", "tool", "repo", cmd="ls -la"),
        a(1, "lookup", "system", "item", cmd="python3 scripts/wi-lookup W-2042"),
        a(2, "edit", "tool", "file", path="acme/billing.py"),
        a(3, "verify", "system", "repo", cmd="python3 -m unittest", failed=True),
        a(4, "verify", "make", "suite", cmd="make test"),
        a(5, "verify", "make", "repo", cmd="make ci"),
    ])
    al = Model(g).align(s)
    assert al[0] == (None, "")
    assert al[1][0] == 0 and al[1][1] == "deviation"  # the lookup, through another interpreter
    assert al[2][0] == 2  # the code change
    assert al[3] == (3, "avoided")  # the forbidden runner, aligned to the test step
    assert al[4] == (3, "exact") and al[5] == (4, "exact")
    r = analyse(g, [s])
    s4 = next(x for x in r["steps"] if x["id"] == "s4")
    assert s4["avoided_hits"] == 1 and s4["avoided_failed"] == 1


def test_bash_segments():
    assert classify_bash("cd /r && make test 2>&1 | tail -5", "/r")[:2] == ("verify", "make")
    assert classify_bash("ls -la acme/generated/", "/r")[0] == "search"
    assert classify_bash('git commit -m "$(cat <<EOF\nx\nEOF\n)"', "/r")[0] == "vcs"


if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_"):
            fn()
            print("ok", name)
