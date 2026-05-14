# Secrets, Clues, And Discovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add clue and evidence tracking that can satisfy reveal conditions for secrets without directly leaking GM-only facts.

**Reveal-condition shape:** This plan replaces free-text reveal-condition matching with structured `ConditionRef` values. Each scenario `Secret` declares an ordered list of reveal conditions, and every clue references a `ConditionRef` by `id` plus a `MatchMode`. This is a hard requirement of this plan and must land before `.plan/gameplay/06-iron-archduke-scenario-mechanics.md` builds a behavioral fixture on top.

**Architecture:** Preserve `FactVisibility` as the secrecy boundary and add clue/evidence state that links player discoveries to GM-only secrets. Validation allows a secret to become player-known only when the delta cites a related secret ID and a clue whose `ConditionRef` matches one of the secret's declared reveal conditions.

**Tech Stack:** Rust, Cargo workspace, Axum, SQLx/Postgres, Clap CLI, serde, tokio tests

---

## Current State

- Scenario `Secret` has `id`, `text`, and `reveal_conditions: Vec<String>`. This plan replaces that field with `reveal_conditions: Vec<RevealCondition>` where each entry has an `id` and a free-text `description` (the description is GM-only narrative; matching never compares against it).
- Initial world state converts scenario secrets into GM-only facts.
- `FactToAdd` can carry `related_secret_ids` and `reveal_condition_satisfied`. This plan changes `reveal_condition_satisfied` from a free-text string to a `ConditionRef`.
- `BasicDeltaValidator` rejects player-known facts that duplicate GM-only facts unless a related secret ID and proof are present.
- There is no structured clue/evidence inventory showing how reveal conditions were satisfied.

## Target Behavior

- World state tracks clues/evidence discovered by the player.
- Clues can link to one or more secret IDs and reference one or more `ConditionRef` values pointing at structured `RevealCondition` entries on those secrets.
- Validator accepts player-known secret reveals only when at least one linked clue's `ConditionRef` matches a `RevealCondition.id` declared on the targeted secret.
- Projection shows discovered clues without GM-only secret text.
- Prompt rendering helps the model use discovered clues without revealing undiscovered secrets.

## File Structure

- Modify: `crates/domain/src/scenario.rs`
  - Replace `Secret.reveal_conditions: Vec<String>` with `Vec<RevealCondition>` (`{ id: EntityKey, description: String }`).
- Modify: `crates/domain/src/state.rs`
  - Add `ClueState`, `ClueVisibility`, `ClueChange`, `ConditionRef`, and `MatchMode`.
  - Add `clues: Vec<ClueState>` and `clue_changes: Vec<ClueChange>`.
  - Change `FactToAdd.reveal_condition_satisfied: Option<String>` to `Option<ConditionRef>`.
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
  "satisfied_reveal_conditions": [
    { "id": "inspect-treaty-seal", "mode": "exact" }
  ],
  "visible_to_player": true,
  "reason": "The player inspected the document."
}
```

Also round-trip a `Secret` with the new `reveal_conditions` shape:

```json
{
  "id": "poisoned-treaty",
  "text": "The chancellor poisoned the treaty.",
  "reveal_conditions": [
    { "id": "inspect-treaty-seal", "description": "Player physically inspects the treaty seal." },
    { "id": "interrogate-chancellor", "description": "Player extracts a confession from the chancellor." }
  ]
}
```

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p domain clue`

Expected: fails until clue types exist.

- [ ] **Step 3: Add types**

Add:

```rust
pub struct RevealCondition {
    pub id: EntityKey,
    pub description: String,
}

pub struct ConditionRef {
    pub id: EntityKey,
    pub mode: MatchMode,
}

pub enum MatchMode {
    Exact,
}

pub struct ClueState {
    pub id: EntityKey,
    pub text: String,
    pub linked_secret_ids: Vec<EntityKey>,
    pub satisfied_reveal_conditions: Vec<ConditionRef>,
    pub visible_to_player: bool,
}

pub enum ClueChange {
    Discovered { clue_id: EntityKey, text: String, linked_secret_ids: Vec<EntityKey>, satisfied_reveal_conditions: Vec<ConditionRef>, visible_to_player: bool, reason: String },
    VisibilityChanged { clue_id: EntityKey, visible_to_player: bool, reason: String },
}
```

`MatchMode::Exact` is the only mode in this plan: a `ConditionRef.id` matches a `RevealCondition.id` iff the strings are byte-equal. Future plans may add `Normalized` or `ContainsAll` modes; do not add them speculatively here.

Change `Secret.reveal_conditions` in `crates/domain/src/scenario.rs` to `Vec<RevealCondition>` and migrate `samples/*.json` (including `bride-of-the-iron-archduke.json`) to the new shape in the same commit so domain serde tests stay green.

Change `FactToAdd.reveal_condition_satisfied` to `Option<ConditionRef>`.

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

Require:

- every entry in `linked_secret_ids` matches an existing scenario `Secret.id` (or appears in the current delta's secret additions, if the project ever supports adding secrets mid-session — out of scope for this plan);
- every entry in `satisfied_reveal_conditions` is a `ConditionRef` whose `id` matches a `RevealCondition.id` declared on at least one of the linked secrets;
- non-empty `text`;
- non-empty `reason`;
- at least one `ConditionRef` in `satisfied_reveal_conditions` when `linked_secret_ids` is non-empty.

Unknown `ConditionRef.id` values must be rejected with a `ValidationError::UnknownRevealCondition { clue_id, condition_id }`.

- [ ] **Step 4: Validate fact reveal proof through clues**

When `FactToAdd.visibility == PlayerKnown` and `related_secret_ids` is non-empty, accept `reveal_condition_satisfied: Some(condition_ref)` only if:

1. Existing `world_state.clues` or current delta `clue_changes` contains a clue whose `linked_secret_ids` covers every entry of `related_secret_ids`.
2. That clue's `satisfied_reveal_conditions` contains a `ConditionRef` equal (by `id` under `MatchMode::Exact`) to `condition_ref`.
3. The cited `condition_ref.id` matches a `RevealCondition.id` declared on every secret in `related_secret_ids`.
4. The clue is visible to the player or becomes visible in the same delta.

If `reveal_condition_satisfied` is `None`, the fact reveal is rejected. There is no free-text fallback.

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
    pub satisfied_reveal_conditions: Vec<ConditionRef>,
}
```

`ConditionRef` is safe to project to the player: `id` is a stable scenario-author identifier, and `mode` is a tiny enum with no GM-only payload. The associated `RevealCondition.description` lives on the `Secret` (GM-only) and is never projected.

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

- `ConditionRef` removes the free-text brittleness but shifts the authoring burden: scenario authors must declare every reveal condition on each `Secret` up front. Document this in the scenario authoring docs touched by `.plan/features/01-scenario-authoring-cli.md`.
- Clue text itself can leak a secret if authored carelessly; validation cannot fully solve bad authoring.
- Multi-turn reveal proof must check existing clues as well as same-turn clue changes.
- Migration of `samples/*.json` to the new `RevealCondition` shape must land in the same commit as the domain type change, or the sample-validation tests will break.

