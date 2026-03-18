.PHONY: up down up-prod down-prod migrate seed-sde backfill-kills build logs test clean

PROD := -f docker-compose.yml -f docker-compose.prod.yml

# ── Local dev (plain HTTP on :3000) ──────────────────────────────────
up:
	docker compose up -d

down:
	docker compose down

# ── Production (TLS on :443 via Let's Encrypt) ──────────────────────
up-prod:
	docker compose $(PROD) up -d

down-prod:
	docker compose $(PROD) down

# ── Common ───────────────────────────────────────────────────────────
migrate:
	docker compose exec backend-server /usr/local/bin/nea-server migrate

seed-sde:
	docker compose run --rm --entrypoint /usr/local/bin/sde-import backend-worker

backfill-kills:
	docker compose run --rm --entrypoint /usr/local/bin/kill-backfill backend-worker

build:
	docker compose build

logs:
	docker compose logs -f

test:
	cd backend && cargo test --lib --bins

clean:
	docker compose down -v --rmi local --remove-orphans
