# State Delta Extensibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add shared fixture builders and reducer/validator helper patterns so new `WorldStateDelta` variants require fewer repeated test edits.

**Architecture:** Keep domain types explicit, but reduce test boilerplate through crate-local builders. Preserve serde compatibility and validation discipline while making new delta variants cheaper to test across domain, engine, API, and persistence layers.

**Tech Stack:** Rust, Cargo workspace, Axum, SQLx/Postgres, Clap CLI, serde, tokio tests

---

## Current State

- `domain::WorldStateDelta` is a single struct with vectors for typed changes.
- `crates/engine/src/validation.rs`, `reducer.rs`, and `projection.rs` each match over many delta variants.
- Engine tests repeat large `Scenario` and `WorldState` literals.
- API tests have `crates/api/tests/common/mod.rs` with `sample_scenario` and HTTP helpers.
- Persistence tests define separate `sample_scenario`, `sample_world_state`, `sample_message`, and `sample_provider` helpers.
- Adding a new delta family currently requires edits across domain serde tests, validation, reducer, projection, API response tests, persistence JSON round-trips, and fixtures.

## Target Behavior

- Shared test builders create minimal valid scenarios, world states, facts, NPCs, factions, quests, clocks, relationships, and deltas.
- Validator, reducer, and projection tests use these builders instead of long literals.
- Adding a new delta variant has a documented checklist and a smaller set of repeated edits.
- No production behavior changes.

## File Structure

- Create: `crates/domain/src/fixtures.rs` gated by `#[cfg(any(test, feature = "test-fixtures"))]`
  - Minimal builders for domain tests and downstream integration tests.
- Modify: `crates/domain/Cargo.toml`
  - Add a `test-fixtures` feature if downstream crates need builders outside unit tests.
- Modify: `crates/domain/src/lib.rs`
  - Re-export fixtures only under the gate.
- Modify: `crates/engine/src/validation.rs`, `reducer.rs`, `projection.rs`
  - Replace repeated literals in tests with builders.
- Modify: `crates/api/tests/common/mod.rs`
  - Use domain fixtures or wrap them with API-specific defaults.
- Modify: `crates/persistence/tests/repository_tests.rs`
  - Use shared scenario/world-state builders where practical.

## Tasks

### Task 1: Add Domain Fixture Builders

**Files:**
- Create: `crates/domain/src/fixtures.rs`
- Modify: `crates/domain/src/lib.rs`
- Modify: `crates/domain/Cargo.toml`

- [ ] **Step 1: Write failing domain fixture test**

Add a domain unit test that uses the planned API:

```rust
#[test]
fn fixture_builders_create_valid_scenario_and_state() {
    let scenario = fixtures::scenario().with_secret("void-mark", "Hidden truth").build();
    validate_scenario(&scenario).expect("fixture scenario validates");

    let state = fixtures::world_state(&scenario).build();
    assert_eq!(state.scenario_id, scenario.id);
    assert!(state.facts.iter().any(|fact| fact.id == "void-mark"));
}
```

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p domain fixture_builders_create_valid_scenario_and_state`

Expected: fails because `fixtures` does not exist.

- [ ] **Step 3: Implement minimal builders**

Create builders with these defaults:

```rust
pub fn scenario() -> ScenarioBuilder;
pub fn world_state(scenario: &Scenario) -> WorldStateBuilder;
pub fn empty_delta() -> WorldStateDelta;
```

The default scenario should include one visible location `guildhall`, one active NPC `examiner`, one faction `guild`, one quest `register`, one clock `fame`, and no secrets until `.with_secret` is called.

- [ ] **Step 4: Gate fixture exports**

In `Cargo.toml`:

```toml
[features]
test-fixtures = []
```

In `lib.rs`:

```rust
#[cfg(any(test, feature = "test-fixtures"))]
pub mod fixtures;
```

- [ ] **Step 5: Run domain tests**

Run: `cargo test -p domain`

Expected: all domain tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/domain
git commit -m "test: add domain fixture builders"
```

### Task 2: Use Fixtures In Engine Tests

**Files:**
- Modify: `crates/engine/Cargo.toml`
- Modify: `crates/engine/src/validation.rs`
- Modify: `crates/engine/src/reducer.rs`
- Modify: `crates/engine/src/projection.rs`
- Modify: `crates/engine/src/context.rs`

- [ ] **Step 1: Enable fixture feature for engine dev builds**

In `crates/engine/Cargo.toml`, change the dev or normal domain dependency used by tests to include:

```toml
domain = { path = "../domain", features = ["test-fixtures"] }
```

Use the dependency section style already present in the file.

- [ ] **Step 2: Replace one validation fixture**

Change `validation.rs` tests to build scenario/state through fixtures while keeping the exact assertions for secret leaks, unknown IDs, clock range, faction range, and NPC status behavior.

- [ ] **Step 3: Replace one reducer fixture family**

Change repeated `minimal_npc_state`, `minimal_faction_state`, `minimal_quest_state`, and `minimal_clock_state` helpers to use the builder and customize only the relevant entity.

- [ ] **Step 4: Replace one projection fixture**

Use fixture state for `projection_filters_gm_only_facts_for_normal_viewers` and `admin_projection_includes_gm_only_facts`.

- [ ] **Step 5: Run engine tests**

Run: `cargo test -p engine`

Expected: all engine tests pass with smaller literals.

- [ ] **Step 6: Commit**

```bash
git add crates/engine
git commit -m "test: reuse domain fixtures in engine tests"
```

### Task 3: Use Fixtures In API And Persistence Tests

**Files:**
- Modify: `crates/api/Cargo.toml`
- Modify: `crates/api/tests/common/mod.rs`
- Modify: `crates/persistence/Cargo.toml`
- Modify: `crates/persistence/tests/repository_tests.rs`

- [ ] **Step 1: Enable fixture feature in integration-test crates**

Add `features = ["test-fixtures"]` to the `domain` dependency where tests compile against the domain crate.

- [ ] **Step 2: Update API sample helper**

Keep `common::sample_scenario()` as the stable API test helper, but implement it using `domain::fixtures::scenario()` and add the existing secret, faction, NPC, quest, and clock details that API tests assert against.

- [ ] **Step 3: Update persistence helpers**

Replace standalone `sample_scenario()` and `sample_world_state()` bodies in `repository_tests.rs` with fixture builders plus local customization for persistence-specific IDs and messages.

- [ ] **Step 4: Run affected tests**

Run: `cargo test -p api --test memory_api_flows`

Run: `cargo test -p persistence --test repository_tests create_and_get_scenario -- --ignored --test-threads=1`

Expected: memory tests pass; ignored persistence test passes when Docker-backed Postgres is available.

- [ ] **Step 5: Commit**

```bash
git add crates/api crates/persistence
git commit -m "test: share fixtures across integration tests"
```

### Task 4: Add Delta Extension Checklist

**Files:**
- Create: `crates/domain/DELTA_EXTENSION.md`

- [ ] **Step 1: Write checklist**

Create a short checklist with these sections:

```markdown
# Delta Extension Checklist

1. Add domain type or enum variant in `state.rs`.
2. Add serde round-trip coverage in `tests/serde_roundtrip_tests.rs`.
3. Add validation rules in `engine/src/validation.rs`.
4. Add reducer behavior in `engine/src/reducer.rs`.
5. Add projection or changed-entity behavior in `engine/src/projection.rs`.
6. Add API memory flow coverage when response shape or visible projection changes.
7. Add Postgres coverage when persistence or raw export behavior changes.
8. Update prompt output contract in `engine/src/prompt.rs`.
9. Update scenario sample or template only when authoring format changes.
```

- [ ] **Step 2: Link from crate README**

Add one sentence in `crates/domain/README.md` pointing to `DELTA_EXTENSION.md`.

- [ ] **Step 3: Run docs search**

Run: `rg -n "Delta Extension Checklist|DELTA_EXTENSION" crates/domain`

Expected: checklist and README link are present.

- [ ] **Step 4: Commit**

```bash
git add crates/domain/DELTA_EXTENSION.md crates/domain/README.md
git commit -m "docs: document delta extension workflow"
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
TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p persistence --test repository_tests -- --ignored --test-threads=1
```

## Acceptance Criteria

- Domain fixture builders are available to tests without leaking into normal production API unless the feature is enabled.
- Engine/API/persistence tests use shared fixture builders for common scenario and world-state setup.
- Adding new delta variants requires fewer repeated literal updates.
- The delta extension checklist names every production and test surface that usually changes.
- No production behavior changes.

## Risks

- Fixture builders can hide important domain details; keep defaults explicit and override-friendly.
- Feature-gated test helpers can complicate Cargo dependency declarations; verify with `cargo check --workspace`.
- Over-normalizing fixtures can reduce scenario variety; preserve specialized fixtures for behavior-specific tests.

