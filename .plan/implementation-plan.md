# Implementation Plan — Hardening the Roleplaying Engine

Generated: 2026-05-07
Based on: `roleplaying-engine-code-review-action-plan.md` + full codebase audit

## Current State Summary

Audit against the 11-item action plan confirmed:

| # | Item | Audit Status |
|---|------|-------------|
| 1 | Session provider selection | PARTIAL — endpoint exists but persists nothing; `SessionRecord` has no `provider_id` field |
| 2 | Export leakage | MISSING + LEAK — POST returns raw `WorldState` unfiltered |
| 3 | Turn locking | IN-MEMORY ONLY — `InMemorySessionTurnLock` only |
| 4 | `NpcChange::KnowledgeAdded` | WRONG FIELD — writes to `notes`, not `known_facts` |
| 5 | Shared turn finalization | MISSING — streaming/non-streaming logic duplicated inline |
| 6 | JSON repair retry | PARTIAL — simple `{…}` extraction, no intelligent repair |
| 7 | Provider `max_retries` | CONFIGURED UNUSED — field exists, never used |
| 8 | `raw_provider_output` | PARTIAL — always `NULL` in code, but acceptance-criterion tests missing |
| 9 | `related_secret_ids` / `reveal_condition_satisfied` | MISSING — fields absent from `FactToAdd` (delta input, state.rs:150); not `Fact` (stored type, state.rs:24) |
| 10 | `NpcState.visible_to_player` | MISSING — visibility inferred from `status` only |
| 11 | Provider health/readiness split | MISSING — single `health()` always returns `ok: true` |

Item 8 (`raw_provider_output`) needs test coverage only. Remaining work: 11 items across 4 phases.

---

## Phase 0 — Documentation Discovery (per-phase)

Each phase below includes the specific files to read before implementing. No external docs needed; all APIs are internal Rust structs.

Files always needed as reference:
- `crates/domain/src/state.rs` — domain structs
- `crates/domain/src/scenario.rs` — Scenario, NPC, Secret, Fact structs
- `crates/api/src/app.rs` — all route handlers
- `crates/api/src/state.rs` — AppState, SessionRecord
- `crates/engine/src/pipeline.rs` — DefaultTurnPipeline
- `crates/engine/src/lock.rs` — SessionTurnLock trait + InMemorySessionTurnLock
- `crates/engine/src/reducer.rs` — apply_delta
- `crates/engine/src/projection.rs` — FrontendStateProjector
- `crates/persistence/src/repositories.rs` — all DB queries
- `crates/providers/src/provider.rs` — LlmProvider trait

---

## Phase 1 — Correctness and Safety (Priority: HIGHEST)

Goal: Remove misleading APIs and prevent state/secret leakage.

### 1.1 Fix `NpcChange::KnowledgeAdded` — `reducer.rs:39-42`

**Priority note:** Action plan places this at #5 (after shared refactor). Keeping it in Phase 1 here because it is an isolated correctness bug, but it has a non-trivial type mismatch that must be resolved before coding.

**Type mismatch (confirmed):**
- `NpcChange::KnowledgeAdded.fact` is `String` (state.rs:168)
- `NpcState.known_facts` is `Vec<EntityKey>` (state.rs:57)
- Action plan pseudo-code `npc.known_facts.push(fact.clone())` does NOT compile as-is

**Read before starting:**
- `crates/engine/src/reducer.rs:39-42` — current wrong write
- `crates/domain/src/state.rs:51-59` — `NpcState` struct
- `crates/domain/src/state.rs:160-182` — `NpcChange` enum with exact field types
- `crates/domain/src/state.rs:1-31` — `WorldState` and `Fact` structs

**Required approach:** The reducer must create a new `Fact` entity in `world_state.facts`, then push its generated `EntityKey` to `npc.known_facts`:

```rust
NpcChange::KnowledgeAdded { npc_id, fact, visibility, .. } => {
    // 1. Create a new Fact entity
    let fact_id = EntityKey::new(); // use whatever ID generation pattern exists
    state.facts.push(Fact {
        id: fact_id.clone(),
        text: fact.clone(),
        visibility,
        known_by: vec![npc_id.clone()],
        source: FactSource::NpcKnowledge,
        reveal_conditions: vec![],
    });
    // 2. Register on NPC
    if let Some(npc) = state.npcs.iter_mut().find(|n| n.npc_id == npc_id) {
        npc.known_facts.push(fact_id);
    }
}
```

Check `crates/domain/src/ids.rs` for exact `EntityKey` construction pattern and `FactSource` variants before writing this.

**Verification:**
- Unit test: apply `NpcChange::KnowledgeAdded` → `world_state.facts` contains new Fact, `npc.known_facts` contains its ID
- Unit test: `npc.notes` unchanged after KnowledgeAdded
- Compile check: no `String` pushed to `Vec<EntityKey>`

---

### 1.2 Real session provider selection

**Read before starting:**
- `crates/api/src/state.rs:99-104` — `SessionRecord` struct (no `provider_id` today)
- `crates/api/src/app.rs:85-99` — `set_session_provider` handler (discards selection)
- `crates/api/src/app.rs:213-239` — non-streaming turn handler (where provider is resolved)
- `crates/api/src/app.rs:241-300` — streaming turn handler (same)
- `crates/persistence/src/repositories.rs` — sessions table queries
- DB migration file (find in `crates/persistence/migrations/` or similar)

**Changes required:**

1. Add `provider_id: Option<Uuid>` to `SessionRecord` in `state.rs`.

2. Add migration:
```sql
ALTER TABLE sessions ADD COLUMN provider_id UUID REFERENCES provider_configs(id);
```

3. Update `repositories.rs` session SELECT queries to include `provider_id`.

4. Update `set_session_provider` handler to run:
```sql
UPDATE sessions SET provider_id = $provider_id, updated_at = now() WHERE id = $session_id
```
Return `404` if session not found, `422` if provider_id is invalid.

5. Update turn resolution in both `turn()` and `turn_stream()` handlers:
```
session.provider_id -> default provider -> error if none
```

**Verification:**
- Integration test: set provider on session A, verify session B is unaffected
- Integration test: turn on session A uses session A's provider
- Test: invalid provider_id returns 422

---

### 1.3 Safe export — split player vs raw

**Read before starting:**
- `crates/api/src/app.rs:44, 194-211` — current export handler (POST, returns raw WorldState)
- `crates/api/src/app.rs:488-492` — `ExportSessionResponse` struct
- `crates/engine/src/projection.rs` — `FrontendStateProjector`
- `crates/domain/src/state.rs:249-270` — `FrontendVisibleState`, `FrontendStatePatch` (both exist)
- `crates/domain/src/state.rs:279-288` — `ViewerContext` struct with `ViewerContext::player()` constructor (exists)

**Changes required:**

1. Change HTTP verb from `POST` to `GET` for default export.

2. Pass `ViewerContext::player()` to `FrontendStateProjector` in the default export handler:
```rust
// GET /sessions/:session_id/export
let visible = projector.project(&world_state, &ViewerContext::player());
// Return FrontendVisibleState + visible messages + public events
```

3. Add raw export route guarded by `AppConfig.debug_mode` or similar flag:
```rust
// GET /admin/sessions/:session_id/export/raw
// Only enabled when debug/admin mode — returns raw WorldState
```

4. Update `ExportSessionResponse` to contain `FrontendVisibleState` not `WorldState`.

**Anti-pattern guard:** Do NOT pass `ViewerContext { include_debug_state: true, is_admin: true }` to the default export. Only the admin/debug route may use elevated context.

**Verification:**
- Test: create session with GM-only fact (`FactVisibility::GmOnly`); normal export must NOT contain that fact
- Test: raw admin export contains the GM-only fact
- Test: normal export response struct contains no `WorldState` field
- Test: normal API responses never contain `raw_provider_output` (covers item 8 acceptance criterion)

---

### 1.4 Database-backed turn locking

**Read before starting:**
- `crates/engine/src/lock.rs` — `SessionTurnLock` trait + `InMemorySessionTurnLock`
- `crates/api/src/state.rs:67, 95, 108` — where `InMemorySessionTurnLock::default()` is constructed
- `crates/persistence/src/repositories.rs` — existing DB query patterns (for SQL style reference)

**Implementation approach:** Use Option B (session row flag) from action plan — safer than advisory locks for portable SQL:

1. Add migration:
```sql
ALTER TABLE sessions
  ADD COLUMN processing_turn BOOLEAN NOT NULL DEFAULT FALSE,
  ADD COLUMN processing_turn_started_at TIMESTAMPTZ;
```

2. Add `PostgresSessionTurnLock` implementing `SessionTurnLock` trait:
```rust
pub struct PostgresSessionTurnLock { pool: PgPool }
```
- `acquire`: `UPDATE sessions SET processing_turn=true, processing_turn_started_at=now() WHERE id=$1 AND processing_turn=false` — if 0 rows updated, return 409
- `release`: `UPDATE sessions SET processing_turn=false WHERE id=$1`
- Add stale lock recovery (e.g. if `processing_turn_started_at` older than 5 minutes, force-release)

3. Wire `PostgresSessionTurnLock` into `AppState` using the DB pool.

**Verification:**
- Test: two simultaneous turns for same session — second gets 409
- Test: concurrent turns for different sessions both succeed
- Test: lock is released after success and after failure (even on panic)

---

## Phase 2 — Reduce Duplication

Goal: streaming and non-streaming turns cannot drift apart.

### 2.1 Extract shared turn prep and finalization

**Read before starting:**
- `crates/api/src/app.rs:213-410` — full non-streaming and streaming handlers side-by-side
- `crates/engine/src/pipeline.rs` — `DefaultTurnPipeline` (used only in non-streaming path today)
- `crates/engine/src/context.rs` — `AgentContext`
- `crates/engine/src/scene.rs` — `SceneReasoningStyle`
- `crates/engine/src/validation.rs` — delta validation
- `crates/engine/src/reducer.rs` — `apply_delta`
- `crates/engine/src/projection.rs` — projector

**New structs to create in `crates/engine/src/pipeline.rs`:**

```rust
pub struct PreparedTurn {
    pub session: Session,
    pub scenario: Scenario,
    pub world_state: WorldState,
    pub context: AgentContext,
    pub scene_type: SceneReasoningStyle,
}

pub struct FinalizedTurn {
    pub message_id: MessageId,
    pub visible_response: String,
    pub world_state_version: i64,
    pub frontend_state_patch: FrontendStatePatch,
}
```

**New engine methods:**
```rust
pub async fn prepare_turn_context(...) -> Result<PreparedTurn, TurnError>
pub async fn finalize_turn_delta(prepared: PreparedTurn, raw_output: &str) -> Result<FinalizedTurn, TurnError>
pub async fn persist_turn_result(finalized: FinalizedTurn, ...) -> Result<(), TurnError>
```

**Refactor:**
- Non-streaming: call `prepare_turn_context` → provider → `finalize_turn_delta` → `persist_turn_result`
- Streaming: call `prepare_turn_context` → stream tokens → collect full output → `finalize_turn_delta` → `persist_turn_result`

**Verification:**
- Delta validation logic appears exactly once in codebase (grep for validation call)
- Reducer call appears exactly once
- Projector call appears exactly once
- Both streaming and non-streaming tests pass

---

## Phase 3 — LLM Robustness

Goal: make local/unstable models usable without corrupting state.

### 3.1 Add one controlled JSON repair retry

**Read before starting:**
- `crates/engine/src/prompt.rs:160-175` — `parse_turn_output` (current simple extraction)
- `crates/providers/src/provider.rs` — `LlmProvider` trait (for repair call API)
- `crates/engine/src/pipeline.rs` — where parse is called after LLM response

**Required behavior:**
```
parse raw output
  → if ok: use it
  → if fail: send repair prompt (raw output + JSON schema)
    → parse repaired output
    → if still invalid: persist error event, do NOT apply delta
```

**Repair prompt template** (snapshot test this):
```
The following output was malformed JSON. Return only valid JSON matching the schema.
Schema: {schema}
Malformed output: {raw}
```

**Verification:**
- Unit test: malformed JSON triggers repair attempt
- Unit test: still-invalid repair does not apply delta
- Snapshot test: repair prompt matches expected template
- Unit test: error event is persisted when repair fails

---

### 3.2 Implement provider retry policy

**Read before starting:**
- `crates/providers/src/openai_compatible.rs:113-166` — `generate()` and `stream()` (no retry today)
- `crates/providers/src/provider.rs:33` — `max_retries` field in `ProviderCapabilities`
- `crates/shared/src/config.rs:140` — config `max_retries`

**Retry rules (from action plan):**
- Retry ONLY: transport errors, timeouts, HTTP 5xx, optionally 429 with backoff
- Do NOT retry: malformed output, validation failure, unsafe delta, prompt errors

**Implementation:**
- Add retry loop in `OpenAiCompatibleProvider::generate()` and `::stream()`
- Use `max_retries` from `ProviderCapabilities`
- Categorize errors: `ProviderError::Transport` vs `ProviderError::ModelOutput`
- Only transport errors trigger retry loop

**Verification:**
- Unit test with mock HTTP server: 5xx on attempt 1, success on attempt 2 → succeeds
- Unit test: malformed JSON does NOT trigger retry
- Test: `max_retries=0` means no retry

---

## Phase 4 — Strengthen Secret Protection

Goal: paraphrase leaks are caught; NPC visibility is explicit.

### 4.1 Add `related_secret_ids` and `reveal_condition_satisfied` to `FactToAdd`

**CRITICAL DISTINCTION:** The action plan targets `FactToAdd` (the LLM delta input struct, state.rs:150), NOT the stored `Fact` struct (state.rs:24). These are different types. `FactToAdd` lives inside `WorldStateDelta.facts_to_add` (state.rs:139). The validation fires when the delta is applied, before the `FactToAdd` becomes a stored `Fact`.

**Read before starting:**
- `crates/domain/src/state.rs:137-156` — `WorldStateDelta` and existing `FactToAdd` struct
- `crates/domain/src/state.rs:24-31` — stored `Fact` struct (do NOT change this)
- `crates/domain/src/scenario.rs:115-119` — `Secret` struct
- `crates/engine/src/validation.rs` — current validation rules (where to add the new rule)
- `crates/engine/src/reducer.rs` — where `FactToAdd` is consumed and converted to `Fact`

**Changes to `FactToAdd` struct (state.rs:150):**
```rust
pub struct FactToAdd {
    pub text: String,
    pub visibility: FactVisibility,
    pub known_by: Vec<EntityKey>,
    pub reveal_conditions: Vec<String>,
    pub reason: String,
    // NEW:
    pub related_secret_ids: Vec<EntityKey>,
    pub reveal_condition_satisfied: Option<String>,
}
```

**Validation rule to add in `validation.rs`:**
```
If visibility == PlayerKnown AND related_secret_ids is not empty,
then reveal_condition_satisfied MUST be Some(_) and non-empty.
```

**Reducer:** When converting `FactToAdd` → `Fact`, copy `related_secret_ids` and `reveal_condition_satisfied` onto the stored `Fact` as well (requires adding these fields to `Fact` too, but the validation gate lives at delta-apply time on `FactToAdd`).

**Migration:** Facts are stored in world_state JSONB — check `repositories.rs` to confirm. If so, no SQL migration needed beyond ensuring the JSON serialization is correct.

**Verification:**
- Unit test: `FactToAdd` with `visibility=PlayerKnown`, `related_secret_ids=[X]`, `reveal_condition_satisfied=None` → validation rejects
- Unit test: same but `reveal_condition_satisfied=Some("revealed via X")` → validation passes
- Unit test: `FactToAdd` with `visibility=GmOnly`, `related_secret_ids=[X]`, no reveal → validation passes
- Unit test: paraphrased leak attempt (PlayerKnown fact referencing secret without proof) rejected

---

### 4.2 Add `visible_to_player` to `NpcState`

**Read before starting:**
- `crates/domain/src/state.rs:51-59` — `NpcState` struct
- `crates/engine/src/projection.rs:47-56` — current projection rule (infers from status)

**Change `NpcState`:**
```rust
pub struct NpcState {
    pub npc_id: EntityKey,
    pub status: NpcStatus,
    pub visible_to_player: bool,  // NEW — explicit visibility flag
    pub location_id: Option<EntityKey>,
    pub attitude_to_player: Option<String>,
    pub known_facts: Vec<EntityKey>,
    pub notes: Vec<String>,
}
```

**Update projection rule in `projection.rs`:**
```
Hidden + visible_to_player=false  → not shown
Missing + visible_to_player=true  → shown as Missing
Captured + visible_to_player=true → shown as Captured
Dead + visible_to_player=true     → shown as Dead
```

**Migration:** Add `visible_to_player` to NPC JSON in world_state JSONB, or add column.

**Verification:**
- Projection test: Missing NPC, visible_to_player=true → appears in FrontendVisibleState
- Projection test: Hidden NPC, visible_to_player=false → absent from FrontendVisibleState
- Projection test: Dead + visible_to_player=true → appears as Dead

---

## Phase 5 — API Polish

Goal: API is truthful and frontend-ready.

### 5.1 Provider health/readiness split

**Read before starting:**
- `crates/providers/src/provider.rs:11, 18-22` — `LlmProvider` trait, `ProviderHealth` struct
- `crates/providers/src/openai_compatible.rs:101-106` — current `health()` always returns `ok:true`
- `crates/api/src/app.rs:27, 75-76` — `/providers/test` route

**Changes:**

1. Add `readiness()` to `LlmProvider` trait:
```rust
async fn readiness(&self) -> Result<ProviderReadiness, ProviderError>;
```

2. Update `ProviderHealth` or add `ProviderReadiness`:
```json
{
  "configured": true,
  "reachable": false,
  "message": "Provider configured but model server is not reachable."
}
```

3. `OpenAiCompatibleProvider::health()` → checks config only (is base_url set, is api_key set)
4. `OpenAiCompatibleProvider::readiness()` → makes lightweight real call (e.g. list models or minimal generate)

5. Add route:
```
GET /providers/{provider_id}/health    → configured check only
GET /providers/{provider_id}/readiness → real reachability check
```

**Verification:**
- Test: `health()` returns configured=true even if server is down
- Test: `readiness()` returns reachable=false if server is unreachable
- Test: `health()` never falsely implies model server is reachable

---

### 5.2 Align license metadata

**Files to check:**
- `LICENSE` (root)
- `README.md` (root) — license badge
- `Cargo.toml` (workspace root) — `license` field

**Change:** Pick one license. Ensure all three match.

---

## Execution Order

Matches action plan priority list exactly:

1. **Phase 1.2** — Real session provider selection
2. **Phase 1.3** — Safe export (player vs raw) — also covers item 8 test gap
3. **Phase 1.4** — DB-backed turn locking
4. **Phase 2.1** — Extract shared turn prep/finalization
5. **Phase 1.1** — Fix `NpcChange::KnowledgeAdded` type mismatch (action plan item 5)
6. **Phase 3.1** — JSON repair retry
7. **Phase 3.2** — Provider retry policy
8. **Phase 4.1** — `FactToAdd` secret relation fields + reveal validation
9. **Phase 4.2** — `NpcState.visible_to_player`
10. **Phase 5.1** — Provider health/readiness split
11. **Phase 5.2** — License alignment

Do not start Phase 2 until Phase 1 correctness items (1.2, 1.3, 1.4) are complete.
Do not start Phase 4 until Phase 2 is complete.
Phase 1.1 (KnowledgeAdded) can be done any time after Phase 2 — the shared reducer plumbing is in place by then.

---

## Agent Execution Brief

You are hardening a Rust roleplaying engine prototype. Do not add new features.

Preserve existing architecture:
- Domain types (`crates/domain/`)
- Engine pipeline (`crates/engine/`)
- Provider abstraction (`crates/providers/`)
- Persistence repositories (`crates/persistence/`)
- Axum API (`crates/api/`)
- Frontend projection (FrontendStateProjector)
- Typed deltas (WorldStateDelta)
- World-state reducer

Rules that must never be violated:
- LLM must never directly mutate full world state
- GM-only facts must never appear in normal frontend routes
- Raw world state, raw deltas, raw provider output, hidden reasoning must never reach normal frontend routes

Each phase is self-contained. Read the listed files before changing anything.