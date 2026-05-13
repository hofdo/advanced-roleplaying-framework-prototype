# Player Character State Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add lightweight player traits, goals, conditions, and resources so the engine can remember the player character beyond inventory and facts.

**Architecture:** Store player character state inside `WorldState` and mutate it through typed deltas. Project player-visible fields to clients, keep GM-only notes hidden, and render compact player context in prompts.

**Tech Stack:** Rust, Cargo workspace, Axum, SQLx/Postgres, Clap CLI, serde, tokio tests

---

## Current State

- The player is represented indirectly through facts, inventory, relationships, and summary.
- `WorldState` has no first-class player profile, goals, conditions, or resources.
- `InventoryItem` tracks visible items.
- Prompt context does not include player-specific traits except through facts and recent messages.

## Target Behavior

- `WorldState` contains a `PlayerCharacterState`.
- Deltas can add/update player traits, goals, conditions, and resources.
- Projection includes player-visible traits/goals/conditions/resources.
- Prompt context includes compact player state in visible and oracle contexts.
- GM-only player notes stay hidden from normal projections and visible narration.

## File Structure

- Modify: `crates/domain/src/state.rs`
  - Add player character state structs and delta variants.
- Modify: `crates/persistence/src/store.rs`
  - Seed player state in `initial_world_state`.
- Modify: `crates/engine/src/validation.rs`
  - Validate player state changes.
- Modify: `crates/engine/src/reducer.rs`
  - Apply player state changes.
- Modify: `crates/engine/src/projection.rs`
  - Project visible player state.
- Modify: `crates/engine/src/context.rs`, `crates/engine/src/prompt.rs`
  - Render player state.
- Modify: `crates/api/tests/memory_api_flows.rs`, `crates/cli/tests/cli_smoke.rs`
  - Cover API and CLI visibility.

## Tasks

### Task 1: Add Player State Domain Model

**Files:**
- Modify: `crates/domain/src/state.rs`
- Modify: `crates/domain/tests/serde_roundtrip_tests.rs`

- [ ] **Step 1: Write failing serde tests**

Add tests for missing `player` field defaulting and for a delta:

```json
{
  "type": "resource_changed",
  "resource_id": "resolve",
  "delta": -1,
  "reason": "Elowen forced herself to stand firm in public."
}
```

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p domain player_character`

Expected: fails because player state types are missing.

- [ ] **Step 3: Add structs**

Add:

```rust
pub struct PlayerCharacterState {
    pub traits: Vec<PlayerTrait>,
    pub goals: Vec<PlayerGoal>,
    pub conditions: Vec<PlayerCondition>,
    pub resources: Vec<PlayerResource>,
    pub gm_notes: Vec<String>,
}
```

Add supporting structs with IDs, labels, descriptions, visibility where needed, and bounded numeric values for resources.

- [ ] **Step 4: Add delta enum**

Add `player_changes: Vec<PlayerChange>` to `WorldStateDelta` with variants:

```rust
TraitAdded
GoalAdded
GoalProgressed
ConditionAdded
ConditionCleared
ResourceChanged
GmNoteAdded
```

Use serde snake_case and defaults.

- [ ] **Step 5: Run domain tests**

Run: `cargo test -p domain`

Expected: domain tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/domain/src/state.rs crates/domain/tests/serde_roundtrip_tests.rs
git commit -m "feat(domain): add player character state"
```

### Task 2: Validate And Reduce Player Changes

**Files:**
- Modify: `crates/engine/src/validation.rs`
- Modify: `crates/engine/src/reducer.rs`

- [ ] **Step 1: Write validation tests**

Add tests that reject empty reasons, resource values outside configured bounds, clearing an unknown condition, and progressing an unknown goal.

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p engine player_character`

Expected: fails until validation exists.

- [ ] **Step 3: Implement validation**

Use current state to check known IDs for updates and clears. Require non-empty IDs, labels, and reasons for additions. Clamp-free validation should reject invalid resource values rather than silently correcting them.

- [ ] **Step 4: Write reducer tests**

Assert trait and goal additions append once, resource changes adjust current value, condition clear removes the condition, and GM notes append without becoming visible.

- [ ] **Step 5: Implement reducer**

Apply player changes before version increment. Maintain stable IDs supplied by the delta for player state elements.

- [ ] **Step 6: Run tests**

Run: `cargo test -p engine player_character`

Run: `cargo test -p engine`

Expected: all engine tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/engine/src/validation.rs crates/engine/src/reducer.rs
git commit -m "feat(engine): apply player character changes"
```

### Task 3: Project And Render Player State

**Files:**
- Modify: `crates/domain/src/state.rs`
- Modify: `crates/engine/src/projection.rs`
- Modify: `crates/engine/src/context.rs`
- Modify: `crates/engine/src/prompt.rs`

- [ ] **Step 1: Add visible DTOs**

Add `VisiblePlayerCharacterState` with traits, goals, conditions, and resources. Do not include `gm_notes`.

- [ ] **Step 2: Write projection test**

Build a state with one trait, one condition, one resource, and a GM note. Assert player projection includes the visible fields and excludes the GM note. Assert admin projection can include GM note only if a raw/admin DTO is deliberately added; otherwise raw export remains the admin path.

- [ ] **Step 3: Add context fields**

Add `player_state` to `AgentContext`. Use visible fields for visible context and include GM notes only in oracle context if secrecy-boundary split exists.

- [ ] **Step 4: Update prompt rendering**

Add `PLAYER CHARACTER` section with traits, active goals, active conditions, resources, and no GM notes in narration-safe rendering.

- [ ] **Step 5: Run tests**

Run: `cargo test -p engine projection context prompt`

Expected: tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/domain/src/state.rs crates/engine/src/projection.rs crates/engine/src/context.rs crates/engine/src/prompt.rs
git commit -m "feat(engine): project player character state"
```

### Task 4: Cover API And CLI

**Files:**
- Modify: `crates/persistence/src/store.rs`
- Modify: `crates/api/tests/memory_api_flows.rs`
- Modify: `crates/cli/tests/cli_smoke.rs`

- [ ] **Step 1: Seed initial player state**

Set `player: PlayerCharacterState::default()` in `initial_world_state` and test fixtures.

- [ ] **Step 2: Add API memory flow**

Submit a turn with `player_changes` adding a condition and changing a resource. Assert `frontend_state_patch.visible_state.player` contains the condition/resource.

- [ ] **Step 3: Add CLI world smoke check**

After a turn, run `rp world <SESSION_ID>` and assert the output JSON includes player state and excludes GM notes.

- [ ] **Step 4: Run tests**

Run: `cargo test -p api --test memory_api_flows player_character`

Run: `cargo test -p cli --test cli_smoke player_character`

Expected: API and CLI coverage pass.

- [ ] **Step 5: Commit**

```bash
git add crates/persistence/src/store.rs crates/api/tests/memory_api_flows.rs crates/cli/tests/cli_smoke.rs
git commit -m "feat(api): expose player character state"
```

## Verification

Run:

```bash
cargo test -p domain
cargo test -p engine
cargo test -p api --test memory_api_flows
cargo test -p cli --test cli_smoke
cargo test --workspace
```

## Acceptance Criteria

- Player character state exists with serde defaults for old state JSON.
- Typed player changes validate and reduce deterministically.
- Normal projection exposes only player-safe player state.
- Prompt context includes player traits, goals, conditions, and resources.
- CLI world output surfaces visible player state.

## Risks

- Player state can overlap with facts and inventory; keep it lightweight and avoid modeling full character sheets.
- Resource bounds must be explicit enough for validation.
- GM notes must never appear in narration-safe prompt rendering or normal projection.

