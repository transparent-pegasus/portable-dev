-include .env
export

.PHONY: build test check run up clean

build:
	cargo build --release
	cd web && npm ci && npm run build

test:
	cargo test
	cd web && npm test

check:
	cargo clippy --all-targets -- -D warnings
	cd web && npm run check

# Serves web/dist and /api/servers on $(BIND_ADDR):$(PORT).
run: build
	cargo run --release

# Starts the dev servers in process-compose.yaml.
up:
	process-compose up

clean:
	cargo clean
	rm -rf web/dist web/node_modules
