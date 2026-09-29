.PHONY: gen test ci lint

gen:
	python3 tools/gen.py

test:
	ACME_ENV=test python3 -m unittest discover -s tests

ci:
	@python3 tools/gen.py --check
	ACME_ENV=test python3 -m unittest discover -s tests

lint:
	python3 -m py_compile acme/*.py acme/generated/*.py tests/*.py
