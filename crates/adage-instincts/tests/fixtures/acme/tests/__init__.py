import os

if os.environ.get("ACME_ENV") != "test":
    raise RuntimeError("ACME_ENV is not set — run tests with 'make test'")
