-include .env
export

# pipefail so a failing command is not masked by the `tee` that logs it.
SHELL := bash
.SHELLFLAGS := -eu -o pipefail -c

# Every verification target keeps its full output here for an agent to read back.
LOGS := .tmp

.PHONY: check test lint format fix build run up attach clean \
        test-src test-web lint-src lint-web format-src format-web

# Everything, in order: test, then lint, then format.
check: test lint format

# Each of the three covers both sides; -src and -web run one side alone.
test lint format: %: %-src %-web

test-src: | $(LOGS)
	cargo test 2>&1 | tee $(LOGS)/$@.log
test-web: | $(LOGS)
	cd web && aube run test 2>&1 | tee ../$(LOGS)/$@.log

lint-src: | $(LOGS)
	cargo clippy --all-targets -- -D warnings 2>&1 | tee $(LOGS)/$@.log
lint-web: | $(LOGS)
	cd web && aube run lint 2>&1 | tee ../$(LOGS)/$@.log

format-src: | $(LOGS)
	cargo fmt --check 2>&1 | tee $(LOGS)/$@.log
format-web: | $(LOGS)
	cd web && aube run format 2>&1 | tee ../$(LOGS)/$@.log

# The write half of `format`. Not part of `check`, which must not edit the tree.
fix:
	cargo fmt
	cd web && aube run fix

build:
	cargo build --release
	cd web && aube ci && aube run build

# Just this index page, unsupervised. `make up` is the whole arrangement.
run: build
	cargo run --release

# Starts everything process-compose.yaml enables: this index page and every dev
# server not marked `disabled: true`. A unix socket instead of process-compose's
# default TCP 8080, so a second instance on this host — another repo's, or a
# long-running one — cannot collide with ours.
PC := process-compose --use-uds -u $(CURDIR)/$(LOGS)/process-compose.sock

up: build | $(LOGS)
	$(PC) up

# Talk to a detached `up` over the same socket.
attach: | $(LOGS)
	$(PC) attach

$(LOGS):
	mkdir -p $@

clean:
	cargo clean
	rm -rf web/dist web/node_modules $(LOGS)/*.log
