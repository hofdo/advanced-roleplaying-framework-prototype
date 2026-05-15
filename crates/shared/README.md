# Shared Crate

## Purpose

The `shared` crate contains cross-crate configuration and common application error types. It exists for values that are genuinely shared across the workspace but do not belong to the domain model, engine behavior, provider transport layer, or persistence layer.

This crate should stay small, but it now also carries replay-fixture DTOs used by CLI/API export helpers and behavioral tests.

## What Lives Here

- `src/config.rs` defines `AppConfig` and nested server, database, storage, provider, admin, and debug configuration types.
- `src/error.rs` defines shared application-level errors.
- `src/replay.rs` defines `ReplayFixture` and related fixture draft types shared by the CLI and API tests.
- `src/lib.rs` re-exports the shared modules used by other crates.

## Why It Exists

Multiple crates need to understand the same runtime configuration. For example, the API starts the server, persistence needs database settings, provider construction needs default provider settings, and tests often switch between in-memory and PostgreSQL storage.

Keeping configuration in one crate avoids duplicating environment parsing and validation across those layers.

## Engine Context

Configuration influences how the engine is assembled rather than how the domain rules work:

- storage backend selection controls whether the API uses memory or PostgreSQL stores
- provider defaults control which concrete LLM provider is built at startup
- debug settings control whether raw provider output may be retained
- admin settings control whether sensitive debug routes are exposed
- replay-fixture drafts reuse the canonical domain scenario/world-state types so exported fixtures stay aligned with current gameplay state

The turn pipeline should receive already constructed dependencies and should not parse environment variables directly.

## Important Boundaries

- Do not add broad utility code here unless multiple crates already need it.
- Do not put domain concepts here; use `domain`.
- Do not put provider protocol logic here; use `providers`.
- Do not put SQL configuration beyond general database config here; use `persistence` for database behavior.
- Do not broaden replay-fixture helpers into a general-purpose serialization layer.
- Keep defaults and environment parsing explicit because configuration mistakes can expose unsafe routes or select the wrong provider.

## Useful Commands

```bash
cargo test -p shared
cargo test -p shared config
```
