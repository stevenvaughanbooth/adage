# Development tasks for quayside.
PYTHON ?= python3
SCHEMAS := $(wildcard schema/*.json)

.PHONY: test gen fmt

test:
	DJANGO_SETTINGS_MODULE=quayside.settings.test $(PYTHON) -m pytest -q

gen: quayside/models_gen.py

quayside/models_gen.py: $(SCHEMAS) tools/gen.py
	$(PYTHON) tools/gen.py --out $@

fmt:
	black quayside tests
