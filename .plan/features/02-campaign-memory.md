# Campaign Memory Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add durable session memory and summary evolution so long campaigns preserve important context without flooding prompts with every event.

**Architecture:** Extend `WorldState` with structured memory entries while preserving the existing `summary` field for compact prompt context. Generate memory updates through typed deltas, validate them in the engine, persist them as part of JSON world state, and project only player-visible memory to normal clients.

**Tech Stack:** Rust, Cargo workspace, Axum, SQLx/Postgres, Clap CLI, serde, tokio tests

---

## Current State

- `WorldState` has `summary: Option<String>` and `recent_events: Vec<String>`.
- `WorldStateDelta` has `summary_update: Option<SummaryUpdate>` and `event_log_entries`.
- `BasicContextBuilder` renders `recent_summary` and the last six messages.
- Persistence stores `WorldState` as JSONB, so additive serde fields can be introduced with defaults.
- API exports project `FrontendVisibleState`; admin export returns raw `WorldState`.
- CLI `world` shows projected or raw state.

## Target Behavior

- Sessions can store durable memory entries with text, visibility, importance, source turn, and optional related entity IDs.
- Delta extraction can add memory entries and update summaries.
- Prompt context includes a bounded set of high-importance visible memory plus GM-only memory only in oracle contexts.
- Normal player projections expose only player-visible memory.
- Existing sessions deserialize with empty memory fields.

## File Structure

- Modify: `crates/domain/src/state.rs`
  - Add `MemoryEntry`, `MemoryVisibility`, and `MemoryChange`.
  - Add `memories: Vec<MemoryEntry>` to `WorldState`.
  - Add `memory_changes: Vec<MemoryChange>` to `WorldStateDelta`.
- Modify: `crates/engine/src/context.rs`
  - Add visible and GM memory fields to `AgentContext`.
- Modify: `crates/engine/src/prompt.rs`
  - Render memory in visible and oracle contexts with visibility filtering.
- Modify: `crates/engine/src/validation.rs`
  - Validate memory changes have reasons and safe visibility.
- Modify: `crates/engine/src/reducer.rs`
  - Apply memory additions and updates.
- Modify: `crates/engine/src/projection.rs`
  - Add visible memory to frontend projection.
- Modify: `crates/persistence/src/store.rs`
  - Seed empty memory in initial world state.
- Modify: `crates/api/tests/memory_api_flows.rs`, `crates/api/tests/postgres_api_flows.rs`
  - Cover memory add/update behavior.

## Tasks

### Task 1: Add Domain Memory Types

**Files:**
- Modify: `crates/domain/src/state.rs`
- Modify: `crates/domain/tests/serde_roundtrip_tests.rs`

- [ ] **Step 1: Write failing serde test**

Add a test that deserializes a `WorldState` JSON without `memories` and asserts `memories.is_empty()`. Add a second test that round-trips:

```json
{
  "type": "added",
  "text": "Elowen learned Marta judges nobles by how they treat servants.",
  "visibility": "player_known",
  "importance": 7,
  "related_entity_ids": ["steward-marta"],
  "reason": "The player spoke respectfully to staff."
}
```

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p domain memory`

Expected: fails because memory fields and types are missing.

- [ ] **Step 3: Add types**

Add:

```rust
pub struct MemoryEntry {
    pub id: EntityKey,
    pub text: String,
    pub visibility: MemoryVisibility,
    pub importance: u8,
    pub related_entity_ids: Vec<EntityKey>,
    pub source_message_id: Option<MessageId>,
}

pub enum MemoryVisibility {
    PlayerKnown,
    GmOnly,
}

pub enum MemoryChange {
    Added { text: String, visibility: MemoryVisibility, importance: u8, related_entity_ids: Vec<EntityKey>, reason: String },
    ImportanceChanged { memory_id: EntityKey, importance: u8, reason: String },
}
```

Add `#[serde(default)]` to `WorldState.memories` and `WorldStateDelta.memory_changes`.

- [ ] **Step 4: Run domain tests**

Run: `cargo test -p domain`

Expected: all domain tests pass and old JSON shapes deserialize.

- [ ] **Step 5: Commit**

```bash
git add crates/domain/src/state.rs crates/domain/tests/serde_roundtrip_tests.rs
git commit -m "feat(domain): add campaign memory deltas"
```

### Task 2: Validate And Reduce Memory Deltas

**Files:**
- Modify: `crates/engine/src/validation.rs`
- Modify: `crates/engine/src/reducer.rs`

- [ ] **Step 1: Write failing validation tests**

Add tests:

```rust
#[test]
fn rejects_memory_without_reason() { /* MemoryChange::Added with reason "" */ }

#[test]
fn rejects_memory_importance_above_ten() { /* importance 11 */ }

#[test]
fn rejects_unknown_memory_id_for_importance_change() { /* memory_id "missing" */ }
```

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p engine memory`

Expected: validation tests fail before validator support.

- [ ] **Step 3: Implement validation**

Require non-empty reasons, require `importance <= 10`, and require `ImportanceChanged.memory_id` to exist in `world_state.memories`.

- [ ] **Step 4: Write reducer tests**

Add tests that `MemoryChange::Added` creates IDs like `memory-<next_version>-<index>` and `ImportanceChanged` updates only the matching entry.

- [ ] **Step 5: Implement reducer**

Apply memory changes before version increment, following the same ID style used for facts.

- [ ] **Step 6: Run engine tests**

Run: `cargo test -p engine`

Expected: all engine tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/engine/src/validation.rs crates/engine/src/reducer.rs
git commit -m "feat(engine): validate and reduce campaign memory"
```

### Task 3: Render And Project Memory

**Files:**
- Modify: `crates/engine/src/context.rs`
- Modify: `crates/engine/src/prompt.rs`
- Modify: `crates/engine/src/projection.rs`
- Modify: `crates/domain/src/state.rs`

- [ ] **Step 1: Add frontend visible type**

Add:

```rust
pub struct VisibleMemory {
    pub id: EntityKey,
    pub text: String,
    pub importance: u8,
    pub related_entity_ids: Vec<EntityKey>,
}
```

and add `visible_memories: Vec<VisibleMemory>` to `FrontendVisibleState` with `#[serde(default)]` if needed for compatibility.

- [ ] **Step 2: Write projection test**

Add a test that player projection includes `MemoryVisibility::PlayerKnown` and excludes `MemoryVisibility::GmOnly`, while admin projection includes both.

- [ ] **Step 3: Add context fields**

Add `player_memories` and `gm_only_memories` to `AgentContext`. In `BasicContextBuilder`, choose at most eight player memories and five GM memories, sorted by descending importance then insertion order.

- [ ] **Step 4: Write prompt test**

Assert narration-safe context includes player memory and excludes GM-only memory. Assert oracle context includes both.

- [ ] **Step 5: Implement rendering**

Add a `CAMPAIGN MEMORY` section to prompt rendering. Include `importance` and related entity IDs only if they help disambiguate memory.

- [ ] **Step 6: Run engine tests**

Run: `cargo test -p engine context prompt projection`

Expected: context, prompt, and projection tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/domain/src/state.rs crates/engine/src/context.rs crates/engine/src/prompt.rs crates/engine/src/projection.rs
git commit -m "feat(engine): render visible campaign memory"
```

### Task 4: Persist And Cover API Flows

**Files:**
- Modify: `crates/persistence/src/store.rs`
- Modify: `crates/api/tests/memory_api_flows.rs`
- Modify: `crates/api/tests/postgres_api_flows.rs`

- [ ] **Step 1: Seed initial state**

In `initial_world_state`, set `memories: vec![]`.

- [ ] **Step 2: Add memory API test**

In `memory_api_flows.rs`, submit a turn whose provider returns a `memory_changes` addition and a `summary_update`. Assert:

```rust
assert_eq!(turn_json["world_state_version"], 1);
assert!(turn_json["frontend_state_patch"]["visible_state"]["visible_memories"].as_array().unwrap().len() == 1);
```

- [ ] **Step 3: Add Postgres API test**

Add an ignored Postgres version that fetches raw admin export and asserts `world_state.memories` contains the entry after persistence.

- [ ] **Step 4: Run tests**

Run: `cargo test -p api --test memory_api_flows campaign_memory`

Run: `TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test postgres_api_flows campaign_memory -- --ignored --test-threads=1`

Expected: memory and Postgres flows pass.

- [ ] **Step 5: Commit**

```bash
git add crates/persistence/src/store.rs crates/api/tests/memory_api_flows.rs crates/api/tests/postgres_api_flows.rs
git commit -m "feat(api): persist campaign memory"
```

## Verification

Run:

```bash
cargo test -p domain
cargo test -p engine
cargo test -p api --test memory_api_flows
cargo test --workspace
```

Optional with Docker:

```bash
TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test postgres_api_flows -- --ignored --test-threads=1
```

## Acceptance Criteria

- Existing persisted world states deserialize with empty memory.
- Typed memory deltas validate, reduce, persist, and project correctly.
- Prompt context includes bounded player-visible memory and keeps GM-only memory out of visible narration.
- Summary updates continue to work alongside structured memory entries.
- API memory and Postgres tests cover durable memory behavior.

## Risks

- Memory can become another unbounded context source; enforce selection limits in `BasicContextBuilder`.
- Visibility errors can leak hidden memory; projection and prompt tests must cover both player and admin views.
- JSONB storage makes schema migration easy but can hide compatibility issues; serde tests must cover old and new shapes.

