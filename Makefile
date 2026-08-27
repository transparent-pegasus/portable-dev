-include .env
# Only what the server and the renders below read. A bare `export` would hand
# every recipe — cargo, aube, build scripts, every dev server process-compose
# starts — the Cloudflare API token that also lives in .env.
export BIND_ADDR PORT PORT_RANGE COMPOSE_FILE WEB_DIR

# The whole covered range: what the Access destination and the nft rule cover.
# Blank PORT_RANGE falls back to this app's own port alone.
PORTS := $(or $(strip $(PORT_RANGE)),$(PORT))

# nftables ships in sbin, off a non-root PATH, and every nft call here needs
# root anyway — resolve it by path rather than asking whether the user sees it.
NFT = $(shell command -v nft 2>/dev/null || for p in /usr/sbin/nft /sbin/nft; do test -x $$p && echo $$p && break; done)

# The unit needs absolute paths, and iproute2 sits in sbin for the same reason.
IP = $(shell command -v ip 2>/dev/null || for p in /usr/sbin/ip /sbin/ip; do test -x $$p && echo $$p && break; done)

# Installed by `make install-net`. `firewall` and `bind-addr` skip their own work
# when it is running, so `make up` needs no sudo on a host that has it.
UNIT := portable-dev-net.service

# pipefail so a failing command is not masked by the `tee` that logs it.
SHELL := bash
.SHELLFLAGS := -eu -o pipefail -c

# Every verification target keeps its full output here for an agent to read back.
LOGS := .tmp

.PHONY: check test lint format fix build run up attach down clean bind-addr firewall ports setup install-net uninstall-net \
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

# --- the tunnel side: this repo owns it -------------------------------------

# A private-network route needs an address WARP can be routed to, and an alias
# on `lo` does not survive a reboot — so re-add it rather than relying on it.
bind-addr: ## add BIND_ADDR to lo if missing (needs sudo)
	@case "$$BIND_ADDR" in 0.0.0.0|::|::0) echo "BIND_ADDR=$$BIND_ADDR would listen on every interface — refusing"; exit 1;; esac
	@ip -4 -o addr show dev lo | grep -qFw "$$BIND_ADDR" \
	  || { echo "adding $$BIND_ADDR/32 to lo (sudo)"; sudo ip addr add "$$BIND_ADDR/32" dev lo; }

# Own table only. Never write into another repo's — theirs is flushed and
# rewritten on every one of their runs, so anything added there disappears.
firewall: ports | $(LOGS) ## drop BIND_ADDR:PORTS arriving off-loopback (needs sudo)
	@test -n "$(NFT)" || { echo "nft not found — install nftables"; exit 1; }
	@systemctl is-active --quiet $(UNIT) 2>/dev/null \
	  || { echo "installing nft drop rule for $(PORTS) (sudo)"; \
	       sed -e "s/@BIND_ADDR@/$$BIND_ADDR/" -e "s/@PORTS@/$(PORTS)/" deploy/portable-dev.nft > $(LOGS)/portable-dev.nft; \
	       sudo $(NFT) -f $(LOGS)/portable-dev.nft; }

# --- the same two pieces of state, kept across reboots -----------------------
# Opt-in and self-contained: `make up` never requires this, and re-adds both
# itself when it is absent. Installing only moves the sudo from every `up` to
# this one command. Re-run it after changing BIND_ADDR or PORT_RANGE.
install-net: ports | $(LOGS) ## re-add the alias and the drop rule at every boot (sudo once)
	@test -n "$(NFT)" -a -n "$(IP)" || { echo "nft or ip not found — install nftables and iproute2"; exit 1; }
	@sed -e "s/@BIND_ADDR@/$$BIND_ADDR/" -e "s/@PORTS@/$(PORTS)/" deploy/portable-dev.nft > $(LOGS)/portable-dev.nft
	@sed -e "s/@BIND_ADDR@/$$BIND_ADDR/" -e "s|@IP@|$(IP)|" -e "s|@NFT@|$(NFT)|" deploy/$(UNIT) > $(LOGS)/$(UNIT)
	sudo install -d -m 755 /etc/portable-dev
	sudo install -m 644 $(LOGS)/portable-dev.nft /etc/portable-dev/firewall.nft
	sudo install -m 644 $(LOGS)/$(UNIT) /etc/systemd/system/$(UNIT)
	sudo systemctl daemon-reload
	sudo systemctl enable --now $(UNIT)

# Stopping the unit runs its ExecStop, which drops our table and the alias. The
# next `make up` puts both back for the current boot, as it did before.
uninstall-net: ## stop re-adding them at boot (sudo)
	-sudo systemctl disable --now $(UNIT)
	sudo rm -f /etc/systemd/system/$(UNIT) /etc/portable-dev/firewall.nft
	-sudo rmdir /etc/portable-dev
	sudo systemctl daemon-reload

# A port outside the covered range is authorized by nothing and reachable from
# the LAN. The server refuses to start on one; catch it before the apply too.
ports: ## check every registered port falls inside PORT_RANGE
	@cargo run --release --quiet -- --check-ports

setup: ports ## terraform the cloudflare side, write .tunnel-token
	@for tool in terraform cloudflared curl jq; do command -v "$$tool" >/dev/null \
	  || { echo "$$tool not found on PATH"; exit 1; }; done
	@{ set -a && . ./.env && set +a; } \
	  && test -n "$$CLOUDFLARE_API_TOKEN" -a -n "$$CLOUDFLARE_ACCOUNT_ID" -a -n "$$USER_EMAIL" -a -n "$$TEAM_NAME" \
	  || { echo "Set CLOUDFLARE_API_TOKEN, CLOUDFLARE_ACCOUNT_ID, USER_EMAIL, TEAM_NAME in .env"; exit 1; }
	@umask 077 \
	  && set -a && . ./.env && set +a \
	  && export TF_VAR_account_id="$$CLOUDFLARE_ACCOUNT_ID" TF_VAR_user_email="$$USER_EMAIL" \
	            TF_VAR_team_name="$$TEAM_NAME" TF_VAR_bind_addr="$$BIND_ADDR" \
	            TF_VAR_port="$$PORT" TF_VAR_port_range="$$PORT_RANGE" TF_VAR_app_name="$$APP_NAME" \
	  && terraform -chdir=infra init -input=false \
	  && terraform -chdir=infra validate \
	  && { terraform -chdir=infra state show cloudflare_zero_trust_device_default_profile.warp >/dev/null 2>&1 \
	       || terraform -chdir=infra import cloudflare_zero_trust_device_default_profile.warp "$$CLOUDFLARE_ACCOUNT_ID"; } \
	  && { terraform -chdir=infra state show cloudflare_zero_trust_organization.this >/dev/null 2>&1 \
	       || terraform -chdir=infra import cloudflare_zero_trust_organization.this "$$CLOUDFLARE_ACCOUNT_ID"; } \
	  && { terraform -chdir=infra state show cloudflare_zero_trust_access_application.enrollment >/dev/null 2>&1 \
	       || { warp_app=$$(curl -fsS -H "Authorization: Bearer $$CLOUDFLARE_API_TOKEN" \
	                          "https://api.cloudflare.com/client/v4/accounts/$$CLOUDFLARE_ACCOUNT_ID/access/apps" \
	                        | jq -r 'first(.result[] | select(.type == "warp") | .id) // empty'); \
	            test -z "$$warp_app" \
	              || terraform -chdir=infra import cloudflare_zero_trust_access_application.enrollment "accounts/$$CLOUDFLARE_ACCOUNT_ID/$$warp_app"; }; } \
	  && terraform -chdir=infra apply -auto-approve \
	  && chmod 600 infra/terraform.tfstate \
	  && for backup in infra/*.backup; do test ! -e "$$backup" || chmod 600 "$$backup"; done
	@umask 077 && terraform -chdir=infra output -raw tunnel_token > .tunnel-token.tmp \
	  && test -s .tunnel-token.tmp || { rm -f .tunnel-token.tmp; echo "terraform output produced no token"; exit 1; }
	@chmod 600 .tunnel-token.tmp && mv .tunnel-token.tmp .tunnel-token
	@echo "wrote .tunnel-token — make up"

# Starts everything process-compose.yaml enables: this index page and every dev
# server not marked `disabled: true`. A unix socket instead of process-compose's
# default TCP 8080, so a second instance on this host — another repo's, or a
# long-running one — cannot collide with ours.
PC := process-compose --use-uds -u $(CURDIR)/$(LOGS)/process-compose.sock

up: build bind-addr firewall | $(LOGS)
	$(PC) up

# Talk to a detached `up` over the same socket.
attach: | $(LOGS)
	$(PC) attach

# Stop every process and the supervisor with it. Same socket, so it reaches
# ours and not another repo's instance.
down:
	$(PC) down

$(LOGS):
	mkdir -p $@

clean:
	cargo clean
	rm -rf web/dist web/node_modules $(LOGS)/*.log
