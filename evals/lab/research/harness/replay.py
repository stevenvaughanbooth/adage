"""Counterfactual replay: drive a policy through a held-out session's hook events and score each
directive it gives, as `adage eval replay-savings` does (`crates/adage-eval/src/savings_replay.rs`),
then price it with the cumulative-context cost model.

    def policy(sit: Situation) -> Directive | None: ...
    report = score(policy, corpus.held_out())
    print(report.headline())

Events (one call of `policy` each; at most one directive per event), in the order Claude Code
raises them:
  SessionStart        t = −1
  UserPromptSubmit    t = index of the last call before the prompt (−1 for the first)
  PreToolUse          t = k, the call about to run: `sit.call` has its input, its output is hidden
                      (`sit.hindsight` exposes whether it will fail only under score(hindsight=True),
                      which reproduces the Rust `--mode pre-emptive`; a hindsight result is flagged)
  PostToolUse         t = k, `sit.call` has its output; `sit.failed` is True when Claude Code raised
                      PostToolUseFailure (the harness said is_error, and the call was not rejected)
  PostToolBatch       t = the last call of the model turn (Adage's batch hook)
  Stop                t = the last call (only for sessions that ended with a Stop hook)
`sit.history` is every call before the event (inputs and outputs), `sit.state` a dict the policy
may keep per session. A directive given at PreToolUse is pre-emptive (it can replace call t).

Scoring (per directive d at t; Rust semantics, `evals/replay/README.md`):
  t*          first t' > t with a_t' ∈ Sat(d);  useful ⇔ t* exists
  S(d)        |{k ∈ (t, t*) : a_k ∈ W(d)}| + [pre ∧ a_t ∉ Sat(d) ∧ a_t failed]
  superseded  given inside an earlier useful directive's window [t, t*): not scored, not costed
  harmful     not useful, session passed, a contradicting action after t, and d's own never
  C(d)        tokens(d) · cache-read price · API calls left ÷ $ per call      (Rust parity, calls)
and the target metrics (headline):
  calls       p_follow · S − C − p_follow · [scored ∧ ¬useful] · misfire_calls
  $           p_follow · $removal(W calls in the window) − $injection(d) − p_follow · [scored ∧ ¬useful] · misfire_calls · $/call
  pass rate   worst-case bound: −p_follow · [session has a harmful directive]  (replay cannot see gains)
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import Callable

from . import costmodel as CM
from . import features as F
from . import stats as ST
from .traces import Call, Session

LEARNED_FOOTER = "Learned by Adage from this repository's earlier agent sessions; not requested by the user."


# ---------------------------------------------------------------------------------------------
# Directives and their classes.
# ---------------------------------------------------------------------------------------------

@dataclass
class Directive:
    """What a policy says at an event.

    kind:
      "run"         run this command (`command`). Scored: a test-runner command → Runner class
                    (calibrated against live U11); any other command → Command class (Sat: a
                    later call starting with its first two words that did not fail; W: failed calls).
      "test_files"  run these test files (`files`): Sat a test run naming one that ran tests; W
                    test-discovery searches (the co-change oracle's class; uncalibrated).
      "read_files"  these files are what you need (`files`): Sat a Read/Edit of one of them; W
                    searches and listings in the window (Grep/Glob/find/grep/ls/rg). UNCALIBRATED.
      "advice"      free text: nothing in a trace can show it satisfied; costed, never credited.
      "custom"      `sat(act, call) -> bool` and `waste(act, call) -> bool` supplied by the policy.
                    Flagged in the report: a hand-written measurement a reviewer must check.
    text: the directive's body as delivered (the Adage wrapper is added for its token count unless
    `wrapped=True`); tokens = ⌈chars / 4⌉ as in Rust.
    """
    kind: str
    text: str = ""
    command: str = ""
    files: tuple = ()
    route: str = ""
    sat: Callable | None = None
    waste: Callable | None = None
    contradicts: Callable | None = None
    wrapped: bool = False
    p_follow: float | None = None  # overrides the score() assumption for this directive

    def tokens(self) -> int:
        t = self.text or self.command or ", ".join(self.files)
        if not self.wrapped:
            t = (f'<adage-reflex track="repo/learned@1.0.0" route="{self.route or "r"}" strength="prime" '
                 f'kind="{self.kind}">\n{t}\n{LEARNED_FOOTER}\n</adage-reflex>')
        return CM.tokens_of(t)


def same_file(target: str, file: str) -> bool:
    if target.startswith("pkg:"):
        pkg = target[4:]
        d = file.rsplit("/", 1)[0] if "/" in file else ""
        if pkg.endswith("..."):
            pre = pkg[:-3].rstrip("/")
            return pre == "" or d == pre or d.startswith(pre + "/")
        return d == pkg
    return (target == file or file.endswith("/" + target) or target.endswith("/" + file)
            or ("/" not in target and len(target) >= 4 and F.base(file).find(target) >= 0))


SEARCH_TOOLS = ("Grep", "Glob")


class Klass:
    """Sat / W / contradiction for one directive (Rust `Class`, plus the harness's extra kinds)."""

    def __init__(self, d: Directive, rule: str):
        self.d, self.rule, self.scored, self.name = d, rule, True, d.kind
        self.runner = self.head = None
        self.files = tuple(d.files)
        if d.kind == "run":
            inv = F.invocation(d.command)
            if inv:
                self.name, self.runner = "runner", inv[0]
            else:
                head = [w for w in F.words(d.command) if not w.startswith("{")][:2]
                if head:
                    self.name, self.head = "command", " ".join(head)
                else:
                    self.name, self.scored = "unscored", False
        elif d.kind in ("advice",):
            self.scored = False
        elif d.kind == "custom":
            self.scored = d.sat is not None

    def sat(self, a: F.Act, c: Call) -> bool:
        n = self.name
        if n == "runner":
            r = a.run
            return r is not None and r.runner == self.runner.runner and a.executed and (
                self.rule == "goal" or r.interp == self.runner.interp)
        if n == "command":
            return (not a.failed) and any(s == self.head or s.startswith(self.head + " ") for s in F.segments(a.cmd))
        if n == "test_files":
            return a.run is not None and a.executed and any(same_file(t, f) for t in a.targets for f in self.files)
        if n == "read_files":
            if a.tool in ("Read", "Edit", "Write", "MultiEdit"):
                p = c.input.get("file_path") if isinstance(c.input, dict) else None
                return isinstance(p, str) and any(p.endswith(f) or f.endswith(p) for f in self.files)
            return False
        if n == "custom":
            return bool(self.d.sat(a, c))
        return False

    def waste(self, a: F.Act, c: Call) -> bool:
        n = self.name
        if n == "runner":
            return a.env_friction
        if n == "command":
            return a.failed
        if n == "test_files":
            return a.discovery
        if n == "read_files":
            if a.tool in SEARCH_TOOLS:
                return True
            if a.tool == "Bash" and a.run is None:
                segs = F.segments(a.cmd)
                w = F.words(segs[0]) if segs else []
                return bool(w) and F.base(w[0]) in ("find", "grep", "rg", "ls", "fd", "tree", "ag")
            return False
        if n == "custom" and self.d.waste is not None:
            return bool(self.d.waste(a, c))
        return False

    def contradicts(self, a: F.Act, c: Call) -> bool:
        n = self.name
        if n == "runner":
            return a.run is not None and a.run.runner != self.runner.runner and a.executed
        if n == "test_files":
            return (a.run is not None and a.executed and bool(a.targets)
                    and not any(same_file(t, f) for t in a.targets for f in self.files))
        if n == "custom" and self.d.contradicts is not None:
            return bool(self.d.contradicts(a, c))
        return False

    def own(self, a: F.Act, c: Call) -> bool:
        n = self.name
        if n == "runner":
            return a.run is not None and a.run.runner == self.runner.runner and a.executed
        if n == "test_files":
            return a.run is not None and a.executed and any(same_file(t, f) for t in a.targets for f in self.files)
        if n == "custom":
            return self.sat(a, c)
        return False


# ---------------------------------------------------------------------------------------------
# Situations: the events a policy sees.
# ---------------------------------------------------------------------------------------------

@dataclass
class Situation:
    event: str
    t: int
    session: "SessionView"
    call: Call | None = None
    act: F.Act | None = None  # the features of `call` (instrument; PostToolUse only)
    failed: bool = False
    prompt: str | None = None
    hindsight: dict | None = None
    state: dict = field(default_factory=dict)

    @property
    def history(self) -> list:
        """Calls strictly before this event's call (PreToolUse/PostToolUse) or up to t (others)."""
        k = self.t if self.event in ("PreToolUse", "PostToolUse") else self.t + 1
        return self.session._s.calls[: max(k, 0)]


class SessionView:
    """What a policy may know about the session it is in: nothing about the future."""

    def __init__(self, s: Session):
        self._s = s
        self.repo, self.slice, self.cwd, self.model, self.task = s.repo, s.slice, s.cwd, s.model, s.task
        self.prompt = s.prompt  # the first user prompt (the task statement)
        self.sid = s.sid


def events(s: Session, hindsight: bool = False):
    """(event, t, call index or None, prompt text or None, extra) in hook order."""
    yield ("SessionStart", -1, None, None)
    k = 0
    for kind, v in s.items:
        if kind == "prompt":
            yield ("UserPromptSubmit", k - 1, None, v)
            continue
        for i in v:
            yield ("PreToolUse", i, i, None)
            yield ("PostToolUse", i, i, None)
        k += len(v)
        yield ("PostToolBatch", k - 1, None, None)
    if s.stopped:
        yield ("Stop", k - 1, None, None)


@dataclass
class Given:
    d: Directive
    t: int
    event: str
    pre: bool


def directives(policy, s: Session, hindsight: bool = False) -> list[Given]:
    view, state, out = SessionView(s), {}, []
    want = getattr(policy, "events", None)
    for ev, t, ci, prompt in events(s, hindsight):
        if want is not None and ev not in want:
            continue
        call = act = None
        failed = False
        hs = None
        if ci is not None:
            c = s.calls[ci]
            if ev == "PreToolUse":
                call = Call(c.tool, c.input, "", False, None, 0)
                if hindsight:
                    a = s.acts[ci]
                    hs = {"will_fail": a.failed and not a.rejected, "output": c.output, "is_error": c.is_error}
            else:
                call, act = c, s.acts[ci]
                failed = c.is_error and not s.acts[ci].rejected
        sit = Situation(ev, t, view, call, act, failed, prompt, hs, state)
        d = policy(sit)
        if d is not None:
            out.append(Given(d, t, "PostToolUseFailure" if (ev == "PostToolUse" and failed) else ev, ev == "PreToolUse"))
    return out


# ---------------------------------------------------------------------------------------------
# Scoring.
# ---------------------------------------------------------------------------------------------

@dataclass
class Scored:
    route: str
    kind: str
    klass: str
    t: int
    event: str
    pre: bool
    tokens: int
    t_star: int | None
    useful: bool
    harmful: bool
    superseded: bool
    saved: float  # S, calls
    cost: float  # C, calls (Rust parity)
    saved_idx: tuple
    usd_saved: float
    usd_cost: float
    tok_saved: float
    tok_cost: float
    p_follow: float
    misfire: bool
    custom: bool


@dataclass
class Row:
    sid: str
    run: str
    slice: str
    repo: str
    task: str
    arm: str
    model: str
    passed: bool | None
    calls: int
    usd: float  # cost model total (baseline $ of the session)
    usd_harness: float | None
    ctx_tokens: float
    dead_end: bool
    directives: int
    useful: int
    harmful: int
    superseded: int
    saved: float  # Σ S (raw)
    cost: float  # Σ C (calls)
    net_raw: float  # Σ S − C : the Rust Ŝ contribution
    net_calls: float  # headline: with p_follow and misfires
    net_usd: float  # headline $
    net_tokens: float
    net_usd_rust: float  # Rust: net_raw × harness $/call
    harm_session: bool
    scored: list

    @property
    def cluster(self):
        return f"{self.slice}/{self.task}"


def score_session(policy, s: Session, sat: str = "goal", p_follow: float = 1.0, misfire_calls: float = 2.0,
                  hindsight: bool = False) -> Row:
    acts, calls, n = s.acts, s.calls, s.n_calls
    cm = CM.of(s)
    usd_call_h = (s.cost_usd or 0.0) / max(n, 1)
    # Rust C(d): cache-read price of the session's model, API calls per tool call from the transcript.
    cache_read = CM.rates(s.model)[3]
    api_per_call = (len(s.responses) / max(n, 1)) if s.responses else 1.0
    out, windows = [], []
    for g in directives(policy, s, hindsight):
        k = Klass(g.d, sat)
        frm = max(g.t + 1, 0)
        t_star = next((j for j in range(frm, n) if k.sat(acts[j], calls[j])), None)
        superseded = any(a <= g.t < b for a, b in windows)
        useful = t_star is not None
        harmful = (not useful and s.passed is True
                   and any(k.contradicts(acts[j], calls[j]) for j in range(min(frm, n), n))
                   and not any(k.own(acts[j], calls[j]) for j in range(min(frm, n), n)))
        idx = []
        if t_star is not None and not superseded:
            idx = [j for j in range(frm, t_star) if k.waste(acts[j], calls[j])]
            if g.pre and g.t >= 0 and not k.sat(acts[g.t], calls[g.t]) and acts[g.t].failed:
                idx.append(g.t)
            windows.append((g.t, t_star))
        tokens = g.d.tokens()
        rem_api = max(0, n - frm) * api_per_call
        cost = 0.0 if (superseded or usd_call_h <= 0) else tokens * cache_read / 1e6 * rem_api / usd_call_h
        usd_s, tok_s = cm.removal(idx)
        usd_c, tok_c = (0.0, 0.0) if superseded else cm.injection(tokens, g.t)
        pf = g.d.p_follow if g.d.p_follow is not None else p_follow
        out.append(Scored(g.d.route or g.d.kind, g.d.kind, k.name, g.t, g.event, g.pre, tokens, t_star, useful, harmful,
                          superseded, float(len(idx)), cost, tuple(idx), usd_s, usd_c, tok_s, tok_c, pf,
                          (k.scored and not useful and not superseded), g.d.kind == "custom"))
    live = [x for x in out if not x.superseded]
    upc = cm.usd_per_call()
    saved = sum(x.saved for x in live)
    cost = sum(x.cost for x in live)
    net_calls = sum(x.p_follow * x.saved - x.cost - (x.p_follow * misfire_calls if x.misfire else 0.0) for x in live)
    net_usd = sum(x.p_follow * x.usd_saved - x.usd_cost - (x.p_follow * misfire_calls * upc if x.misfire else 0.0)
                  for x in live)
    net_tok = sum(x.p_follow * x.tok_saved - x.tok_cost for x in live)
    return Row(s.sid, s.run, s.slice, s.repo, s.task, s.arm, s.model, s.passed, n, cm.total, s.cost_usd, cm.ctx_tokens,
               any(F.dead_end(a, c.output) for a, c in zip(acts, calls)), len(live), sum(x.useful for x in live),
               sum(x.harmful for x in live), len(out) - len(live), saved, cost, saved - cost, net_calls, net_usd,
               net_tok, (saved - cost) * usd_call_h, any(x.harmful for x in live), out)


@dataclass
class Report:
    name: str
    rows: list
    assumptions: dict
    level: float = 0.90

    # ---- summary -------------------------------------------------------------------------
    def _ci(self, f, rows=None):
        rows = self.rows if rows is None else rows
        m, ci = ST.clustered([f(r) for r in rows], [r.cluster for r in rows], level=self.level)
        return m, ci

    def summary(self, rows=None) -> dict:
        rows = self.rows if rows is None else rows
        d = sum(r.directives for r in rows)
        u = sum(r.useful for r in rows)
        h = sum(r.harmful for r in rows)
        calls = sum(r.calls for r in rows)
        n = len(rows)
        calls_m, calls_ci = self._ci(lambda r: r.net_calls, rows)
        usd_m, usd_ci = self._ci(lambda r: r.net_usd, rows)
        share, share_ci = ST.clustered_ratio([r.net_usd for r in rows], [r.usd for r in rows], [r.cluster for r in rows],
                                             level=self.level)
        cshare, cshare_ci = ST.clustered_ratio([r.net_calls for r in rows], [r.calls for r in rows],
                                               [r.cluster for r in rows], level=self.level)
        raw_m, raw_ci = ST.clustered([r.net_raw for r in rows], [r.task for r in rows], level=0.95)
        hs = sum(r.harm_session for r in rows)
        _, hlo, hhi = ST.wilson(hs, n, self.level)
        _, ulo, _ = ST.wilson(u, d, 0.90)
        pf = self.assumptions["p_follow"]
        base_usd = sum(r.usd for r in rows) / n if n else 0.0
        import hashlib
        test_set = hashlib.sha1(",".join(sorted(r.sid for r in rows)).encode()).hexdigest()[:12]
        return {
            "sessions": n, "tasks": len({r.cluster for r in rows}), "calls": calls, "test_set_id": test_set,
            "baseline": {"usd_per_task": base_usd, "calls_per_task": calls / n if n else 0.0},
            # --- the target metrics (headline) ---
            "calls_saved_per_task": calls_m, "calls_saved_ci": calls_ci,
            "calls_saved_share": cshare, "calls_saved_share_ci": cshare_ci,
            "usd_saved_per_task": usd_m, "usd_saved_ci": usd_ci,
            "usd_saved_share": share, "usd_saved_share_ci": share_ci,
            "tokens_saved_per_task": sum(r.net_tokens for r in rows) / n if n else 0.0,
            "pass_rate_effect": {"point": 0.0, "worst_case": -pf * hs / n if n else 0.0,
                                 "worst_case_ci": [-pf * hhi, -pf * hlo],
                                 "note": "replay cannot see pass-rate gains; the bound assumes every harmful directive, followed, flips a pass to a fail"},
            # --- proxies and gates ---
            "s_hat_rust": raw_m, "s_hat_rust_ci95": raw_ci,
            "directives": d, "useful": u, "harmful": h, "superseded": sum(r.superseded for r in rows),
            "useful_rate": u / d if d else 0.0, "useful_lb90": ulo, "harmful_rate": h / d if d else 0.0,
            "per_10_calls": 10 * d / calls if calls else 0.0,
            "gates": {"useful_lb90>=0.8": ulo >= 0.8 if d else None, "harmful<=1%": (h / d <= 0.01) if d else None,
                      "per_10_calls<=1": (10 * d / calls <= 1) if calls else None},
            "custom_classes": sum(x.custom for r in rows for x in r.scored),
            "uncalibrated_classes": sorted({x.klass for r in rows for x in r.scored if x.klass not in ("runner",)}),
        }

    def by(self, key) -> dict:
        groups = {}
        for r in self.rows:
            groups.setdefault(key(r), []).append(r)
        return {k: self.summary(v) for k, v in sorted(groups.items())}

    def by_route(self) -> dict:
        m = {}
        for r in self.rows:
            for x in r.scored:
                if x.superseded:
                    continue
                e = m.setdefault(x.route, {"directives": 0, "useful": 0, "harmful": 0, "saved": 0.0, "cost": 0.0})
                e["directives"] += 1
                e["useful"] += x.useful
                e["harmful"] += x.harmful
                e["saved"] += x.saved
                e["cost"] += x.cost
        return m

    def headline(self) -> str:
        s = self.summary()
        lv = int(self.level * 100)
        pr = s["pass_rate_effect"]
        flags = []
        if s["custom_classes"]:
            flags.append(f"{s['custom_classes']} directives scored with CUSTOM Sat/W")
        unc = [c for c in s["uncalibrated_classes"] if c not in ("unscored", "advice")]
        if unc:
            flags.append("uncalibrated classes: " + ", ".join(unc))
        if self.assumptions.get("hindsight"):
            flags.append("HINDSIGHT (PreToolUse saw the future failure)")
        return (
            f"{self.name}: {s['sessions']} sessions / {s['tasks']} tasks [test set {s['test_set_id']}] (baseline ${s['baseline']['usd_per_task']:.3f}, "
            f"{s['baseline']['calls_per_task']:.1f} calls per task)\n"
            f"  $ saved/task     {s['usd_saved_per_task']:+.4f}  [{lv}% CI {s['usd_saved_ci'][0]:+.4f}, {s['usd_saved_ci'][1]:+.4f}]"
            f"  = {100 * s['usd_saved_share']:+.1f}% of $ [{100 * s['usd_saved_share_ci'][0]:+.1f}, {100 * s['usd_saved_share_ci'][1]:+.1f}]\n"
            f"  calls saved/task {s['calls_saved_per_task']:+.3f}  [{lv}% CI {s['calls_saved_ci'][0]:+.3f}, {s['calls_saved_ci'][1]:+.3f}]"
            f"  = {100 * s['calls_saved_share']:+.1f}% of calls\n"
            f"  pass rate        not estimable by replay; worst case {100 * pr['worst_case']:+.1f} pts "
            f"[{100 * pr['worst_case_ci'][0]:+.1f}, {100 * pr['worst_case_ci'][1]:+.1f}]\n"
            f"  proxies: Ŝ(Rust) {s['s_hat_rust']:+.3f} [95% {s['s_hat_rust_ci95'][0]:+.3f}, {s['s_hat_rust_ci95'][1]:+.3f}], "
            f"{s['directives']} directives, useful {s['useful_rate']:.2f} (LB90 {s['useful_lb90']:.2f}), harmful {s['harmful_rate']:.2f}, "
            f"{s['per_10_calls']:.2f}/10 calls; assumptions p_follow={self.assumptions['p_follow']}, misfire={self.assumptions['misfire_calls']} calls"
            + ("\n  FLAGS: " + "; ".join(flags) if flags else ""))

    def to_json(self) -> dict:
        s = self.summary()
        return {"name": self.name, "assumptions": self.assumptions, "summary": s, "by_route": self.by_route(),
                "by_slice": {k: {kk: v[kk] for kk in ("sessions", "usd_saved_per_task", "usd_saved_ci", "usd_saved_share",
                                                      "calls_saved_per_task", "calls_saved_ci", "s_hat_rust", "directives")}
                             for k, v in self.by(lambda r: r.slice).items()}}


def score(policy, sessions, name: str | None = None, sat: str = "goal", p_follow: float = 1.0,
          misfire_calls: float = 2.0, hindsight: bool = False, level: float = 0.90) -> Report:
    """Replay `policy` over `sessions` (normally `corpus.held_out()`) and score it.

    p_follow: probability the agent follows a directive (1.0 is what live U11 showed, 41/41;
      report 0.5 as a sensitivity for any kind with no live follow measurement).
    misfire_calls: calls a followed directive that never reaches Sat costs (the learner's prior, 2).
    hindsight: expose `sit.hindsight` at PreToolUse (Rust --mode pre-emptive). Flagged in output.
    """
    rows = [score_session(policy, s, sat, p_follow, misfire_calls, hindsight)
            for s in sorted(sessions, key=lambda s: s.run)]
    return Report(name or getattr(policy, "name", getattr(policy, "__name__", "policy")), rows,
                  {"sat": sat, "p_follow": p_follow, "misfire_calls": misfire_calls, "hindsight": hindsight}, level)
