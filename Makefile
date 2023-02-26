.PHONY: db build default test check-deps

DATABASE_URL:=postgresql://postgres:password@localhost:5432/postgres

default:
	cat Makefile | grep ":"

check-deps:
	cargo upgrade --dry-run --verbose

check:
	cargo clippy

test-coverage:
	cargo tarpaulin --out Html --implicit-test-threads
	xdg-open tarpaulin-report.html

watch:
	cargo watch -w src --shell 'make test'

dbup:
	docker-compose up -d database 
	while ! nc -z localhost 5432; do sleep 1; echo "waiting on postgres..."; done;
	docker ps | grep batch

dbdown:
	docker-compose down

doc:
	cargo doc --no-deps

test: dbup doc
	cargo test

#
# Examples
#
test-examples: test
	export DATABASE_URL=${DATABASE_URL}
	psql ${DATABASE_URL} -c "DROP TABLE IF EXISTS metrics"
	psql ${DATABASE_URL} -c "CREATE TABLE metrics (url TEXT, latency_ms BIGINT);"
	cargo run --quiet --example basic
	cargo run --quiet --example configuration
	psql ${DATABASE_URL} -c "SELECT count(*) FROM metrics where latency_ms = 42" | grep 2

	psql ${DATABASE_URL} -c "DROP TABLE IF EXISTS users"
	psql ${DATABASE_URL} -c "CREATE TABLE IF NOT EXISTS users (id BIGINT NOT NULL, id2 BIGINT, name TEXT);"
	cargo run --quiet --example multi_producer
	psql ${DATABASE_URL} -c "SELECT count(*) FROM users" | grep 40960 

	psql ${DATABASE_URL} -c "DROP TABLE IF EXISTS spotprices"
	psql ${DATABASE_URL} -c "CREATE TABLE spotprices (dt TIMESTAMPTZ, instance TEXT, os TEXT, region TEXT, az TEXT, price FLOAT8);"
	cargo run --quiet --example load_csv 
	psql ${DATABASE_URL} -c "SELECT count(*) FROM spotprices" | grep 9000

