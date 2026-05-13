# Docs Current State Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Update README documentation so it accurately describes implemented Postgres turn locking, current secrecy boundaries, routes, and test coverage.

**Architecture:** Treat documentation as a checked contract for the repository. Verify claims against `api`, `persistence`, and engine code before changing prose, and add narrow doc checks that prevent the most important stale claims from returning.

**Tech Stack:** Rust, Cargo workspace, Axum, SQLx/Postgres, Clap CLI, serde, tokio tests

---

## Current State

- `README.md` says "Turn locking is in-memory only" even though `crates/persistence/src/lock.rs` implements `PostgresSessionTurnLock`.
- `crates/api/src/state.rs` wires `PostgresSessionTurnLock` when `StorageBackend::Postgres` is configured.
- `crates/persistence/migrations/0001_core_schema.sql` includes `processing_turn` and `processing_turn_started_at` columns on `sessions`.
- `crates/persistence/tests/repository_tests.rs` includes ignored tests for fresh lock acquisition, duplicate lock rejection, and stale lock recovery.
- `crates/README.md` already says the persistence crate owns database-backed locks.
- `README.md` correctly calls out the non-streaming secrecy limitation, but that text must change after `architecture/01-non-streaming-secrecy-boundary.md` is implemented.

## Target Behavior

- Root documentation accurately states that memory mode uses an in-memory lock and Postgres mode uses database-backed turn locks.
- The architecture and known limitations sections match actual code.
- The API route table uses "list" for events, not "stream", because `GET /sessions/:id/events` returns JSON.
- Test documentation names which suites are default, ignored Postgres, and live local-LLM.
- Docs do not overstate production readiness.

## File Structure

- Modify: `README.md`
  - Fix stale lock limitation.
  - Update testing and route descriptions.
  - Keep secrecy limitation current with the implemented state.
- Modify: `crates/README.md`
  - Add a short note that Postgres turn locking is implemented in `persistence` and wired by `api`.
- Optional create: `scripts/check-docs-current-state.sh`
  - Small shell guard for stale phrases if maintainers want an executable doc check.

## Tasks

### Task 1: Replace Stale Lock Limitation

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Confirm code path**

Read:

```bash
sed -n '1,140p' crates/api/src/state.rs
sed -n '1,140p' crates/persistence/src/lock.rs
sed -n '1,80p' crates/persistence/migrations/0001_core_schema.sql
```

Expected: Postgres mode constructs `PostgresSessionTurnLock`, and the migration has lock columns.

- [ ] **Step 2: Change known limitation text**

Replace the stale "Turn locking is in-memory only" section with:

```markdown
**Turn locking depends on storage mode**

Memory mode uses `InMemorySessionTurnLock`, which only coordinates turns inside one process. PostgreSQL mode uses `PostgresSessionTurnLock`, backed by `sessions.processing_turn` and `processing_turn_started_at`, so separate API instances sharing the same database coordinate turn processing. This is suitable for prototype multi-process protection, but it is still a coarse session-level lock rather than a queue.
```

- [ ] **Step 3: Run stale phrase search**

Run: `rg -n "Turn locking is in-memory only|PostgreSQL-backed distributed locking is not yet implemented|Run a single API instance" README.md crates/README.md`

Expected: no matches.

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: correct turn locking status"
```

### Task 2: Update Secrecy Boundary Text

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Check current implementation state**

Run: `rg -n "build_visible_response_prompt|build_non_streaming_prompt|build_delta_extraction_prompt|render_narration_context" crates/engine/src/prompt.rs crates/engine/src/pipeline.rs`

Expected after the secrecy-boundary plan: visible response and delta extraction are separate. Expected before that plan: only streaming and delta extraction are separate.

- [ ] **Step 2: Write docs matching code**

If the non-streaming split has landed, replace the known limitation with:

```markdown
**Secrecy boundary is split before state mutation**

Streaming and non-streaming turns generate player-visible narration from a narration-safe context. Structured delta extraction runs afterward with oracle context so hidden facts can affect state validation without being passed to player-visible narration. Secret-leak validation in the engine remains a second layer of defense.
```

If the split has not landed, keep the limitation but add the exact file references:

```markdown
**Non-streaming secrecy boundary**

Streaming narration uses `render_narration_context` and excludes GM-only facts. Non-streaming turns still combine visible response and delta generation through `build_non_streaming_prompt`, which means the model sees oracle context while writing player-visible output. The tracked fix is `.plan/architecture/01-non-streaming-secrecy-boundary.md`.
```

- [ ] **Step 3: Run docs search**

Run: `rg -n "secrecy|GM-only|non-streaming|render_narration_context|build_non_streaming_prompt" README.md .plan/architecture/01-non-streaming-secrecy-boundary.md`

Expected: README language matches the implementation state.

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: align secrecy boundary description"
```

### Task 3: Fix Route And Test Descriptions

**Files:**
- Modify: `README.md`
- Modify: `crates/README.md`

- [ ] **Step 1: Verify route behavior**

Run: `rg -n "route\\(\"/sessions/:session_id/events\"|async fn list_events|EventRecord" crates/api/src/app.rs crates/persistence/src/repositories.rs`

Expected: events route returns `Json<Vec<EventRecord>>`.

- [ ] **Step 2: Update route table**

Change the route description for `GET /sessions/:id/events` to "List session events".

- [ ] **Step 3: Update crate README**

Add this note under the persistence crate description:

```markdown
PostgreSQL turn locking lives in `persistence/src/lock.rs` and is selected by `api::AppState` when `ROLEPLAY_STORAGE=postgres`.
```

- [ ] **Step 4: Run Markdown checks**

Run: `rg -n "Stream session events|database-backed locks|PostgresSessionTurnLock|processing_turn" README.md crates/README.md`

Expected: no stale "Stream session events" route description, and current lock references exist.

- [ ] **Step 5: Commit**

```bash
git add README.md crates/README.md
git commit -m "docs: refresh API and crate map"
```

## Verification

Run:

```bash
rg -n "Turn locking is in-memory only|PostgreSQL-backed distributed locking is not yet implemented|Stream session events" README.md crates/README.md
rg -n "PostgresSessionTurnLock|processing_turn|database-backed locks" README.md crates/README.md crates/api/src/state.rs crates/persistence/src/lock.rs
cargo test -p persistence --test repository_tests acquire_lock_on_fresh_session_succeeds -- --ignored --test-threads=1
```

The final command requires Docker-backed Postgres. If it is not run, record that docs were checked against source code only.

## Acceptance Criteria

- README no longer claims Postgres locking is missing.
- README and `crates/README.md` identify the current lock implementation and its storage-mode boundary.
- The events route description matches the JSON-listing implementation.
- The secrecy-boundary documentation matches the code at the time of the docs change.
- No flat `.plan/*.md` files are rewritten as part of this docs update.

## Risks

- Documentation can race with architecture work; verify the secrecy-boundary state immediately before editing.
- Avoid claiming distributed production readiness beyond session-level Postgres coordination.
- Avoid making the live local-LLM script look like a deterministic CI requirement.

