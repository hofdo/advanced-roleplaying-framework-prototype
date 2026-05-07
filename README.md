# Advanced Roleplaying Framework Prototype

Rust backend prototype for the advanced roleplaying engine. The runtime defaults to
PostgreSQL-backed persistence and keeps an in-memory mode for fast local experiments.

## Prerequisites

- Rust stable
- Docker
- An OpenAI-compatible LLM endpoint for real turns

## Start PostgreSQL

For the API runtime, you can use the checked-in Compose file:

```bash
docker compose up -d postgres
```

The database listens on `localhost:5432` with:

```text
database: roleplay
user: roleplay
password: roleplay
```

## Run The API With PostgreSQL

```bash
export DATABASE_URL=postgres://roleplay:roleplay@localhost:5432/roleplay
export ROLEPLAY_STORAGE=postgres
export LLM_BASE_URL=http://localhost:8081/v1
export LLM_MODEL=local-model
cargo run -p api
```

The API runs migrations on startup when PostgreSQL storage is enabled.

Smoke check:

```bash
curl http://127.0.0.1:8080/health
```

Expected shape:

```json
{"status":"ok","active_provider":"local-llama","database":"postgres:ok"}
```

## Run In Memory Mode

Use this when you only want to exercise routes without durable storage:

```bash
ROLEPLAY_STORAGE=memory cargo run -p api
```

## Tests

```bash
cargo test --workspace
```

The PostgreSQL API flow tests are managed by `testcontainers`. They start a temporary
PostgreSQL container automatically and do not require `docker compose` or `DATABASE_URL`:

```bash
cargo test -p api --test postgres_api_flows -- --ignored
```

These tests still require a reachable Docker daemon.
