# Relationships And Faction Pressure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make trust, suspicion, loyalty, and faction pressure first-class so social and political consequences are easier to track than a single attitude score.

**Architecture:** Extend existing `RelationshipState`, `RelationshipChange`, `FactionState`, and `FactionChange` rather than creating a separate social subsystem. Keep numeric values bounded and project only player-visible pressure.

**Tech Stack:** Rust, Cargo workspace, Axum, SQLx/Postgres, Clap CLI, serde, tokio tests

---

## Current State

- `RelationshipState` has `source_id`, `target_id`, `attitude`, and `notes`.
- `RelationshipChange` supports attitude deltas and notes.
- `FactionState` has `standing`, `public_notes`, `hidden_notes`, and `revealed_goals`.
- `FactionChange` supports standing changes, goal reveals, public notes, and hidden notes.
- Prompt context renders relevant factions with standing and public goal, but does not render relationship details.
- Scenario samples contain rich faction identities but runtime pressure is not first-class.

## Target Behavior

- Relationships track separate `trust`, `suspicion`, and `loyalty` scores.
- Factions track `pressure` as a bounded value with public/hidden pressure notes.
- Deltas can change these values with reasons.
- Prompt context includes relevant relationship and faction pressure signals.
- Projection exposes player-visible social pressure without hidden faction notes.

## File Structure

- Modify: `crates/domain/src/state.rs`
  - Add fields to `RelationshipState` and `FactionState`.
  - Add relationship/faction change variants.
- Modify: `crates/engine/src/validation.rs`
  - Validate bounds and known entity IDs.
- Modify: `crates/engine/src/reducer.rs`
  - Apply social metric changes.
- Modify: `crates/engine/src/projection.rs`
  - Project visible relationships and faction pressure.
- Modify: `crates/engine/src/context.rs`, `crates/engine/src/prompt.rs`
  - Render social state.
- Modify: `crates/cli/scenarios/samples/*.json`
  - Add sample pressure clocks/notes only when supported by scenario schema.
- Modify: tests across domain, engine, API.

## Tasks

### Task 1: Extend Domain Social State

**Files:**
- Modify: `crates/domain/src/state.rs`
- Modify: `crates/domain/tests/serde_roundtrip_tests.rs`

- [ ] **Step 1: Write serde compatibility test**

Deserialize old relationship JSON with only `attitude` and assert `trust`, `suspicion`, and `loyalty` default to `0`. Deserialize old faction state and assert `pressure` defaults to `0`.

- [ ] **Step 2: Write new delta round-trip test**

Round-trip:

```json
{
  "type": "trust_changed",
  "source_id": "archduke-severin",
  "target_id": "player",
  "delta": 2,
  "reason": "Elowen defended Falkenmark publicly."
}
```

and:

```json
{
  "type": "pressure_changed",
  "faction_id": "imperial-throne",
  "delta": 5,
  "public": true,
  "reason": "The emperor expects progress before the wedding."
}
```

- [ ] **Step 3: Run expected failing command**

Run: `cargo test -p domain relationship faction_pressure`

Expected: fails until fields and variants exist.

- [ ] **Step 4: Add fields and variants**

Add defaulted fields:

```rust
pub trust: i32,
pub suspicion: i32,
pub loyalty: i32,
```

to `RelationshipState`, and:

```rust
pub pressure: i32,
pub public_pressure_notes: Vec<String>,
pub hidden_pressure_notes: Vec<String>,
```

to `FactionState`.

Add change variants for trust, suspicion, loyalty, pressure, and pressure notes.

- [ ] **Step 5: Run domain tests**

Run: `cargo test -p domain`

Expected: domain tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/domain/src/state.rs crates/domain/tests/serde_roundtrip_tests.rs
git commit -m "feat(domain): track social pressure"
```

### Task 2: Validate And Reduce Social Metrics

**Files:**
- Modify: `crates/engine/src/validation.rs`
- Modify: `crates/engine/src/reducer.rs`

- [ ] **Step 1: Write validation tests**

Add tests rejecting unknown relationship source/target IDs, values outside `-100..=100`, faction pressure outside `-100..=100`, and empty reasons.

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p engine social_pressure`

Expected: fails before validator support.

- [ ] **Step 3: Implement validation**

Reuse existing relationship entity ID checks. Calculate next metric values from current state plus delta and reject out-of-range values.

- [ ] **Step 4: Write reducer tests**

Assert:

```rust
TrustChanged creates relationship when missing.
SuspicionChanged updates existing relationship.
PressureChanged updates faction pressure.
PublicPressureNoteAdded and HiddenPressureNoteAdded append to separate vectors.
```

- [ ] **Step 5: Implement reducer**

Apply metric deltas by finding existing relationship or creating one with all metrics defaulted to zero, then applying the changed metric.

- [ ] **Step 6: Run tests**

Run: `cargo test -p engine social_pressure`

Run: `cargo test -p engine`

Expected: all engine tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/engine/src/validation.rs crates/engine/src/reducer.rs
git commit -m "feat(engine): apply social pressure deltas"
```

### Task 3: Project And Prompt Social State

**Files:**
- Modify: `crates/domain/src/state.rs`
- Modify: `crates/engine/src/projection.rs`
- Modify: `crates/engine/src/context.rs`
- Modify: `crates/engine/src/prompt.rs`

- [ ] **Step 1: Add visible DTOs**

Add `VisibleRelationship` and extend visible faction output if a visible faction DTO is introduced. Include trust/suspicion/loyalty and public pressure notes only.

- [ ] **Step 2: Write projection test**

Assert player projection includes public pressure and excludes hidden pressure notes. Assert relationship metrics are included for relationships involving visible NPCs or factions.

- [ ] **Step 3: Add context fields**

Add relevant relationships to `AgentContext`. Include relationships connected to active speaker, current factions, or player-facing entities.

- [ ] **Step 4: Update prompt**

Render:

```text
SOCIAL STATE:
Relationships: Severin -> player trust 3 suspicion 1 loyalty 0
Faction pressure: Imperial Throne pressure 5 public notes ...
```

Keep hidden pressure notes only in oracle context.

- [ ] **Step 5: Run tests**

Run: `cargo test -p engine projection context prompt`

Expected: projection and prompt tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/domain/src/state.rs crates/engine/src/projection.rs crates/engine/src/context.rs crates/engine/src/prompt.rs
git commit -m "feat(engine): render social pressure"
```

### Task 4: Cover Scenario And API Behavior

**Files:**
- Modify: `crates/persistence/src/store.rs`
- Modify: `crates/api/tests/memory_api_flows.rs`
- Modify: `crates/cli/scenarios/samples/bride-of-the-iron-archduke.json`

- [ ] **Step 1: Seed initial faction fields**

Set default pressure fields in `initial_world_state` when mapping scenario factions.

- [ ] **Step 2: Add memory API flow**

Submit a turn that changes Severin trust and imperial pressure. Assert projected state includes updated social metrics.

- [ ] **Step 3: Update Iron Archduke sample only through existing schema**

If sample schema does not yet support initial pressure fields, do not add unsupported keys. Instead add rules and clocks that exercise pressure through runtime deltas.

- [ ] **Step 4: Run tests**

Run: `cargo test -p api --test memory_api_flows social_pressure`

Run: `cargo test -p cli scenario_authoring_template_deserializes_and_validates`

Expected: API passes and scenario samples still validate.

- [ ] **Step 5: Commit**

```bash
git add crates/persistence/src/store.rs crates/api/tests/memory_api_flows.rs crates/cli/scenarios/samples/bride-of-the-iron-archduke.json
git commit -m "feat(gameplay): surface relationship pressure"
```

## Verification

Run:

```bash
cargo test -p domain
cargo test -p engine
cargo test -p api --test memory_api_flows
cargo test -p cli
cargo test --workspace
```

## Acceptance Criteria

- Relationships track trust, suspicion, and loyalty separately from attitude.
- Factions track pressure and pressure notes.
- Validator bounds all social metrics.
- Player projections never expose hidden pressure notes.
- Prompt context uses social state for political and emotional scenes.

## Risks

- Too many social numbers can become hard to interpret. Keep output compact and reserve narrative detail for notes.
- Initial scenario schema may not support runtime pressure fields; do not add unsupported sample keys.
- Relationship IDs involving the player need a stable convention such as `player`.

