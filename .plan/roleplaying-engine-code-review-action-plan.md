# Advanced Roleplaying Framework Prototype — Code Review Action Plan

## Purpose

This document converts the architecture/code review findings into a phased execution plan for coding agents.

Repository under review:

```text
https://github.com/hofdo/advanced-roleplaying-framework-prototype
```

The project already has the correct overall shape for a roleplaying engine:

```text
frontend
  -> Axum API
  -> turn pipeline
  -> context builder
  -> role identity activation
  -> LLM provider
  -> response parser
  -> delta validator
  -> world-state reducer
  -> frontend state projector
  -> persisted state
```

The next work should focus on hardening the prototype, not adding new features.

---

## Current Architecture Status

### Implemented well

The prototype already appears to contain the important building blocks:

- Rust workspace split into `api`, `domain`, `engine`, `providers`, `persistence`, and `shared`.
- Domain model for scenarios, NPCs, factions, facts, clocks, quests, world state, frontend visible state, and typed world-state deltas.
- `NpcStatus` enum and `NpcState.status`.
- Provider abstraction with OpenAI-compatible provider and capabilities.
- Axum endpoints for health, providers, scenarios, sessions, turns, streaming turns, projected world state, and events.
- Non-streaming turn pipeline.
- Streaming turn flow with visible token stream and second-call delta extraction.
- Frontend projection instead of returning full authoritative world state from normal turn responses.
- PostgreSQL migration baseline with scenarios, sessions, world states, messages, world-state deltas, events, and provider configs.
- Prompt version metadata.
- Typed `WorldStateDelta` structure.

### Main remaining risk

Several MVP shortcuts still weaken the safety model:

```text
stubbed session provider selection
raw export leakage
in-memory-only turn locking
streaming/non-streaming logic duplication
weak secret-leak detection
missing JSON repair retry
missing provider retry policy
```

Fix those before building advanced features.

---

## Phase 1 — Fix correctness and safety issues

Goal: remove misleading APIs and prevent accidental state/secret leakage.

### 1.1 Implement real session-scoped provider selection

Problem:

```http
PATCH /sessions/{session_id}/provider
```

exists, but behaves like a stub if it only validates the provider and keeps using the global/default provider.

Required behavior:

1. Persist the selected provider on the session.
2. Resolve provider at turn time with this priority:

```text
session provider -> default provider -> error if none configured
```

Suggested persistence:

```sql
UPDATE sessions
SET provider_id = $provider_id,
    updated_at = now()
WHERE id = $session_id;
```

Acceptance criteria:

- Setting a provider for session A does not change session B.
- Turn processing for session A uses the selected provider.
- Invalid provider ID returns `404` or `422`.
- Provider selection is covered by integration tests.

---

### 1.2 Prevent raw export leakage

Problem:

Session export can expose full authoritative `WorldState`, which may contain:

- GM-only facts,
- hidden NPC knowledge,
- hidden faction goals,
- unrevealed clocks,
- internal state.

Required behavior:

Split exports into player-visible and raw-admin modes.

Recommended endpoints:

```http
GET /sessions/{session_id}/export
GET /admin/sessions/{session_id}/export/raw
```

Default export must use `FrontendStateProjector`.

Acceptance criteria:

- Normal export contains only `FrontendVisibleState`, visible messages, and public events.
- Raw export is available only through explicit admin/debug path.
- GM-only facts never appear in normal export.
- Add tests with a GM-only fact and verify it is absent from normal export.

---

### 1.3 Make turn locking deployment-safe

Problem:

An in-memory turn lock protects only one process. It does not protect multiple API instances or restarts during streaming.

Required behavior:

Use one of these for PostgreSQL-backed deployments:

Option A — PostgreSQL advisory lock:

```sql
SELECT pg_try_advisory_lock(hashtext($session_id));
```

Option B — session row lock/flag:

```sql
UPDATE sessions
SET processing_turn = true,
    processing_turn_started_at = now()
WHERE id = $1
  AND processing_turn = false;
```

If lock acquisition fails:

```http
409 Conflict
```

Acceptance criteria:

- Two simultaneous turns for the same session cannot both run.
- Concurrent turns for different sessions can run.
- Locks are released after success and after failure.
- Stale lock recovery exists if using `processing_turn`.

---

### 1.4 Fix `NpcChange::KnowledgeAdded`

Problem:

`NpcChange::KnowledgeAdded` must update `NpcState.known_facts`, not generic notes.

Required behavior:

```rust
NpcChange::KnowledgeAdded { fact, .. } => {
    npc.known_facts.push(fact.clone());
}
```

Acceptance criteria:

- NPC knowledge appears in `known_facts`.
- Notes remain for non-factual annotations only.
- Add reducer unit test.

---

## Phase 2 — Reduce duplicated engine logic

Goal: ensure streaming and non-streaming turns cannot drift apart.

### 2.1 Extract shared turn preparation/finalization

Problem:

Non-streaming turns use the main pipeline, while streaming manually rebuilds the same components and repeats validation/reducer/persistence logic.

Required refactor:

Move shared logic into reusable engine methods:

```rust
prepare_turn_context(...)
finalize_turn_delta(...)
persist_turn_result(...)
```

Recommended shape:

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

Acceptance criteria:

- Non-streaming and streaming use the same delta validation.
- Non-streaming and streaming use the same reducer.
- Non-streaming and streaming use the same projector.
- A bug fix in finalization applies to both paths.

---

## Phase 3 — Improve LLM robustness

Goal: make local/unstable models usable without corrupting state.

### 3.1 Add one controlled JSON repair retry

Problem:

Real models often return malformed structured output.

Required behavior:

```text
parse fails
  -> send repair prompt with raw output and schema
  -> parse repaired output
  -> if still invalid, persist error event and do not apply delta
```

Do not retry the whole turn. Repair only structured output.

Acceptance criteria:

- Malformed JSON triggers one repair attempt.
- Invalid repaired JSON does not mutate world state.
- Repair prompt is snapshot-tested.
- Error event is persisted when repair fails.

---

### 3.2 Implement provider retry policy

Problem:

Provider capabilities include retry fields, but retries must be implemented carefully.

Retry only:

- transport errors,
- timeouts,
- HTTP `5xx`,
- optionally HTTP `429` with backoff.

Do not retry:

- malformed model output,
- validation failure,
- unsafe delta,
- prompt-level errors.

Acceptance criteria:

- `max_retries` is used.
- Retry is covered by provider tests.
- Malformed JSON does not use transport retry path.

---

### 3.3 Clarify raw provider output policy

Problem:

`raw_provider_output` is useful for debugging but can contain hidden reasoning or secrets.

Required policy:

```text
raw_provider_output must be NULL by default in production.
Store it only when explicit local debug mode is enabled.
Never expose it to normal frontend responses or normal exports.
```

Acceptance criteria:

- Default turn persistence stores `raw_provider_output = NULL`.
- Debug mode is opt-in.
- Tests verify normal API responses never contain raw provider output.

---

## Phase 4 — Strengthen secret protection

Goal: move beyond literal string matching for secret leakage.

### 4.1 Add secret/fact relation metadata

Problem:

Exact text comparison misses paraphrased leaks.

Example:

```text
GM-only: The soul-mark was not created by the goddess.
Leaked paraphrase: Another power made your mark.
```

Recommended model:

```rust
pub struct FactToAdd {
    pub text: String,
    pub visibility: FactVisibility,
    pub known_by: Vec<EntityKey>,
    pub related_secret_ids: Vec<EntityKey>,
    pub reveal_condition_satisfied: Option<String>,
    pub reason: String,
}
```

Validation rule:

```text
If visibility is PlayerKnown and related_secret_ids is not empty,
then reveal_condition_satisfied must be present and valid.
```

Acceptance criteria:

- Player-known facts related to secrets require reveal condition proof.
- GM-only facts cannot become player-known by paraphrase without reveal.
- Add tests for paraphrased leak attempts.

---

### 4.2 Improve NPC visibility projection

Problem:

Projection based only on `NpcStatus` can hide useful state, especially `Missing`.

Recommended model:

```rust
pub struct NpcState {
    pub id: EntityKey,
    pub status: NpcStatus,
    pub visible_to_player: bool,
    // ...
}
```

Projection rule:

```text
Hidden + visible_to_player=false -> not shown
Missing + visible_to_player=true -> shown as Missing
Captured + visible_to_player=true -> shown as Captured
Dead + visible_to_player=true -> shown as Dead
```

Acceptance criteria:

- Missing NPCs can be visible when the player knows they are missing.
- Hidden NPCs remain hidden when appropriate.
- Projection tests cover `Missing`, `Hidden`, `Captured`, and `Dead`.

---

## Phase 5 — API and product polish

Goal: make the API truthful, predictable, and frontend-ready.

### 5.1 Provider health vs readiness

Problem:

Provider health can mean either “configured” or “reachable.” These are different.

Recommended split:

```http
GET /providers/{provider_id}/health      // configured
GET /providers/{provider_id}/readiness   // reachable model/provider
```

Or return both fields:

```json
{
  "configured": true,
  "reachable": false,
  "message": "Provider configured but model server is not reachable."
}
```

Acceptance criteria:

- Provider health does not falsely imply the model server is reachable.
- Readiness check performs a real lightweight provider call if enabled.

---

### 5.2 Align license metadata

Problem:

Repository license and `Cargo.toml` package license must match.

Required behavior:

- Pick one license.
- Align `LICENSE`, root README, and workspace `Cargo.toml`.

Acceptance criteria:

- GitHub license display matches Cargo metadata.

---

### 5.3 Add authentication later, not now

For a local prototype, no auth is acceptable.

Before multi-user deployment, add:

- session ownership,
- API key or user auth,
- admin/debug route protection,
- export authorization.

Do not add this before the core turn pipeline is stable unless the project is already deployed to other users.

---

## Final Prioritized Task List

Implement in this order:

1. Real session provider selection.
2. Player-visible export only; raw export admin/debug-only.
3. Database-backed or advisory turn locking.
4. Refactor streaming/non-streaming shared finalization.
5. Fix `NpcChange::KnowledgeAdded` to write to `known_facts`.
6. Add JSON repair retry once.
7. Implement provider retry policy.
8. Strengthen secret leak validation with `related_secret_ids` and reveal conditions.
9. Add `visible_to_player` to NPC projection.
10. Clarify provider health/readiness.
11. Align license metadata.

---

## Coding Agent Execution Brief

You are improving an existing Rust roleplaying engine prototype.

Do not add new features before hardening the core pipeline.

Focus on these architectural fixes:

```text
session provider persistence
safe projected export
deployment-safe turn locking
shared turn finalization
correct NPC knowledge reducer behavior
JSON repair retry
provider retry policy
stronger secret leak validation
NPC visibility projection
provider health/readiness distinction
```

Preserve the existing architecture:

```text
domain types
engine pipeline
provider abstraction
persistence repositories
Axum API
frontend projection
typed deltas
world-state reducer
```

Do not let the LLM directly mutate full world state. Do not expose GM-only facts, raw world state, raw deltas, raw provider output, or hidden reasoning to normal frontend routes.
