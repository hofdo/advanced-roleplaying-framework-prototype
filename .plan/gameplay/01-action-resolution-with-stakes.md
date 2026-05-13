# Action Resolution With Stakes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add structured risky-action resolution so action turns consistently declare intent, stakes, outcome, consequences, and state changes.

**Architecture:** Model action resolution as a typed delta family that can advance clocks, change NPC/faction state, and add visible facts. Keep the LLM proposing outcomes, but require engine validation to reject incomplete or consequence-free risky actions.

**Tech Stack:** Rust, Cargo workspace, Axum, SQLx/Postgres, Clap CLI, serde, tokio tests

---

## Current State

- `TurnMode::Action` exists and prompt rules say the player is performing an in-world action.
- `RuleBasedSceneClassifier` maps combat/action keywords to `TacticalCombat`.
- `BasicReasoningStyleOptimizer` already says tactical combat should use "clear action resolution" and "stakes beyond player HP".
- `WorldStateDelta` can already change facts, NPCs, factions, quests, clocks, relationships, inventory, location, scene, and summary.
- There is no explicit action-resolution record tying intent, stakes, outcome, and consequences together.

## Target Behavior

- Risky action turns can produce a structured `ActionResolution`.
- The prompt asks for intent, stakes, outcome tier, consequence summary, and linked delta effects.
- Validation rejects risky action resolutions without stakes or without an observable state/event consequence.
- Projected state and timeline/debug output can show action resolutions to players when visible.
- Existing non-action turns are not required to include action resolution.

## File Structure

- Modify: `crates/domain/src/state.rs`
  - Add `ActionResolution`, `ActionOutcome`, and `ActionResolutionChange`.
  - Add `action_resolutions` to `WorldState`.
  - Add `action_resolution_changes` to `WorldStateDelta`.
- Modify: `crates/engine/src/scene.rs`
  - Add more risky-action keywords only if tests show misses.
- Modify: `crates/engine/src/context.rs`
  - Include recent action resolutions in prompt context.
- Modify: `crates/engine/src/prompt.rs`
  - Update Action/TacticalCombat directives and output contract.
- Modify: `crates/engine/src/validation.rs`
  - Validate action resolution completeness.
- Modify: `crates/engine/src/reducer.rs`
  - Persist action resolution entries.
- Modify: `crates/engine/src/projection.rs`
  - Project visible action resolutions if added to frontend state.
- Modify: API and CLI tests for memory/Postgres turn flows.

## Tasks

### Task 1: Add Domain Types And Serde Coverage

**Files:**
- Modify: `crates/domain/src/state.rs`
- Modify: `crates/domain/tests/serde_roundtrip_tests.rs`

- [ ] **Step 1: Write failing serde tests**

Add tests for a world state without `action_resolutions` and for a delta containing:

```json
{
  "type": "recorded",
  "intent": "Disarm the assassin before he reaches Marta.",
  "stakes": ["Marta may be injured", "the crowd may panic"],
  "outcome": "success_with_cost",
  "consequence": "The assassin is stopped, but the alarm clock advances.",
  "visible_to_player": true,
  "linked_clock_ids": ["wedding"],
  "reason": "The player chose a risky public action."
}
```

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p domain action_resolution`

Expected: fails because action resolution types are missing.

- [ ] **Step 3: Add types**

Add:

```rust
pub struct ActionResolution {
    pub id: EntityKey,
    pub intent: String,
    pub stakes: Vec<String>,
    pub outcome: ActionOutcome,
    pub consequence: String,
    pub visible_to_player: bool,
    pub linked_clock_ids: Vec<EntityKey>,
}

pub enum ActionOutcome {
    Success,
    SuccessWithCost,
    Partial,
    Failure,
}

pub enum ActionResolutionChange {
    Recorded { intent: String, stakes: Vec<String>, outcome: ActionOutcome, consequence: String, visible_to_player: bool, linked_clock_ids: Vec<EntityKey>, reason: String },
}
```

Use serde snake_case. Add `#[serde(default)]` fields to `WorldState` and `WorldStateDelta`.

- [ ] **Step 4: Run domain tests**

Run: `cargo test -p domain`

Expected: all domain tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/domain/src/state.rs crates/domain/tests/serde_roundtrip_tests.rs
git commit -m "feat(domain): add action resolution state"
```

### Task 2: Validate And Reduce Action Resolutions

**Files:**
- Modify: `crates/engine/src/validation.rs`
- Modify: `crates/engine/src/reducer.rs`
- Modify: `crates/engine/src/projection.rs`

- [ ] **Step 1: Write failing validation tests**

Add tests:

```rust
rejects_action_resolution_without_stakes()
rejects_action_resolution_without_consequence()
rejects_action_resolution_with_unknown_linked_clock()
```

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p engine action_resolution`

Expected: fails because validator support is missing.

- [ ] **Step 3: Implement validation**

Require non-empty reason, intent, consequence, at least one stake, and known clock IDs for `linked_clock_ids`.

- [ ] **Step 4: Write reducer test**

Assert a recorded action resolution creates an ID like `action-<next_version>-<index>`, preserves visibility, and increments world-state version once.

- [ ] **Step 5: Implement reducer and changed entities**

Append to `state.action_resolutions` and add `action_resolution` refs in `changed_entities`.

- [ ] **Step 6: Run engine tests**

Run: `cargo test -p engine action_resolution`

Run: `cargo test -p engine`

Expected: all engine tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/engine/src/validation.rs crates/engine/src/reducer.rs crates/engine/src/projection.rs
git commit -m "feat(engine): apply action resolutions"
```

### Task 3: Update Prompt And Context Behavior

**Files:**
- Modify: `crates/engine/src/context.rs`
- Modify: `crates/engine/src/prompt.rs`
- Modify: `crates/engine/src/scene.rs`

- [ ] **Step 1: Write prompt test**

Assert an action prompt includes:

```text
ACTION RESOLUTION
intent
stakes
outcome
consequence
```

and that the output contract names `action_resolution_changes`.

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p engine action_prompt`

Expected: fails until prompt text is updated.

- [ ] **Step 3: Update directives**

For `TurnMode::Action` and `TacticalCombat`, add prompt instructions:

```text
For risky actions, include action_resolution_changes with explicit stakes, an outcome tier, and at least one visible consequence through facts, events, clocks, NPCs, factions, inventory, or location.
```

- [ ] **Step 4: Include recent resolutions**

Add recent visible action resolutions to `AgentContext` and render them in a compact section so the engine remembers unresolved costs.

- [ ] **Step 5: Run tests**

Run: `cargo test -p engine context prompt scene`

Expected: tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/engine/src/context.rs crates/engine/src/prompt.rs crates/engine/src/scene.rs
git commit -m "feat(engine): prompt structured action stakes"
```

### Task 4: Cover API Flows

**Files:**
- Modify: `crates/persistence/src/store.rs`
- Modify: `crates/api/tests/memory_api_flows.rs`
- Modify: `crates/api/tests/postgres_api_flows.rs`

- [ ] **Step 1: Seed initial state**

Set `action_resolutions: vec![]` in `initial_world_state` and any test world-state fixtures.

- [ ] **Step 2: Add memory flow**

Provider response should include a valid `action_resolution_changes` entry plus a clock advancement. Assert the turn response changed entities includes `action_resolution` and `clock`.

- [ ] **Step 3: Add Postgres flow**

Add ignored test that admin raw export includes the action resolution after persistence.

- [ ] **Step 4: Run tests**

Run: `cargo test -p api --test memory_api_flows action_resolution`

Run: `TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test postgres_api_flows action_resolution -- --ignored --test-threads=1`

Expected: memory and Postgres flows pass.

- [ ] **Step 5: Commit**

```bash
git add crates/persistence/src/store.rs crates/api/tests/memory_api_flows.rs crates/api/tests/postgres_api_flows.rs
git commit -m "feat(api): expose action resolution changes"
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

- Risky action turns can record structured stakes and outcomes.
- Validator rejects incomplete action resolution records.
- Reducer persists action resolutions and changed entities include them.
- Prompt instructions require visible consequences for risky actions.
- Existing non-action turns continue to work without action resolution records.

## Risks

- Over-structuring every action can make simple turns noisy; require this only for risky action mode or action-like scene styles.
- The LLM may produce action resolution without linked state changes; validation should require meaningful consequence.
- Existing mock provider responses must include default empty `action_resolution_changes` if serde defaults are not enough.

