.PHONY: up down migrate seed-sde backfill-kills build logs test clean

up:
	docker compose up -d

down:
	docker compose down

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
