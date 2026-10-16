---
name: acme-work-item
description: Work a ticket (W-NNNN) in the acme-billing repository end to end. Use when a prompt names a work item or asks to change billing, customer or schema code.
---

# Working a work item in acme-billing

Work items live only in the internal tracker; a prompt usually names just the id, such as W-2042.

## Procedure

1. **Read the work item.** Run `scripts/wi-lookup <ID>` (for example `scripts/wi-lookup W-2042`).
   It prints the title, description, linked files and acceptance criteria. Read the linked files
   first; you rarely need to explore beyond them.
2. **If the item adds or changes a record field**, edit the schema in `schema/<record>.json`, never
   `acme/generated/models.py` (it is generated). After editing `schema/*.json`, run `make gen` to
   regenerate `acme/generated/models.py`.
3. **Make the code change** in the linked files under `acme/`.
4. **Run the tests with `make test`.** Never run `python3 -m unittest` or `pytest` directly instead of
   `make test`: `tests/__init__.py` refuses to run without `ACME_ENV` and fails with
   `RuntimeError: ACME_ENV is not set`, and pytest is not installed. Expect `OK` at the end.
5. **Before you finish, run `make ci`.** It checks that the generated models are fresh and runs the
   whole suite. It fails with `generated models are stale` if you skipped `make gen`.

## Notes

- Keep changes minimal and inside the linked files.
- Do not commit; leave the working tree for review.
