# Targets nobody uses day to day. None of them is the repository's test or generate command.
.PHONY: bench test-legacy docs release

bench:
	BENCH_ITERATIONS=100 python3 bench/run.py

test-legacy:
	LEGACY_DB=sqlite python3 -m pytest legacy

docs:
	mkdocs build --strict

release:
	git tag v$(VERSION)
	git push --tags
