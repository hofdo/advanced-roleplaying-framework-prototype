# Advanced Roleplaying Framework Prototype

Rust backend prototype for an LLM-driven roleplaying engine. The project is designed
to accept player input, assemble role-aware scenario context, call a language model,
extract structured world-state changes, validate those changes against scenario rules,
persist the authoritative state, and return only frontend-safe information to the
player.

The intended turn flow is:

`player input -> turn lock -> load state -> classify scene -> activate role -> build context/prompt -> provider call -> parse output -> validate delta -> reduce world state -> persist -> project frontend state`

This repository focuses on backend orchestration rather than frontend rendering. It
uses PostgreSQL for durable runtime storage, typed deltas for state mutation, and a
projection layer that prevents GM-only facts, hidden reasoning, and other internal
engine data from leaking into normal player-facing responses. The runtime defaults to
PostgreSQL-backed persistence and keeps an in-memory mode for fast local experiments.

## Architecture

```
crates/
  api/          — Axum HTTP server, route handlers, AppState
  domain/       — WorldState, WorldStateDelta, typed change variants
  engine/       — DefaultTurnPipeline, scene classifier, delta reducer, state projector
  persistence/  — sqlx repositories, PostgreSQL migrations, in-memory stores
  providers/    — OpenAiCompatibleProvider, MockProvider, retry policy
  shared/       — common types shared across crates
```

## Prerequisites

- Rust stable
- Docker (for PostgreSQL and Docker-gated tests)
- An OpenAI-compatible LLM endpoint for real turns

## Start PostgreSQL

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

## API Reference

### Core

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Service health (config-only, no network calls) |
| `GET` | `/readiness` | Service readiness (performs lightweight provider HTTP check) |

### Scenarios

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/scenarios` | Create scenario |
| `GET` | `/scenarios` | List scenarios |
| `GET` | `/scenarios/:id` | Get scenario |
| `DELETE` | `/scenarios/:id` | Delete scenario |

### Sessions

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/sessions` | Create session (links to a scenario) |
| `GET` | `/sessions/:id` | Get session |
| `DELETE` | `/sessions/:id` | Delete session |
| `PATCH` | `/sessions/:id/provider` | Assign a registered provider to this session |
| `GET` | `/sessions/:id/world-state` | Get current frontend-safe world state |
| `GET` | `/sessions/:id/export` | Export frontend-visible state (player projection) |
| `GET` | `/sessions/:id/events` | SSE stream of session events |

### Turns

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/sessions/:id/turn` | Submit a player turn (blocking response) |
| `POST` | `/sessions/:id/turn/stream` | Submit a player turn (SSE streaming response) |

### Admin

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/admin/sessions/:id/export/raw` | Full unfiltered `WorldState` (GM view) |
| `POST` | `/admin/sessions/:id/turn/debug` | Turn with `applied_delta` in response (for debugging LLM output) |

### Providers

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/providers` | Register a new LLM provider configuration |
| `GET` | `/providers` | List registered providers |
| `DELETE` | `/providers/:id` | Remove a provider |
| `GET` | `/providers/health` | Provider health (config check only) |
| `GET` | `/providers/readiness` | Provider readiness (live HTTP check) |

## Tests

Run all unit and in-memory integration tests (no Docker required):

```bash
cargo test --workspace
```

### Test Categories

**Unit tests** — compiled into each crate binary, no external deps:
- `domain`: serde roundtrip tests for all 18 `WorldStateDelta` change variants
- `engine/reducer`: 51 tests covering all reducer delta branches
- `engine/scene`: scene classifier for all `TurnMode` and keyword combinations
- `engine/pipeline`: `process_turn_debug()` correctness and delta capture
- `api/state`: `resolve_provider()` registry lookup and fallback logic

**In-memory integration tests** — zero Docker, zero network:

```bash
cargo test -p api --test memory_api_flows
```

20 tests covering the full API surface using `InMemoryStore` + `MockProvider`.
These run on every `cargo test --workspace` invocation.

**Docker-gated API tests** — require Docker daemon, use testcontainers:

```bash
cargo test -p api --test postgres_api_flows -- --ignored
```

Full pipeline tests against a real PostgreSQL container including behavioral
fixtures (faction standing, clock advancement, player projection secrecy).

**Docker-gated persistence tests** — require Docker daemon:

```bash
cargo test -p persistence --test repository_tests -- --ignored
```

24 tests covering every SQL repository method: `ScenarioRepository`,
`SessionRepository`, `WorldStateRepository`, `MessageRepository`,
`EventRepository`, `ProviderConfigRepository`, and `PostgresSessionTurnLock`.

**Provider HTTP tests** — wiremock mock server, no Docker:

```bash
cargo test -p providers
```

5 tests: HTTP 500 → `ProviderError::Status`, HTTP 429 → `ProviderError::RateLimit`,
retry-on-5xx success, malformed JSON body, missing `choices` field.

### Run Everything Including Docker Tests

```bash
cargo test --workspace && \
cargo test -p api --test postgres_api_flows -- --ignored && \
cargo test -p persistence --test repository_tests -- --ignored
```
