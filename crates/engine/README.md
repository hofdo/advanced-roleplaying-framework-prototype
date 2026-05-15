# Engine Crate

## Purpose

The `engine` crate contains the roleplaying turn engine. It turns player input plus current state into prompt context, calls an LLM provider, parses model output, strips hidden reasoning, validates proposed changes, applies state reductions, persists the result through a store boundary, and returns frontend-safe output.

This is the core behavior of the project.

## What Lives Here

- `context.rs` builds role, NPC, faction, player, clue, action-resolution, message, and reasoning-style context for prompts.
- `scene.rs` classifies the current player input into a scene reasoning style.
- `prompt/` builds prompts, renders narration-safe and oracle contexts, parses structured model output, and prepares repair prompts.
- `safety.rs` strips hidden reasoning blocks from provider output.
- `validation.rs` validates proposed world-state deltas against current state and secrecy constraints.
- `reducer.rs` applies validated deltas to authoritative world state.
- `projection.rs` produces player-visible state and changed-entity references.
- `lock.rs` defines session turn locking.
- `pipeline/` wires the full turn lifecycle together, including pipeline events and typed turn-state helpers.

## Why It Exists

The API needs one place to ask, "process this turn correctly." The engine crate provides that boundary. It centralizes the rules that make LLM output safe enough to use:

- prompt construction is backend-owned
- player-visible narration and oracle delta extraction are split so visible generation never needs GM-only facts
- player narration and world-state mutation are separated where streaming allows it
- hidden reasoning is removed
- proposed deltas are parsed and validated before use
- authoritative state is changed only by reducers
- frontend output is projected from authoritative state

## Engine Context

The default pipeline coordinates these steps:

```text
acquire session turn lock
-> load session, scenario, world state, and recent messages
-> classify scene and activate role identity
-> build prompt context
-> call provider
-> parse and sanitize model output
-> validate delta
-> reduce world state
-> persist messages, events, delta, and version
-> project frontend-safe state
```

Streaming turns use the same safety goals with a different ordering: visible narration is streamed first, then the engine extracts and validates structured state changes before finalizing persistence.

## Important Boundaries

- The engine should depend on provider traits, not concrete provider transport details.
- The engine should depend on store traits, not SQL.
- Validation should reject unsafe or incoherent deltas rather than patching them silently.
- Reducers should apply already validated deltas and avoid re-implementing broad validation logic.
- Projection is part of the secrecy boundary. Do not expose GM-only facts, hidden clocks, hidden NPCs, hidden clues, hidden action resolutions, or raw internal state through player-facing projections.

## Useful Commands

```bash
cargo test -p engine
cargo test -p engine validation
cargo test -p engine projection
```
