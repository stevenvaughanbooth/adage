#!/usr/bin/env python3
"""External-parent migration checks. No model calls; no candidate programs on the host.

The private contract owns SQL probes and expected JSON/SQLSTATE. Candidate SQL runs as
an unprivileged database owner inside a bounded, offline, disposable PostgreSQL container.
This is a database-behavior component, not full Go/API coverage or launch admission.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time
import uuid

PG_IMAGE = "sha256:57c72fd2a128e416c7fcc499958864df5301e940bca0a56f58fddf30ffc07777"
POSTGRES_PROFILES = {
    "16.14": (PG_IMAGE, 70),
    "13.11": ("sha256:5c265bf1fd307dfb3ff3894c2ff6d4c3d98c777142c6fa4e04519fe39f615b2d", 999),
}
MAX_SQL_BYTES = 120_000
MIGRATIONS = Path("coderd/database/migrations")
DUMP = Path("coderd/database/dump.sql")


def sql_file(root, relative):
    root, relative = Path(root), Path(relative)
    if relative.is_absolute() or ".." in relative.parts:
        raise ValueError("SQL path escapes source")
    path = root
    for part in ("", *relative.parts):
        path /= part
        if path.is_symlink():
            raise ValueError("linked SQL inputs are not supported")
    if not path.is_file():
        raise ValueError("SQL input must be a regular file")
    with path.open("rb") as stream:
        body = stream.read(MAX_SQL_BYTES + 1)
    if len(body) > MAX_SQL_BYTES or b"\0" in body:
        raise ValueError("SQL input exceeds limit or contains NUL")
    return body.decode("utf-8")


def snapshot(base, candidate):
    """Capture the bytes actually executed; reject modified historical migration files."""
    base, candidate = Path(base), Path(candidate)
    # Check the directory before globbing: a linked empty directory must also fail.
    for root in (base, candidate):
        path = root
        for part in ("", *MIGRATIONS.parts):
            path /= part
            if path.is_symlink():
                raise ValueError("linked migration directory")
    historical = {p.name: sql_file(base, MIGRATIONS / p.name)
                  for p in (base / MIGRATIONS).glob("*.sql")}
    for name, body in historical.items():
        if sql_file(candidate, MIGRATIONS / name) != body:
            raise ValueError("historical migration changed: " + name)
    names = sorted(p.name for p in (candidate / MIGRATIONS).glob("*.up.sql") if p.name not in historical)
    up = {name: sql_file(candidate, MIGRATIONS / name) for name in names}
    down = {name.replace(".up.sql", ".down.sql"):
            sql_file(candidate, MIGRATIONS / name.replace(".up.sql", ".down.sql")) for name in reversed(names)}
    return {"base_dump": sql_file(base, DUMP), "candidate_dump": sql_file(candidate, DUMP),
            "historical_migrations": historical, "up": up, "down": down}


def validate_contract(contract):
    if contract.get("schema") != 1 or not re.fullmatch(r"[0-9a-f]{40}", contract.get("base_commit", "")):
        raise ValueError("contract needs schema 1 and pinned base_commit")
    if not isinstance(contract.get("task_id"), str) or not contract["task_id"]:
        raise ValueError("contract needs task_id")
    for stage in ("upgrade", "fresh", "downgrade"):
        section = contract.get(stage)
        if not isinstance(section, dict) or not isinstance(section.get("checks"), list) or not section["checks"]:
            raise ValueError("each stage requires behavioral checks")
        for check in section["checks"]:
            if not isinstance(check.get("name"), str) or not isinstance(check.get("sql"), str):
                raise ValueError("checks need name and SQL")
            if ("expected" in check) == ("sqlstate" in check):
                raise ValueError("check needs exactly one expected JSON value or SQLSTATE")
            if "sqlstate" in check and not re.fullmatch(r"[0-9A-Z]{5}", check["sqlstate"]):
                raise ValueError("invalid SQLSTATE")
    return contract


class Grader:
    def __init__(self, out, *, postgres_profile="16.14"):
        if postgres_profile not in POSTGRES_PROFILES:
            raise ValueError("unsupported pinned PostgreSQL profile")
        self.image, self.uid = POSTGRES_PROFILES[postgres_profile]
        self.out = Path(out)
        self.out.mkdir(parents=True, exist_ok=False, mode=0o700)
        (self.out / "logs").mkdir(mode=0o700)
        self.name = "adage-pg-grade-" + uuid.uuid4().hex[:12]
        self.commands = []

    def call(self, argv, label, *, input_text=None, timeout=45):
        number = len(self.commands)
        prefix = f"{number:03}-{label}"
        try:
            result = subprocess.run(argv, input=input_text, capture_output=True, text=True, timeout=timeout)
        except subprocess.TimeoutExpired as exc:
            result = subprocess.CompletedProcess(argv, 124,
                (exc.stdout or b"").decode() if isinstance(exc.stdout, bytes) else exc.stdout or "",
                (exc.stderr or b"").decode() if isinstance(exc.stderr, bytes) else exc.stderr or "")
        (self.out / "logs" / (prefix + ".stdout")).write_text(result.stdout)
        (self.out / "logs" / (prefix + ".stderr")).write_text(result.stderr)
        self.commands.append({"argv": argv, "label": label, "returncode": result.returncode, "log": prefix})
        (self.out / "commands.json").write_text(json.dumps(self.commands, indent=2) + "\n")
        return result

    def must(self, argv, label, **kwargs):
        result = self.call(argv, label, **kwargs)
        if result.returncode:
            raise RuntimeError(label + " failed; inspect retained logs")
        return result.stdout

    def sql(self, db, body, label, *, user="candidate"):
        # -c sends one server SQL string. A leading SQL comment also prevents psql
        # interpreting a candidate's leading backslash as a client shell command.
        return self.call(["docker", "exec", "-e", "PGOPTIONS=-c statement_timeout=5000 -c lock_timeout=2000",
            self.name, "psql", "-X", "-qAt", "-v", "ON_ERROR_STOP=1", "-v", "VERBOSITY=verbose",
            "-U", user, "-d", db, "-c", "-- server SQL only\n" + body], label)

    def start(self):
        self.must(["docker", "run", "--detach", "--name", self.name, "--network", "none",
            "--read-only", "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
            "--user", f"{self.uid}:{self.uid}", "--memory", "768m", "--cpus", "2", "--pids-limit", "128",
            "--tmpfs", f"/var/lib/postgresql/data:rw,nosuid,nodev,mode=0700,uid={self.uid},gid={self.uid},size=512m",
            "--tmpfs", f"/var/run/postgresql:rw,nosuid,nodev,mode=0755,uid={self.uid},gid={self.uid}",
            "-e", "POSTGRES_HOST_AUTH_METHOD=trust", "-e", "PGDATA=/var/lib/postgresql/data/db",
            "-e", "CODEX_SANDBOX=agent-preparation", self.image,
            "postgres", "-c", "listen_addresses=127.0.0.1"], "start")
        for _ in range(40):
            ready = self.call(["docker", "exec", self.name, "pg_isready", "-U", "postgres"], "ready")
            main = self.call(["docker", "exec", self.name, "cat", "/proc/1/comm"], "main")
            if ready.returncode == 0 and main.stdout.strip() == "postgres":
                break
            time.sleep(0.25)
        else:
            raise RuntimeError("PostgreSQL startup failed")
        result = self.sql("postgres", "CREATE ROLE candidate LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE;",
                          "create-role", user="postgres")
        if result.returncode:
            raise RuntimeError("unable to create restricted migration role")

    def database(self, name, dump):
        self.must(["docker", "exec", self.name, "createdb", "-U", "postgres", "-O", "candidate", name], "create-db")
        result = self.sql(name, dump, "schema")
        return result.returncode == 0

    def checks(self, db, checks, stage):
        rows = []
        for index, check in enumerate(checks):
            result = self.sql(db, check["sql"], f"{stage}-check-{index}")
            observed = None
            if "sqlstate" in check:
                # Match psql's diagnostic header, not arbitrary notice/message text.
                passed = result.returncode != 0 and bool(re.search(
                    r"^ERROR:\s+" + check["sqlstate"] + r":", result.stderr, re.M))
            else:
                decoded = False
                try:
                    observed = json.loads(result.stdout)
                    decoded = True
                except (ValueError, TypeError):
                    pass
                passed = (decoded and result.returncode == 0
                          and json.dumps(observed, sort_keys=True, allow_nan=False)
                          == json.dumps(check["expected"], sort_keys=True, allow_nan=False))
            rows.append({"stage": stage, "name": check["name"], "passed": passed,
                         "returncode": result.returncode, "observed": observed})
        return rows

    def grade(self, captured, contract):
        rows = []
        for stage, dump in (("upgrade", captured["base_dump"]), ("fresh", captured["candidate_dump"])):
            loaded = self.database(stage, dump)
            rows.append({"stage": stage, "name": "schema_load", "passed": loaded})
            if not loaded:
                continue
            section = contract[stage]
            if seed := section.get("seed"):
                # Only the private contract may supply privileged seed SQL. It can create
                # focused old-data fixtures without candidate-authored test scaffolding.
                result = self.sql(stage, seed, stage + "-seed", user="postgres")
                if result.returncode:
                    raise RuntimeError("trusted seed failed")
            if stage == "upgrade":
                rows.append({"stage": stage, "name": "new_migration_present", "passed": bool(captured["up"])})
                for i, sql in enumerate(captured["up"].values()):
                    result = self.sql(stage, contract.get("session_sql", "") + sql, f"up-{i}")
                    rows.append({"stage": stage, "name": f"migration_{i}", "passed": result.returncode == 0})
            if seed := section.get("probe_seed"):
                result = self.sql(stage, seed, stage + "-probe-seed", user="postgres")
                rows.append({"stage": stage, "name": "new_row_seed", "passed": result.returncode == 0})
            rows.extend(self.checks(stage, section["checks"], stage))
        if captured["down"]:
            for i, sql in enumerate(captured["down"].values()):
                result = self.sql("upgrade", contract.get("session_sql", "") + sql, f"down-{i}")
                rows.append({"stage": "downgrade", "name": f"migration_{i}", "passed": result.returncode == 0})
            rows.extend(self.checks("upgrade", contract["downgrade"]["checks"], "downgrade"))
        else:
            rows.append({"stage": "downgrade", "name": "down_migration_present", "passed": False})
        return rows

    def close(self):
        self.call(["docker", "logs", self.name], "server-log")
        self.call(["docker", "rm", "--force", self.name], "cleanup")


def run(base, candidate, contract_path, out, *, postgres_profile="16.14"):
    os.umask(0o077)
    contract_bytes = Path(contract_path).read_bytes()
    contract = validate_contract(json.loads(contract_bytes))
    head = subprocess.run(["git", "-C", str(base), "rev-parse", "HEAD"], capture_output=True, text=True, check=True)
    dirty = subprocess.run(["git", "-C", str(base), "status", "--porcelain"], capture_output=True, text=True, check=True)
    if head.stdout.strip() != contract["base_commit"] or dirty.stdout.strip():
        raise ValueError("trusted base must be clean and match the pinned commit")
    grader = Grader(out, postgres_profile=postgres_profile)
    (grader.out / "contract.json").write_bytes(contract_bytes)
    (grader.out / "grader.py").write_bytes(Path(__file__).read_bytes())
    result = {"task_id": contract["task_id"], "base_commit": contract["base_commit"],
              "quality_authority": "external_parent", "kind": "semantic", "scope": "database_behavior_only",
              "postgres_image": grader.image, "postgres_profile": postgres_profile, "candidate": str(Path(candidate).absolute()), "accepted": False,
              "remaining_omissions": contract.get("remaining_omissions", ["Go and API behavior not graded"]),
              "paid_model_calls": 0}
    started = False
    try:
        captured = snapshot(base, candidate)
        body = json.dumps(captured, sort_keys=True).encode()
        (grader.out / "sql-inputs.json").write_bytes(body)
        result["sql_inputs_sha256"] = hashlib.sha256(body).hexdigest()
        started = True
        grader.start()
        result["checks"] = grader.grade(captured, contract)
        result["accepted"] = all(row["passed"] for row in result["checks"])
        result["status"] = "completed"
    except (ValueError, OSError) as exc:
        result.update(status="invalid_candidate", error=str(exc))
    except Exception as exc:
        result.update(status="grader_error", error=str(exc))
    finally:
        if started:
            grader.close()
        (grader.out / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("base", "candidate", "contract", "out"):
        parser.add_argument("--" + name, required=True, type=Path)
    parser.add_argument("--postgres-profile", choices=sorted(POSTGRES_PROFILES), default="16.14")
    args = parser.parse_args()
    result = run(args.base, args.candidate, args.contract, args.out, postgres_profile=args.postgres_profile)
    print(json.dumps({"accepted": result["accepted"], "status": result["status"], "out": str(args.out)}))
    return 0 if result["status"] == "completed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
