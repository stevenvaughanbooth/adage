"""Persistent admission accounting for paid sessions; missing costs never become zero."""
import math
import copy
import threading
from datetime import datetime, timezone
from contextlib import contextmanager
try:
    import fcntl
except ImportError:  # Keep read-only plans/reports available on non-POSIX hosts.
    fcntl = None


@contextmanager
def execution_lock(root):
    """One runner process owns this experiment's accounting and state at a time."""
    if fcntl is None:
        raise SystemExit("paid execution requires a host with POSIX file locking")
    root.mkdir(parents=True, exist_ok=True)
    with (root / ".runner.lock").open("a") as handle:
        try:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise SystemExit(f"another runner owns {root}; refusing overlapping execution") from None
        try:
            yield
        finally:
            fcntl.flock(handle, fcntl.LOCK_UN)


def known_cost(value):
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return None
    return float(value) if math.isfinite(value) and value >= 0 else None


UNRESOLVED = "an earlier session has unresolved accounting; reconcile its actual cost first"
MISSING_TERMINAL = "a dispatched session has no valid terminal total_cost_usd"
PERSIST_FAILED = "budget accounting could not be persisted; dispatch is blocked"
OVERRUN = "a reported cost exceeded its reserved cap; stop and reconcile"
UNKNOWN_REASONS = {UNRESOLVED, MISSING_TERMINAL}


def valid_cap_authorization(authorization, attempt):
    """Only a recorded human decision for this exact original reservation is acceptable."""
    return (isinstance(authorization, dict) and authorization.get("kind") == "human_cap_charge"
            and authorization.get("approved") is True and authorization.get("run") == attempt.get("run")
            and known_cost(authorization.get("cap_usd")) == known_cost(attempt.get("cap_usd"))
            and all(isinstance(authorization.get(key), str) and authorization[key].strip()
                    for key in ("statement", "source")))


def cap_charge_valid(attempt):
    cap = known_cost(attempt.get("cap_usd"))
    resolution = attempt.get("resolution", {})
    return (cap is not None and cap > 0 and attempt.get("cost_usd") is None
            and known_cost(attempt.get("charged_usd")) == cap and isinstance(resolution, dict)
            and resolution.get("prior_status") == "unknown" and isinstance(resolution.get("recorded_at"), str)
            and bool(resolution["recorded_at"].strip())
            and valid_cap_authorization(resolution.get("authorization"), attempt))


class Budget:
    def __init__(self, state, total, per_session, lock=None, persist=lambda: None):
        if known_cost(total) is None or total <= 0 or known_cost(per_session) is None or per_session <= 0:
            raise ValueError("budget limits must be finite positive amounts")
        self.total, self.per_session = total, per_session
        self.condition = threading.Condition(lock or threading.Lock())
        self.persist = persist
        if "accounting" not in state:
            attempts = []
            for key, run in state.get("runs", {}).items():
                if run.get("status") == "skipped":
                    continue
                cost = known_cost((run.get("metrics") or {}).get("cost_usd"))
                # Older runners silently wrote zero for a missing terminal cost. That zero
                # cannot safely establish a free session during migration.
                attempts.append({"run": key, "status": "settled" if cost and run.get("status") != "running"
                                 else "unknown", "cost_usd": cost if cost else None, "legacy": True})
            state["accounting"] = {"version": 1, "attempts": attempts, "blocked_reason": None}
        self.data = state["accounting"]
        if self.data.get("version") != 1:
            raise ValueError("unsupported budget accounting version")
        self._refresh_block()

    def _refresh_block(self, clear_resolved_unknown=False):
        # Preserve unrelated persistence, overrun and operator blocks, even when a separate
        # unknown attempt is resolved. Old ledgers carry text reasons, so clear only our two
        # exact unknown-accounting reasons, never an arbitrary nonempty block.
        previous = self.data.get("blocked_reason")
        if previous and previous not in UNKNOWN_REASONS:
            return
        reason = None
        for attempt in self.data["attempts"]:
            status = attempt.get("status")
            if status == "cap_charged":
                if not cap_charge_valid(attempt):
                    reason = "an earlier conservative cap charge is invalid; dispatch is blocked"
                    break
            elif status in ("settled", "cancelled"):
                cost = known_cost(attempt.get("cost_usd"))
                if cost is None:
                    reason = "an earlier session has an invalid cost; reconcile it first"
                    break
                cap = known_cost(attempt.get("cap_usd"))
                if cap is not None and cost > cap + 1e-9:
                    reason = OVERRUN
                    break
            else:
                reason = UNRESOLVED
        if self.spent > self.total + 1e-9:
            reason = OVERRUN
        if reason or clear_resolved_unknown:
            self.data["blocked_reason"] = reason

    @property
    def spent(self):
        """Admission spend: measured costs plus explicitly authorized conservative charges."""
        return self.measured_spent + self.conservative_charges

    @property
    def measured_spent(self):
        return sum(known_cost(a.get("cost_usd")) or 0.0 for a in self.data["attempts"])

    @property
    def conservative_charges(self):
        return sum(known_cost(a.get("charged_usd")) or 0.0 for a in self.data["attempts"]
                   if a.get("status") == "cap_charged")

    def resolve_unknown_at_cap(self, run, authorization):
        """Charge one stopped unknown attempt at its full original cap, never infer its bill.

        Authorization is supplied from an explicit human decision and stored verbatim. This
        cannot resolve an active/reserved attempt, choose a smaller charge, or erase a failure.
        Call under execution_lock for durable file-backed ledgers, as with normal dispatch.
        """
        with self.condition:
            matches = [a for a in self.data["attempts"] if a.get("run") == run]
            if len(matches) != 1:
                raise ValueError("cap resolution requires one unambiguous original attempt")
            attempt = matches[0]
            cap = known_cost(attempt.get("cap_usd"))
            if attempt.get("status") != "unknown" or attempt.get("cost_usd") is not None:
                raise ValueError("only a stopped unknown-cost attempt may be conservatively charged")
            if cap is None or cap <= 0 or not valid_cap_authorization(authorization, attempt):
                raise ValueError("explicit human authorization for the exact original positive cap is required")
            prior = copy.deepcopy(attempt)
            attempt.update(status="cap_charged", charged_usd=cap, resolution={
                "prior_status":"unknown", "recorded_at":datetime.now(timezone.utc).isoformat(),
                "authorization":copy.deepcopy(authorization),
                "actual_cost":"unknown; charged at the reserved cap for admission only"})
            self._refresh_block(clear_resolved_unknown=True)
            try:
                self.save()
            except BaseException:
                attempt.clear()
                attempt.update(prior)
                self.data["blocked_reason"] = PERSIST_FAILED
                raise
            return copy.deepcopy(attempt)

    def save(self):
        try:
            self.persist()
        except BaseException:
            self.data["blocked_reason"] = PERSIST_FAILED
            raise
        finally:
            self.condition.notify_all()

    def reserve(self, run):
        with self.condition:
            while not self.data["blocked_reason"]:
                pending = [a for a in self.data["attempts"] if a["status"] in ("reserved", "dispatched")]
                reserved = sum(a["cap_usd"] for a in pending)
                if self.spent + reserved + self.per_session <= self.total + 1e-9:
                    attempt = {"run": run, "status": "reserved", "cap_usd": self.per_session, "cost_usd": None}
                    self.data["attempts"].append(attempt)
                    self.save()  # durable before even starting preparation
                    return attempt, None
                if not pending:
                    return None, "remaining budget cannot reserve a full session cap"
                self.condition.wait()
            return None, self.data["blocked_reason"]

    def dispatch(self, attempt):
        with self.condition:
            # Unknown accounting from another worker cancels even a prepared reservation.
            if self.data["blocked_reason"]:
                return False
            attempt["status"] = "dispatched"
            self.save()  # a crash after this point is unknown, never assumed free
            return True

    def settle(self, attempt, result=None):
        with self.condition:
            if attempt["status"] in ("settled", "cancelled", "unknown", "cap_charged"):
                return
            if attempt["status"] == "reserved":
                attempt.update(status="cancelled", cost_usd=0.0)
            else:
                cost = known_cost((result or {}).get("total_cost_usd"))
                attempt.update(status="settled" if cost is not None else "unknown", cost_usd=cost)
                if cost is None:
                    if self.data.get("blocked_reason") in (None, *UNKNOWN_REASONS):
                        self.data["blocked_reason"] = MISSING_TERMINAL
                elif cost > attempt["cap_usd"] + 1e-9 or self.spent > self.total + 1e-9:
                    self.data["blocked_reason"] = OVERRUN
            self.save()
