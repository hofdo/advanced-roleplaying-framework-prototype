# Secrets, Clues, And Discovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add clue and evidence tracking that can satisfy reveal conditions for secrets without directly leaking GM-only facts.

**Architecture:** Preserve `FactVisibility` as the secrecy boundary and add clue/evidence state that links player discoveries to GM-only secrets. Validation should allow a secret to become player-known only when the delta cites a related secret ID and a satisfied clue or evidence condition.

**Tech Stack:** Rust, Cargo workspace, Axum, SQLx/Postgres, Clap CLI, serde, tokio tests

---

## Current State

- Scenario `Secret` has `id`, `text`, and `reveal_conditions`.
- Initial world state converts scenario secrets into GM-only facts.
- `FactToAdd` can carry `related_secret_ids` and `reveal_condition_satisfied`.
- `BasicDeltaValidator` rejects player-known facts that duplicate GM-only facts unless a related secret ID and proof are present.
- There is no structured clue/evidence inventory showing how reveal conditions were satisfied.

## Target Behavior

- World state tracks clues/evidence discovered by the player.
- Clues can link to one or more secret IDs and satisfy named reveal conditions.
- Validator accepts player-known secret reveals only when linked clue/evidence satisfies the condition.
- Projection shows discovered clues without GM-only secret text.
- Prompt rendering helps the model use discovered clues without revealing undiscovered secrets.

## File Structure

- Modify: `crates/domain/src/state.rs`
  - Add `ClueState`, `ClueVisibility`, and `ClueChange`.
  - Add `clues: Vec<ClueState>` and `clue_changes: Vec<ClueChange>`.
- Modify: `crates/engine/src/validation.rs`
  - Check reveal proof against clue state.
- Modify: `crates/engine/src/reducer.rs`
  - Add and reveal clues.
- Modify: `crates/engine/src/projection.rs`
  - Project discovered clues.
- Modify: `crates/engine/src/context.rs`, `crates/engine/src/prompt.rs`
  - Render clues separately from GM-only facts.
- Modify: `crates/api/tests/behavioral_fixtures.rs`
  - Add a fixture proving clue-gated reveal.

## Tasks

### Task 1: Add Clue Domain State

**Files:**
- Modify: `crates/domain/src/state.rs`
- Modify: `crates/domain/tests/serde_roundtrip_tests.rs`

- [ ] **Step 1: Write failing serde tests**

Add compatibility test for missing `clues` defaulting to empty and round-trip a clue delta:

```json
{
  "type": "discovered",
  "clue_id": "silver-seal-residue",
  "text": "The treaty seal smells of bitter almond.",
  "linked_secret_ids": ["poisoned-treaty"],
  "satisfied_reveal_conditions": ["inspect the treaty seal"],
  "visible_to_player": true,
  "reason": "The player inspected the document."
}
```

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p domain clue`

Expected: fails until clue types exist.

- [ ] **Step 3: Add types**

Add:

```rust
pub struct ClueState {
    pub id: EntityKey,
    pub text: String,
    pub linked_secret_ids: Vec<EntityKey>,
    pub satisfied_reveal_conditions: Vec<String>,
    pub visible_to_player: bool,
}

pub enum ClueChange {
    Discovered { clue_id: EntityKey, text: String, linked_secret_ids: Vec<EntityKey>, satisfied_reveal_conditions: Vec<String>, visible_to_player: bool, reason: String },
    VisibilityChanged { clue_id: EntityKey, visible_to_player: bool, reason: String },
}
```

Add default fields to state and delta.

- [ ] **Step 4: Run domain tests**

Run: `cargo test -p domain`

Expected: tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/domain/src/state.rs crates/domain/tests/serde_roundtrip_tests.rs
git commit -m "feat(domain): add clue discovery state"
```

### Task 2: Validate Clues And Secret Reveals

**Files:**
- Modify: `crates/engine/src/validation.rs`

- [ ] **Step 1: Write validation tests**

Add tests:

```rust
accepts_secret_reveal_when_discovered_clue_satisfies_condition()
rejects_secret_reveal_when_related_clue_missing()
rejects_clue_with_unknown_secret_id()
rejects_clue_without_reveal_condition()
```

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p engine clue secret_reveal`

Expected: tests fail until validation uses clues.

- [ ] **Step 3: Validate clue changes**

Require known secret IDs for `linked_secret_ids`, non-empty text, non-empty reason, and at least one satisfied reveal condition when linked to a secret.

- [ ] **Step 4: Validate fact reveal proof through clues**

When `FactToAdd.visibility == PlayerKnown` and `related_secret_ids` is non-empty, accept `reveal_condition_satisfied` only if:

1. Existing `world_state.clues` or current delta `clue_changes` links to that secret ID.
2. A satisfied clue condition matches the proof string or the secret's reveal condition.
3. The clue is visible to the player or becomes visible in the same delta.

- [ ] **Step 5: Run tests**

Run: `cargo test -p engine validation::tests::accepts_secret_reveal_when_discovered_clue_satisfies_condition`

Run: `cargo test -p engine`

Expected: secret reveal and existing secret-leak tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/engine/src/validation.rs
git commit -m "feat(engine): validate clue-gated reveals"
```

### Task 3: Reduce, Project, And Prompt Clues

**Files:**
- Modify: `crates/engine/src/reducer.rs`
- Modify: `crates/engine/src/projection.rs`
- Modify: `crates/engine/src/context.rs`
- Modify: `crates/engine/src/prompt.rs`
- Modify: `crates/domain/src/state.rs`

- [ ] **Step 1: Write reducer tests**

Assert discovered clues are inserted or replaced by `clue_id`, visibility changes update the existing clue, and version increments once.

- [ ] **Step 2: Add visible clue DTO**

Add:

```rust
pub struct VisibleClue {
    pub id: EntityKey,
    pub text: String,
    pub satisfied_reveal_conditions: Vec<String>,
}
```

Add `visible_clues` to `FrontendVisibleState`.

- [ ] **Step 3: Write projection test**

Assert player projection includes visible clues and excludes invisible clues. Admin projection can include all clues if `ViewerContext.is_admin`.

- [ ] **Step 4: Add context and prompt rendering**

Render visible clues in narration-safe context. Render invisible or GM-only clue links only in oracle context if the non-streaming secrecy split exists.

- [ ] **Step 5: Run tests**

Run: `cargo test -p engine clue`

Expected: clue reducer, projection, context, and prompt tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/domain/src/state.rs crates/engine/src/reducer.rs crates/engine/src/projection.rs crates/engine/src/context.rs crates/engine/src/prompt.rs
git commit -m "feat(engine): render discovered clues"
```

### Task 4: Cover Behavioral Fixture

**Files:**
- Modify: `crates/persistence/src/store.rs`
- Modify: `crates/api/tests/behavioral_fixtures.rs`

- [ ] **Step 1: Seed initial clues**

Set `clues: vec![]` in `initial_world_state` and test fixtures.

- [ ] **Step 2: Add fixture test**

Create an ignored Postgres behavioral test:

1. First turn discovers a clue linked to `void-mark`.
2. Second turn reveals the player-known fact using `related_secret_ids: ["void-mark"]` and matching `reveal_condition_satisfied`.
3. Projection includes the player-known reveal and visible clue.

- [ ] **Step 3: Run fixture**

Run: `TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test behavioral_fixtures clue_gated_secret_reveal -- --ignored --test-threads=1`

Expected: fixture passes.

- [ ] **Step 4: Commit**

```bash
git add crates/persistence/src/store.rs crates/api/tests/behavioral_fixtures.rs
git commit -m "feat(gameplay): gate secret reveals through clues"
```

## Verification

Run:

```bash
cargo test -p domain
cargo test -p engine
cargo test -p api --test memory_api_flows
TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test behavioral_fixtures clue_gated_secret_reveal -- --ignored --test-threads=1
```

## Acceptance Criteria

- Clues/evidence are first-class world state.
- Player-known secret reveals require linked clue proof.
- Visible projection shows clues without leaking undiscovered GM-only secret text.
- Prompt context separates discovered clues from hidden facts.
- Existing secret-leak rejection tests remain active.

## Risks

- Matching reveal conditions by free text can be brittle. Normalize strings or compare against secret IDs plus explicit clue condition lists.
- Clue text itself can leak a secret if authored carelessly; validation cannot fully solve bad authoring.
- Multi-turn reveal proof must check existing clues as well as same-turn clue changes.

