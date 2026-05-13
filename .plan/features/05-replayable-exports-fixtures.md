# Replayable Exports Fixtures Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn exported sessions into replayable regression fixtures that can prove future engine changes preserve important session behavior.

**Architecture:** Define a versioned replay fixture format made of scenario, initial state, player inputs, mocked provider responses, expected visible responses, expected deltas, and expected final projections. Add a test harness that replays fixtures through the same `DefaultTurnPipeline` and API paths used in real sessions.

**Tech Stack:** Rust, Cargo workspace, Axum, SQLx/Postgres, Clap CLI, serde, tokio tests

---

## Current State

- `GET /sessions/:id/export` returns session, player-visible state, and events.
- `/admin/sessions/:id/export/raw` returns session, raw world state, and events.
- `debug_turn` returns `applied_delta`.
- `crates/api/tests/behavioral_fixtures.rs` hard-codes several provider outputs and assertions for scenario-level behavior.
- `providers::MockProvider` can replay queued responses.
- There is no stable export format that can be fed back into tests.

## Target Behavior

- A fixture JSON file can replay a sequence of turns through the engine using mock provider responses.
- Export tooling can produce a fixture draft from a session, with sensitive raw fields included only from admin export.
- Behavioral fixtures can move from large embedded strings toward versioned JSON files.
- Replay tests compare expected visible response substrings, world-state version, changed entities, selected raw state assertions, and final visible projection.

## File Structure

- Create: `crates/api/tests/fixtures/replay/`
  - Store JSON replay fixtures.
- Create: `crates/api/tests/common/replay.rs`
  - Fixture structs and replay harness.
- Modify: `crates/api/tests/behavioral_fixtures.rs`
  - Add replay tests that call the harness.
- Modify: `crates/api/src/app.rs`
  - Optional export route changes if fixture export is exposed through admin API.
- Modify: `crates/cli/src/commands/session.rs`
  - Optional `session export-fixture` command for local fixture generation.

## Tasks

### Task 1: Define Replay Fixture Format

**Files:**
- Create: `crates/api/tests/common/replay.rs`
- Create: `crates/api/tests/fixtures/replay/guildhall-flood.json`

- [ ] **Step 1: Write failing replay test**

Add a test in `behavioral_fixtures.rs`:

```rust
#[tokio::test]
async fn replay_guildhall_flood_fixture() {
    replay::run_fixture(include_str!("fixtures/replay/guildhall-flood.json"))
        .await
        .expect("fixture replays");
}
```

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p api --test behavioral_fixtures replay_guildhall_flood_fixture -- --ignored --test-threads=1`

Expected: fails because the replay module and fixture file do not exist.

- [ ] **Step 3: Add fixture structs**

In `common/replay.rs`, define:

```rust
pub struct ReplayFixture {
    pub version: u32,
    pub name: String,
    pub scenario: domain::Scenario,
    pub turns: Vec<ReplayTurn>,
    pub expected_final: ExpectedFinalState,
}

pub struct ReplayTurn {
    pub input: String,
    pub mode: Option<domain::TurnMode>,
    pub provider_response: serde_json::Value,
    pub expected_response_contains: Vec<String>,
    pub expected_delta: Option<domain::WorldStateDelta>,
}

pub struct ExpectedFinalState {
    pub world_state_version: i64,
    pub visible_fact_contains: Vec<String>,
    pub hidden_fact_ids_absent_from_projection: Vec<String>,
}
```

- [ ] **Step 4: Add first fixture JSON**

Use the existing flood guildhall provider response and scenario from `common::sample_scenario()`. Include one turn with expected response substring `guildhall erupts into panic`, final version `1`, visible fact substring `dangerous level`, and hidden ID `void-mark`.

- [ ] **Step 5: Commit**

```bash
git add crates/api/tests/common/replay.rs crates/api/tests/fixtures/replay/guildhall-flood.json crates/api/tests/behavioral_fixtures.rs
git commit -m "test: define replay fixture format"
```

### Task 2: Implement Replay Harness

**Files:**
- Modify: `crates/api/tests/common/replay.rs`
- Modify: `crates/api/tests/behavioral_fixtures.rs`

- [ ] **Step 1: Build replay flow**

In `run_fixture`, parse JSON, create a Postgres test context with `MockProvider` responses from fixture turns, create scenario/session through API routes, submit each turn, and assert response substrings.

- [ ] **Step 2: Compare expected delta when present**

When `expected_delta` exists, call admin debug route instead of normal turn for that fixture turn, or compare persisted delta through raw timeline after the timeline debugger plan lands. For the first implementation, use debug route behind admin config.

- [ ] **Step 3: Compare final projection**

Call `GET /sessions/:id/export` and assert final visible state version and visible facts. Assert hidden fact IDs are absent from `player_known_facts`.

- [ ] **Step 4: Run replay test**

Run: `TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test behavioral_fixtures replay_guildhall_flood_fixture -- --ignored --test-threads=1`

Expected: replay fixture passes.

- [ ] **Step 5: Commit**

```bash
git add crates/api/tests/common/replay.rs crates/api/tests/behavioral_fixtures.rs
git commit -m "test: replay exported session fixtures"
```

### Task 3: Add Fixture Export Command

**Files:**
- Modify: `crates/cli/src/commands/session.rs`
- Modify: `crates/cli/tests/cli_smoke.rs`

- [ ] **Step 1: Write failing CLI test**

Add a smoke test for:

```bash
cargo run -p cli -- session export-fixture <SESSION_ID> --name "smoke"
```

Assert the output is JSON with `version`, `name`, `scenario`, `turns`, and `expected_final`.

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p cli --test cli_smoke export_fixture`

Expected: fails because command is missing.

- [ ] **Step 3: Implement command**

Add:

```rust
ExportFixture {
    session_id: Uuid,
    #[arg(long)]
    name: String,
}
```

For the first version, export scenario, current visible final state, and an empty `turns` array with a clear `source_session_id` field. The replay harness can already consume fully authored fixtures; this command creates a draft.

- [ ] **Step 4: Run test**

Run: `cargo test -p cli --test cli_smoke export_fixture`

Expected: command emits parseable JSON.

- [ ] **Step 5: Commit**

```bash
git add crates/cli/src/commands/session.rs crates/cli/tests/cli_smoke.rs
git commit -m "feat(cli): export replay fixture drafts"
```

### Task 4: Convert One Behavioral Fixture

**Files:**
- Modify: `crates/api/tests/behavioral_fixtures.rs`
- Create: `crates/api/tests/fixtures/replay/secret-leak-prevention.json`

- [ ] **Step 1: Add secret-leak fixture**

Create a fixture where provider response attempts to add a player-known fact matching the GM-only `void-mark` text without reveal proof. Expected result should be an error status `422` and no world-state version increment.

- [ ] **Step 2: Extend fixture format for expected errors**

Add to `ReplayTurn`:

```rust
pub expected_status: Option<u16>
```

Default to `200` when absent.

- [ ] **Step 3: Replay error fixture**

Add a test `replay_secret_leak_prevention_fixture` and assert status `422` plus final version `0`.

- [ ] **Step 4: Run tests**

Run: `TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test behavioral_fixtures replay_ -- --ignored --test-threads=1`

Expected: both replay fixtures pass.

- [ ] **Step 5: Commit**

```bash
git add crates/api/tests/behavioral_fixtures.rs crates/api/tests/common/replay.rs crates/api/tests/fixtures/replay
git commit -m "test: replay secret leak fixture"
```

## Verification

Run:

```bash
cargo test -p api --test memory_api_flows
cargo test -p cli --test cli_smoke export_fixture
TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test behavioral_fixtures replay_ -- --ignored --test-threads=1
```

## Acceptance Criteria

- Replay fixture format is versioned and documented by structs.
- At least one successful behavioral fixture and one rejected-secret fixture replay through the harness.
- CLI can export a fixture draft from a session.
- Replay tests assert final visible projection and hidden fact absence.
- Existing behavioral fixture tests continue to run while migration to JSON fixtures proceeds incrementally.

## Risks

- Exported raw provider output can contain sensitive data; fixture draft export should avoid raw provider output unless explicitly admin-only.
- Replay comparisons can become brittle if exact prose changes. Prefer substrings and state assertions over full response equality.
- Fixture format versioning must be explicit from the first file to avoid breaking stored fixtures silently.

