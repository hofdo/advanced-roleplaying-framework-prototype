# Persistence Crate

## Purpose

The `persistence` crate contains PostgreSQL-backed storage for the roleplaying engine. It owns migrations, repository traits and implementations, database row mapping, provider config storage, and database-backed session turn locking.

It keeps durable storage concerns out of the domain and engine crates.

## What Lives Here

- `migrations/` defines the PostgreSQL schema for scenarios, sessions, world states, messages, events, provider configs, and related data.
- `src/repositories.rs` defines repository traits, `PgPersistence`, row mapping, provider config records, and `TurnStateStore` integration.
- `src/lock.rs` implements a PostgreSQL-backed session turn lock.
- `tests/` contains Docker-gated repository tests.

## Why It Exists

The engine needs durable state, but SQL should not define the engine's behavior. This crate adapts database tables to domain and engine interfaces so the turn pipeline can persist and reload state without knowing how rows are stored.

That separation also lets the API use in-memory storage for fast tests and local experiments while production-like runs use PostgreSQL.

## Engine Context

During a turn, persistence may be responsible for:

- loading the session record
- loading the scenario and current world state
- loading recent messages for prompt context
- storing player and assistant messages
- storing engine events and raw delta information
- writing the new authoritative world-state version
- storing registered LLM provider configurations
- coordinating session turn locks across processes when configured

## Important Boundaries

- Keep SQL and row-shape knowledge in this crate.
- Do not put gameplay rules in repository methods. Repositories should store and retrieve already defined domain objects.
- Preserve versioning behavior for world state; the engine relies on authoritative state advancing predictably.
- Provider config records are configuration persistence, not provider implementations. Provider construction belongs in the API composition layer and provider behavior belongs in `providers`.
- Migrations should be compatible with existing data unless an intentional migration plan says otherwise.

## Useful Commands

```bash
cargo test -p persistence
cargo test -p persistence --test repository_tests -- --ignored
DATABASE_URL=postgres://roleplay:roleplay@localhost:5432/roleplay cargo test -p persistence --test repository_tests -- --ignored
```
