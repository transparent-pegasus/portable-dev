-include .env
export

# pipefail so a failing command is not masked by the `tee` that logs it.
SHELL := bash
.SHELLFLAGS := -eu -o pipefail -c

# Every verification target keeps its full output here for an agent to read back.
LOGS := .tmp

.PHONY: check test lint format fix build run up clean \
        test-src test-web lint-src lint-web format-src format-web

# Everything, in order: test, then lint, then format.
check: test lint format

# Each of the three covers both sides; -src and -web run one side alone.
test lint format: %: %-src %-web

test-src: | $(LOGS)
	cargo test 2>&1 | tee $(LOGS)/$@.log
test-web: | $(LOGS)
	cd web && npm test 2>&1 | tee ../$(LOGS)/$@.log

lint-src: | $(LOGS)
	cargo clippy --all-targets -- -D warnings 2>&1 | tee $(LOGS)/$@.log
lint-web: | $(LOGS)
	cd web && npm run lint 2>&1 | tee ../$(LOGS)/$@.log

format-src: | $(LOGS)
	cargo fmt --check 2>&1 | tee $(LOGS)/$@.log
format-web: | $(LOGS)
	cd web && npm run format 2>&1 | tee ../$(LOGS)/$@.log

# The write half of `format`. Not part of `check`, which must not edit the tree.
fix:
	cargo fmt
	cd web && npm run fix

build:
	cargo build --release
	cd web && npm ci && npm run build

# Serves web/dist and /api/servers on $(BIND_ADDR):$(PORT).
run: build
	cargo run --release

# Starts the dev servers in process-compose.yaml.
up:
	process-compose up

$(LOGS):
	mkdir -p $@

clean:
	cargo clean
	rm -rf web/dist web/node_modules $(LOGS)/*.log
