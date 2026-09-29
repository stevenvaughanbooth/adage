---
name: acme-billing
description: How to work in the acme billing repository. Use when changing billing code or schemas.
---

# Working in acme billing

Run the tests with `python3 -m unittest`.

After editing `schema/*.json`, always run `make gen` to regenerate `acme/generated/models.py`.

Look up work items such as W-2042 with `python3 scripts/wi-lookup W-2042` before you start.

Whenever you change `proto/*.proto`, run `make gen` as well.

Install the dependencies with `pip install -r requirements.txt`.

The `acme/` package holds the billing code.
