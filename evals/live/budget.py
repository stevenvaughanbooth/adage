"""Persistent admission accounting for paid sessions; missing costs never become zero."""
import math
import threading
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


class Budget:
    def __init__(self, state, total, per_session, lock=None, persist=lambda: None):
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
        for attempt in self.data["attempts"]:
            if attempt["status"] not in ("settled", "cancelled"):
                self.data["blocked_reason"] = "an earlier session has unresolved accounting; reconcile its actual cost first"
            elif known_cost(attempt.get("cost_usd")) is None:
                self.data["blocked_reason"] = "an earlier session has an invalid cost; reconcile it first"

    @property
    def spent(self):
        return sum(known_cost(a.get("cost_usd")) or 0.0 for a in self.data["attempts"])

    def save(self):
        try:
            self.persist()
        except BaseException:
            self.data["blocked_reason"] = "budget accounting could not be persisted; dispatch is blocked"
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
            if attempt["status"] in ("settled", "cancelled", "unknown"):
                return
            if attempt["status"] == "reserved":
                attempt.update(status="cancelled", cost_usd=0.0)
            else:
                cost = known_cost((result or {}).get("total_cost_usd"))
                attempt.update(status="settled" if cost is not None else "unknown", cost_usd=cost)
                if cost is None:
                    self.data["blocked_reason"] = "a dispatched session has no valid terminal total_cost_usd"
                elif cost > attempt["cap_usd"] + 1e-9 or self.spent > self.total + 1e-9:
                    self.data["blocked_reason"] = "a reported cost exceeded its reserved cap; stop and reconcile"
            self.save()
