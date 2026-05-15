# API Crate

## Purpose

The `api` crate is the HTTP and application-composition layer for the roleplaying engine. It exposes Axum routes, wires together application state, chooses the active storage and provider implementations, and translates engine errors into HTTP responses.

It should orchestrate requests. It should not become the place where core world-state rules, prompt rules, or provider protocol details live.

## What Lives Here

- `src/main.rs` starts the service, loads configuration, builds `AppState`, and binds the HTTP listener.
- `src/app.rs` defines routes, request/response bodies, SSE streaming behavior, and admin/debug route protection.
- `src/state.rs` owns application state, in-memory API storage, Postgres store adapters, provider construction, and session projection helpers.
- `src/error.rs` maps domain, engine, persistence, and provider failures into API responses.
- `tests/` contains in-memory API flow tests, Postgres-gated API tests, provider dispatch tests, replay helpers, behavioral fixtures, and live local-LLM smoke tests.

## Why It Exists

The engine needs a public boundary where clients can create scenarios, start sessions, submit turns, stream narration, register providers, and inspect safe state. This crate provides that boundary while keeping the core engine usable without HTTP.

The API also owns cross-cutting operational behavior:

- admin token enforcement
- health and readiness routes
- server-sent event formatting
- provider registration and session provider assignment
- provider health and readiness testing for the configured provider
- selection between in-memory and PostgreSQL-backed stores
- public and raw timeline inspection
- raw debug exports that must not be exposed on normal player routes

## Engine Context

For a player turn, route handlers load the session, resolve the configured provider, construct a `DefaultTurnPipeline`, and call into `engine`. The handler returns only the pipeline result or an SSE event stream; it does not apply deltas by hand.

For streaming turns, this crate streams visible narration tokens first, then lets the engine finalize the turn by extracting and validating structured state changes before persistence.

The public API currently exposes scenario CRUD plus `PUT /scenarios/:id`, session CRUD plus `GET /sessions/:id/timeline`, `PATCH /sessions/:id/provider`, `GET /sessions/:id/world-state`, `GET /sessions/:id/export`, `GET /sessions/:id/events`, turn submission routes, provider registration plus `POST /providers/test`, and admin-only raw export/timeline/debug endpoints.

## Important Boundaries

- Keep route handlers thin. Domain and world-state rules belong in `domain` or `engine`.
- Keep provider-specific request formats in `providers`; API code should depend on the `LlmProvider` trait and provider config records.
- Keep SQL in `persistence`; API store adapters should delegate to repositories rather than embedding queries.
- Normal player routes must use frontend-safe projections. Raw state belongs behind admin/debug routes only.
- Missing explicit session providers should fail clearly instead of silently using a different model.

## Useful Commands

```bash
cargo run -p api
ROLEPLAY_STORAGE=memory cargo run -p api
cargo test -p api --test memory_api_flows
cargo test -p api --test behavioral_fixtures -- --ignored --test-threads=1
cargo test -p api --test provider_dispatch_tests
cargo test -p api --test postgres_api_flows -- --ignored --test-threads=1
```
