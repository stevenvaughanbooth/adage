"""Read-only: do the held-out family names occur in any earlier session record? (transfer-v3 caveat)

Scans every native transcript, stream, prompt, final patch and learning-history copy under the given
authorities for the class names, task ids and snake_case forms of the authored families. The live
development study (cohorts/launch-gate-v5/dev-v8b) is skipped: its prompts are fixed to the
development task and it is not read before its own report. Prints JSON.
"""
import json
import os
import sys

NAMES = {
    "has-keys": ["HasKeys", "has-keys", "has_keys"],
    "iso-date": ["IsoDate", "iso-date", "iso_date"],
    "json-object": ["JsonObject", "json-object", "json_object"],
    "lat-long": ["LatLong", "lat-long", "lat_long"],
    "max-sum": ["MaxSum", "max-sum", "max_sum"],
}
FILES = {"native-transcript.jsonl", "stdout.jsonl", "stream-observations.jsonl", "prompt.txt", "final-tracked.patch"}
PRUNE = {"repo", ".venv", "private-score", "cache", "runtime", "bases", "adage-home", "source", "bin", ".git", "validation"}
SKIP = "cohorts/launch-gate-v5/dev-v8b"
# Marshmallow's own CHANGELOG names the removed `utils.from_iso_date`, `from_iso_datetime` and
# `to_iso_datetime`; agents read it in every session.
REPOSITORY_TEXT = ("iso_datetime", "from_iso_date")


def scan(authorities):
    scanned, hits, repository_text = 0, [], 0
    for authority in authorities:
        for directory, dirs, files in os.walk(authority):
            if SKIP in directory:
                dirs[:] = []
                continue
            dirs[:] = [d for d in dirs if d not in PRUNE]
            history = os.path.basename(directory) == "history"
            for name in files:
                if name not in FILES and not (history and name.endswith(".jsonl")):
                    continue
                path = os.path.join(directory, name)
                text = open(path, errors="replace").read()
                scanned += 1
                for known in REPOSITORY_TEXT:
                    repository_text += text.count(known)
                    text = text.replace(known, "")
                for family, needles in NAMES.items():
                    for needle in needles:
                        at = text.find(needle)
                        if at >= 0:
                            hits.append(dict(family=family, needle=needle, path=path,
                                             context=text[max(0, at - 80):at + 80]))
    return dict(authorities=authorities, skipped=SKIP, files_scanned=scanned, names=NAMES, hits=hits,
                excluded_repository_text=dict(strings=list(REPOSITORY_TEXT), occurrences=repository_text),
                families_seen=sorted({h["family"] for h in hits}))


if __name__ == "__main__":
    print(json.dumps(scan(sys.argv[1:]), indent=2))
