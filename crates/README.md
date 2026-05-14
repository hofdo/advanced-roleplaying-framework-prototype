# Crates

## Purpose

This folder contains the Rust workspace crates that make up the backend roleplaying engine. Each crate owns one architectural responsibility so the engine can keep domain rules, turn orchestration, HTTP routing, persistence, provider integration, and shared configuration separate.

The root `README.md` explains the project at a high level. This file explains how the crate folders fit together.

## What Lives Here

```text
api/          Axum HTTP API, route handlers, application state, API stores
cli/          `rp` binary — terminal driver linking the engine directly
domain/       Canonical scenario, world state, delta, visibility, and validation types
engine/       Turn pipeline, prompt/context construction, validation, reduction, projection
persistence/  PostgreSQL repositories, migrations, and database-backed locks (see PostgresSessionTurnLock)
providers/    LLM provider abstraction plus OpenAI-compatible, llama.cpp, OpenRouter, and mock implementations
shared/       Cross-crate configuration and shared application errors
```

PostgreSQL turn locking lives in `persistence/src/lock.rs` and is selected by `api::AppState` when `ROLEPLAY_STORAGE=postgres`.

## Why It Exists

The engine has a few boundaries that matter more than the individual web routes:

- Player-facing clients must not build prompts.
- Player-facing clients must not mutate authoritative state directly.
- LLMs may propose typed changes, but the backend validates and applies them.
- Normal API responses must project frontend-safe state rather than raw GM state.
- Provider-specific HTTP behavior must not leak into the turn pipeline.
- Storage details must not define domain or engine behavior.

Splitting the workspace this way keeps those rules enforceable in code review and makes it easier to test each layer independently.

## Engine Context

The normal turn flow crosses the crates in this order:

```text
api
-> engine
-> providers
-> engine
-> domain
-> persistence or in-memory api store
-> engine projection
-> api response
```

The API crate accepts the request and resolves storage/provider dependencies. The engine crate prepares context, calls the provider, parses and validates the model output, reduces state, persists the result through a store trait, and returns a player-safe projection. The domain crate defines the data being moved through that process. The persistence crate supplies durable implementations for the store boundaries. The providers crate isolates model transport details.

## Dependency Direction

Current dependency shape:

```text
api -> domain, engine, persistence, providers, shared
engine -> domain, providers, shared
persistence -> domain, engine, shared
providers -> external HTTP/JSON crates only
domain -> serialization, validation, IDs, time
shared -> config and common error dependencies
```

Important rule: `domain` should remain the lowest-level engine model. It should not depend on `api`, `engine`, `persistence`, or `providers`.

## Useful Commands

```bash
cargo test --workspace
cargo test -p api --test memory_api_flows
cargo test -p providers
cargo check --workspace
```

Docker-gated PostgreSQL tests are documented in the root `README.md`.
