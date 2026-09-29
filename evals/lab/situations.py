"""Decision points and their situations.

A decision point comes after the prompt and after each tool batch. Its situation is what the agent
knew then: the prompt, the last k actions (normalised), the error lines of recent failures, and the
outcome so far (edited yet, tests run, last test failing, failures, step count). Two texts are
embedded (prompt, recent history) and a small structural record is kept for the structural term.
"""

from __future__ import annotations

from dataclasses import dataclass

from traces import Session

TEST_WORDS = ("pytest", "npm test", "go test", "vitest", "jest", "unittest")


@dataclass
class Situation:
    session: Session
    b: int  # batch index about to be issued
    i: int  # index of the first step of batch b in session.steps
    prompt_text: str
    hist_text: str
    last_keys: tuple  # keys of the last k steps, most recent last
    flags: tuple  # (edited, tested, last_test_failed, any_failure, phase bucket)


def is_test(a) -> bool:
    return a.tool == "Bash" and any(w in a.key for w in TEST_WORDS)


def situations(s: Session, k: int = 5) -> list[Situation]:
    out = []
    starts = {}
    for i, st in enumerate(s.steps):
        starts.setdefault(st.batch, i)
    prompt = " ".join(s.prompt.split())[:600]
    for b in sorted(starts):
        i = starts[b]
        hist = s.steps[:i]
        last = hist[-k:]
        edited = any(h.action.edit for h in hist)
        tests = [h for h in hist if is_test(h.action)]
        tested = bool(tests)
        last_test_failed = bool(tests) and tests[-1].failed
        fails = [h for h in hist if h.failed]
        phase = 0 if i < 3 else 1 if i < 10 else 2 if i < 25 else 3
        errs = [h.error for h in fails[-2:] if h.error]
        state = (f"step {i}; " + ("edited; " if edited else "not edited yet; ")
                 + ("tests failing; " if last_test_failed else "tests passing; " if tested else "no tests run; ")
                 + (f"{len(fails)} failures" if fails else "no failures"))
        hist_text = " | ".join(h.action.text + (" [failed]" if h.failed else "") for h in last) or "start"
        if errs:
            hist_text += " | errors: " + " ; ".join(errs)
        hist_text += " | " + state
        out.append(Situation(s, b, i, prompt, hist_text, tuple(h.action.key for h in last),
                             (edited, tested, last_test_failed, bool(fails), phase)))
    return out


def struct_sim(a: Situation, b: Situation) -> float:
    s = 0.0
    if a.last_keys and b.last_keys and a.last_keys[-1] == b.last_keys[-1]:
        s += 0.4
    ka, kb = set(a.last_keys), set(b.last_keys)
    if ka or kb:
        s += 0.3 * len(ka & kb) / len(ka | kb)
    elif not a.last_keys and not b.last_keys:
        s += 0.7
    s += 0.3 * sum(x == y for x, y in zip(a.flags, b.flags)) / len(a.flags)
    return s
