# Session Timeline Debugger Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Improve turn, event, and delta inspection so developers can understand exactly how a session evolved.

**Architecture:** Add read-only timeline APIs backed by existing messages, events, deltas, and world-state versions. The API should expose player-safe timeline data by default and raw/admin details only through admin routes or CLI admin mode.

**Tech Stack:** Rust, Cargo workspace, Axum, SQLx/Postgres, Clap CLI, serde, tokio tests

---

## Current State

- `GET /sessions/:id/events` returns `Vec<EventRecord>`.
- Admin debug turn returns `applied_delta`.
- Postgres persists `messages`, `world_state_deltas`, `events`, and `world_states`.
- Memory store keeps `messages` and `events`, but does not expose message or delta history through `ApplicationStore`.
- CLI `world` prints current projected/raw state, and `chat` has `/world` and `/status`.
- `crates/api/tests/postgres_api_flows.rs` already asserts delta rows and event rows after turns.

## Target Behavior

- `GET /sessions/:id/timeline` returns ordered public timeline entries: user message, assistant message, world events, pipeline milestones, and visible state version changes.
- `GET /admin/sessions/:id/timeline/raw` returns messages, deltas, raw event records, and raw provider output when storage is configured to keep it.
- `rp session timeline <SESSION_ID>` prints a concise timeline.
- `rp session timeline <SESSION_ID> --admin` prints raw debug detail.
- Timeline endpoints are read-only and do not mutate session state.

## File Structure

- Modify: `crates/persistence/src/repositories.rs`
  - Add message and delta listing repository methods for Postgres.
- Modify: `crates/persistence/src/store.rs`
  - Add timeline methods to `ApplicationStore` and memory/Postgres implementations.
- Modify: `crates/api/src/app.rs`
  - Add public and admin timeline routes.
- Modify: `crates/cli/src/commands/session.rs`
  - Add `timeline` subcommand.
- Modify: `crates/cli/src/render.rs`
  - Add terminal renderer for timeline entries.
- Modify: `crates/api/tests/memory_api_flows.rs`, `crates/api/tests/postgres_api_flows.rs`, and `crates/cli/tests/cli_smoke.rs`
  - Cover public/admin behavior.

## Tasks

### Task 1: Add Timeline Domain DTOs

**Files:**
- Modify: `crates/persistence/src/repositories.rs`
- Modify: `crates/persistence/src/store.rs`

- [ ] **Step 1: Define read models**

Add serializable structs in `persistence`:

```rust
pub struct TimelineEntry {
    pub kind: String,
    pub description: String,
    pub message_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub world_state_version: Option<i64>,
}

pub struct RawTimeline {
    pub session: SessionRecord,
    pub messages: Vec<MessageRecord>,
    pub deltas: Vec<WorldStateDeltaRecord>,
    pub events: Vec<EventRecord>,
}

pub struct WorldStateDeltaRecord {
    pub id: Uuid,
    pub session_id: SessionId,
    pub message_id: Option<Uuid>,
    pub delta: domain::WorldStateDelta,
    pub validation_status: String,
}
```

- [ ] **Step 2: Write failing memory store test**

Add a store test that persists a successful turn and then calls `timeline(session_id)`, expecting user and assistant entries plus a world event entry.

- [ ] **Step 3: Run expected failing command**

Run: `cargo test -p persistence timeline`

Expected: fails because timeline methods do not exist.

- [ ] **Step 4: Extend ApplicationStore**

Add:

```rust
async fn timeline(&self, session_id: SessionId) -> Result<Vec<TimelineEntry>, TurnPipelineError>;
async fn raw_timeline(&self, session_id: SessionId) -> Result<Option<RawTimeline>, TurnPipelineError>;
```

Implement memory timeline from in-memory messages and events. For memory raw timeline, include messages/events and an empty deltas vector unless memory delta storage is added.

- [ ] **Step 5: Run persistence unit tests**

Run: `cargo test -p persistence timeline`

Expected: memory timeline tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/persistence/src/store.rs crates/persistence/src/repositories.rs
git commit -m "feat(persistence): add session timeline read models"
```

### Task 2: Implement Postgres Timeline Queries

**Files:**
- Modify: `crates/persistence/src/repositories.rs`
- Modify: `crates/persistence/tests/repository_tests.rs`

- [ ] **Step 1: Write ignored repository test**

Add `list_world_state_deltas_returns_applied_deltas` and `raw_timeline_includes_messages_deltas_events`. Persist a successful turn, then assert one delta, two messages, and at least one event.

- [ ] **Step 2: Run expected failing command**

Run: `TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p persistence --test repository_tests raw_timeline -- --ignored --test-threads=1`

Expected: fails because queries are missing.

- [ ] **Step 3: Add repository query**

Add `WorldStateDeltaRepository::list(&PgPersistence, session_id)` that queries:

```sql
SELECT id, session_id, message_id, delta, validation_status
FROM world_state_deltas
WHERE session_id = $1
ORDER BY created_at
```

Add message listing for all messages ordered by `created_at`.

- [ ] **Step 4: Implement Postgres store methods**

Build `timeline` from messages and events; build `raw_timeline` from session, messages, deltas, and events.

- [ ] **Step 5: Run ignored repository tests**

Run: `TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p persistence --test repository_tests timeline -- --ignored --test-threads=1`

Expected: timeline tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/persistence/src/repositories.rs crates/persistence/src/store.rs crates/persistence/tests/repository_tests.rs
git commit -m "feat(persistence): query session timelines"
```

### Task 3: Add API Routes

**Files:**
- Modify: `crates/api/src/app.rs`
- Modify: `crates/api/tests/memory_api_flows.rs`
- Modify: `crates/api/tests/postgres_api_flows.rs`

- [ ] **Step 1: Write memory API test**

Add test `session_timeline_lists_turn_entries`. Create scenario/session, run one turn, call `GET /sessions/:id/timeline`, and assert JSON array contains `user_message`, `assistant_message`, and `world_event` kinds.

- [ ] **Step 2: Write admin raw route test**

Enable admin config, run one turn, call `/admin/sessions/:id/timeline/raw` with bearer token, and assert `messages`, `deltas`, and `events` fields exist. In memory mode, accept empty `deltas`.

- [ ] **Step 3: Run expected failing command**

Run: `cargo test -p api --test memory_api_flows timeline`

Expected: fails because routes are missing.

- [ ] **Step 4: Add routes**

Add:

```rust
.route("/sessions/:session_id/timeline", get(get_timeline))
.route("/admin/sessions/:session_id/timeline/raw", get(get_raw_timeline))
```

Protect raw route with the existing admin route layer.

- [ ] **Step 5: Run API tests**

Run: `cargo test -p api --test memory_api_flows timeline`

Run: `TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test postgres_api_flows timeline -- --ignored --test-threads=1`

Expected: memory and Postgres timeline tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/app.rs crates/api/tests/memory_api_flows.rs crates/api/tests/postgres_api_flows.rs
git commit -m "feat(api): expose session timelines"
```

### Task 4: Add CLI Timeline Command

**Files:**
- Modify: `crates/cli/src/commands/session.rs`
- Modify: `crates/cli/src/render.rs`
- Modify: `crates/cli/tests/cli_smoke.rs`

- [ ] **Step 1: Write CLI smoke test**

Add a smoke test that creates a scenario/session, runs a turn with a mock provider, and runs:

```bash
cargo run -p cli -- session timeline <SESSION_ID>
```

Assert stdout contains the assistant response and a world event entry.

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p cli --test cli_smoke session_timeline`

Expected: fails because the command is missing.

- [ ] **Step 3: Implement command**

Add to `Cmd`:

```rust
Timeline {
    session_id: Uuid,
    #[arg(long)]
    admin: bool,
}
```

Call `state.store.timeline` by default and `state.store.raw_timeline` for admin output.

- [ ] **Step 4: Add renderer**

For public timeline, print one line per entry:

```text
<kind> <world_state_version_or_dash> <description>
```

For admin timeline, use `print_json`.

- [ ] **Step 5: Run CLI tests**

Run: `cargo test -p cli --test cli_smoke session_timeline`

Expected: CLI timeline test passes.

- [ ] **Step 6: Commit**

```bash
git add crates/cli/src/commands/session.rs crates/cli/src/render.rs crates/cli/tests/cli_smoke.rs
git commit -m "feat(cli): inspect session timelines"
```

## Verification

Run:

```bash
cargo test -p persistence timeline
cargo test -p api --test memory_api_flows timeline
cargo test -p cli --test cli_smoke session_timeline
cargo test --workspace
```

Optional with Docker:

```bash
TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p persistence --test repository_tests timeline -- --ignored --test-threads=1
TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test postgres_api_flows timeline -- --ignored --test-threads=1
```

## Acceptance Criteria

- Public timeline route lists session evolution without raw GM state.
- Admin raw timeline route exposes messages, deltas, and events behind the existing admin bearer token.
- CLI can inspect public or raw timeline data.
- Postgres timeline includes persisted deltas from `world_state_deltas`.
- Timeline read paths do not mutate state.

## Risks

- Ordering across messages, events, and deltas can be ambiguous if using separate `created_at` timestamps. Keep raw lists separate and make public ordering deterministic.
- Raw provider output can contain sensitive data; keep it admin-only and honor `store_raw_provider_output`.
- Memory store lacks persistent delta records; document or add in-memory delta storage if tests require parity.

