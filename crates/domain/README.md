# Domain Crate

## Purpose

The `domain` crate defines the canonical data model for the roleplaying engine. It contains the scenario format, authoritative world state, typed state deltas, player-visible projections, IDs, messages, and domain-level validation.

This crate is the vocabulary shared by the rest of the workspace.

## What Lives Here

- `src/ids.rs` defines strongly typed identifiers used across scenarios, sessions, messages, and state.
- `src/scenario.rs` defines scenario setup data such as roles, NPCs, factions, quests, secrets, locations, and clock templates.
- `src/state.rs` defines runtime world state, visibility flags, typed `WorldStateDelta` mutations, frontend-visible state, changed-entity references, and message records.
- `src/validation.rs` validates domain objects before they enter the engine or storage.
- `DELTA_EXTENSION.md` tracks the cross-crate checklist for adding new `WorldStateDelta` variants safely.
- `tests/` covers serialization round trips and validation behavior.

## Why It Exists

The engine is deliberately built around typed state transitions. The LLM can propose changes, but it proposes them as `WorldStateDelta` values instead of replacing the whole world state. The backend can then validate, reduce, persist, and project those changes deterministically.

Putting these types in a low-level crate prevents HTTP routes, SQL repositories, and provider implementations from inventing their own versions of the same concepts.

## Engine Context

The domain model appears throughout the turn flow:

- Scenarios seed sessions and initial world state.
- Current world state is loaded before each turn.
- Prompt context is derived from domain data.
- LLM output is parsed into typed delta structures.
- Delta validation checks proposed changes against known entities and secrecy rules.
- Reducers apply validated deltas to authoritative state.
- Projectors derive frontend-safe views from authoritative state.

## Important Boundaries

- Do not add dependencies on `api`, `engine`, `persistence`, or `providers`.
- Keep this crate focused on data definitions and domain invariants.
- Avoid provider-specific or database-specific fields unless they are truly part of the game domain.
- Visibility and projection types are part of the secrecy boundary; changes should be reviewed with player-facing leakage in mind.
- New delta variants should include validation, reducer, projection, serialization, and API behavior updates in neighboring crates.

## Useful Commands

```bash
cargo test -p domain
cargo test -p domain --test serde_roundtrip_tests
cargo test -p domain --test validation_tests
```
