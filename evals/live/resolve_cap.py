"""Preview/apply one explicitly human-authorized full-cap charge; never dispatch a model."""
import argparse
import hashlib
import json
import os
from pathlib import Path

from evals.live.budget import Budget, execution_lock, known_cost


def digest(data):
    return hashlib.sha256(data).hexdigest()


def durable_write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".cap-charge.tmp")
    with temporary.open("xb") as handle:
        handle.write(data)
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, path)
    descriptor = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def resolve_file(state_path, authorization_path, total, per_session, apply=False):
    """The authorization document pins the stopped run, full cap, and original ledger hash."""
    state_path, authorization_path = Path(state_path).resolve(), Path(authorization_path).resolve()
    raw_authorization = authorization_path.read_bytes()
    evidence = json.loads(raw_authorization)
    scope = evidence["scope"]
    cap = known_cost(scope.get("original_cap_usd"))
    if (evidence.get("schema_version") != 1 or cap is None or cap <= 0
        or known_cost(scope.get("charged_usd")) != cap or scope.get("actual_cost_usd") is not None
        or not evidence.get("approved_question") or not evidence.get("user_reply")):
        raise ValueError("explicit full-cap human authorization evidence is required")
    run = scope["run"]
    authorization = dict(kind="human_cap_charge", approved=True, run=run, cap_usd=cap,
        statement=evidence["user_reply"], source=str(authorization_path) + "#sha256=" + digest(raw_authorization),
        approved_question=evidence["approved_question"])
    with execution_lock(state_path.parent):
        before = state_path.read_bytes()
        if digest(before) != evidence["original_paid_state_sha256"]:
            raise ValueError("ledger changed since the authorized review; do not reuse stale authorization")
        state = json.loads(before)
        if state.get("runs", {}).get(run, {}).get("status") not in ("failed", "completed", "complete"):
            raise ValueError("the corresponding run is not terminal; refuse to resolve a live attempt")
        def persist():
            if not apply:
                return
            backup = state_path.parent / "accounting-resolutions" / (digest(before) + ".before.json")
            if backup.exists():
                if backup.read_bytes() != before:
                    raise ValueError("accounting backup collision")
            else:
                durable_write(backup, before)
            durable_write(state_path, (json.dumps(state, indent=2, sort_keys=True) + "\n").encode())
        budget = Budget(state, total, per_session, persist=persist)
        record = budget.resolve_unknown_at_cap(run, authorization)
        return dict(applied=apply, attempt=record, measured_spent_usd=budget.measured_spent,
                    conservative_charges_usd=budget.conservative_charges, admission_spent_usd=budget.spent,
                    blocked_reason=budget.data.get("blocked_reason"), before_sha256=digest(before),
                    actual_cost_usd=None, model_calls=0)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("state", type=Path)
    parser.add_argument("authorization", type=Path)
    parser.add_argument("--total-usd", required=True, type=float)
    parser.add_argument("--session-cap-usd", required=True, type=float)
    parser.add_argument("--apply", action="store_true", help="persist the reviewed cap charge; default only previews")
    args = parser.parse_args()
    print(json.dumps(resolve_file(args.state,args.authorization,args.total_usd,args.session_cap_usd,args.apply), indent=2))
