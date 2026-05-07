# Gap Analysis — Current Code vs Spec Documents

Generated: 2026-05-07  
Specs: `advanced-roleplaying-engine-guide-as-is.md` (master) + `advanced-roleplaying-engine-rust-implementation-2.md` (impl guide)

---

## MVP Status: COMPLETE

All 12 MVP criteria from rust-implementation-2.md are satisfied:
1. Scenario can be imported ✅
2. Session can be started ✅
3. Frontend can send player action ✅
4. Engine acquires session turn lock (PostgreSQL-backed) ✅
5. Engine builds context ✅
6. Engine calls LLM provider ✅
7. Frontend receives immersive response ✅
8. Engine validates and applies typed delta ✅
9. Authoritative world state persists ✅
10. Frontend-visible projected state returned without GM-only data ✅
11. Second turn uses updated state ✅
12. Streaming works without mutating state prematurely ✅

All 11 hardening items from the action plan are also complete.

---

## What Is Fully Implemented

| Feature | Evidence |
|---------|----------|
| All core API routes | `app.rs:18-53` |
| Session provider persistence + turn resolution | `repositories.rs:248-262`, `app.rs:216-229` |
| Safe export (player projection) | `app.rs:198-230` uses `ViewerContext::player()` |
| Raw export admin-only path | `GET /admin/sessions/:id/export/raw` |
| PostgreSQL turn lock + stale recovery | `persistence/src/lock.rs` |
| Shared turn prep/finalization | `engine/pipeline.rs` `PreparedTurn`/`FinalizedTurn` |
| NpcChange::KnowledgeAdded → `known_facts` | `engine/reducer.rs:39-60` |
| JSON repair retry, error event on failure | `engine/pipeline.rs` async `finalize_turn_delta` |
| Provider retry on transport/5xx/429 | `providers/openai_compatible.rs:147-165` |
| FactToAdd secret reveal validation | `engine/validation.rs`, `MissingRevealProof` variant |
| NpcState.visible_to_player projection | `domain/state.rs:55`, `engine/projection.rs` |
| Provider health (config) vs readiness (network) | `GET /providers/health`, `GET /providers/readiness` |
| License alignment (Apache-2.0) | `Cargo.toml:15` |
| TurnMode enum declared + accepted | `domain/state.rs:138-145`, `TurnRequestInput` |
| GET /world-state returns FrontendVisibleState | `app.rs:480-488` |
| patch_from_delta() on FrontendStateProjector | `engine/projection.rs:15-21` |
| Structured observability tracing | `engine/pipeline.rs:305-359` |
| NPC Dead→Active validation | `domain/validation.rs:69` |

---

## Remaining Gaps

### Priority 1 — Post-MVP spec requirements (not yet done)

#### 1.1 TurnMode not consumed by pipeline
**Spec says:** `TurnMode` (Dialogue/Action/Direct/Remember) should influence prompt behavior.  
**Current state:** `TurnMode` is accepted in `TurnRequestInput.mode` (`pipeline.rs:26`) and `TurnRequest.mode` (`app.rs:549`) but never used to change prompt building, scene classification, or context. Tests hardcode `mode: Some(TurnMode::Action)` but pipeline ignores the value.  
**Gap:** Mode-aware prompt shaping (e.g. `Direct` → out-of-character GM mode, `Remember` → fact correction path).  
**Spec reference:** rust-implementation-2.md TurnMode section (lines 1160-1173).

#### 1.2 DebugTurnResponse missing
**Spec says:** Admin/debug path should return `DebugTurnResponse` with `applied_delta: WorldStateDelta`.  
**Current state:** `TurnResponseBody` never includes the raw delta. No debug turn route exists.  
**Gap:** Add `GET /admin/sessions/:id/turn/debug` or a debug mode flag on the normal turn that returns `DebugTurnResponse`.  
**Spec reference:** rust-implementation-2.md lines 1191-1203.

#### 1.3 Incomplete NPC status transition validation
**Spec says:** Reject `Unconscious` NPCs making plans or negotiations. Reject `Dead` NPCs speaking without resurrection/undeath established. Require `reason` for every status change.  
**Current state:** Only `Dead→Active` without revival event is validated (`domain/validation.rs:69`). `Unconscious`, `Captured`, `Hidden` restrictions are absent.  
**Gap:** Add status behavior rules: `Unconscious` cannot perform `AttitudeChanged` or `KnowledgeAdded`. `Dead` NPC speech rejected unless scenario has ghost/undeath rules.  
**Spec reference:** rust-implementation-2.md lines 350-356 "Status validation rules".

---

### Priority 2 — Architecture gaps blocking future features

#### 2.1 Provider registry missing (blocks real multi-provider support)
**Spec says:** Session provider_id → resolve from provider registry → default → error.  
**Current state:** `SessionRecord.provider_id` is persisted, but turn resolution (`app.rs:222-229`) always falls back to `Arc::clone(&state.provider)` regardless of `session.provider_id`. Comment says "when a provider registry is added this is where the lookup will go."  
**Gap:** `AppState` needs `providers: HashMap<Uuid, Arc<dyn LlmProvider>>` populated from `ProviderConfigRepository`. Turn resolution must look up by `session.provider_id`.  
**Spec reference:** rust-implementation-2.md lines 1120-1124 (`PATCH /sessions/{session_id}/provider`).

#### 2.2 ProviderConfigRepository missing
**Spec says:** Provider configs should be persisted in `provider_configs` DB table and managed via API.  
**Current state:** `provider_configs` table exists in schema, but `ProviderConfigRepository` trait and `POST /providers`, `DELETE /providers/:id` management routes do not exist. Providers are config-file only.  
**Gap:** Add `ProviderConfigRepository` trait + `PgPersistence` impl; add `POST /providers` (register), `DELETE /providers/:id` (remove) routes; load providers from DB at startup alongside config-file default.  
**Spec reference:** rust-implementation-2.md lines 193-197 (`ProviderConfigRepository`).

---

### Priority 3 — Test quality gaps

#### 3.1 No wiremock integration tests
**Spec says:** Use `wiremock` for LLM provider tests. Required test cases include: valid turn applies delta, invalid delta rejected, provider failure returns 502, malformed JSON triggers repair once, streaming does not mutate state until final delta, secret leak rejected, unknown entity ID rejected, 409 on concurrent turns.  
**Current state:** `wiremock` is NOT in `api/Cargo.toml` dev-dependencies. Provider tests use the mock provider but no HTTP-level mock server.  
**Gap:** Add `wiremock` to dev-deps; implement required integration test cases against a mock HTTP server.  
**Spec reference:** rust-implementation-2.md lines 1452-1466 "Integration tests".

#### 3.2 No behavioral fixture tests
**Spec says:** Add scenario-level tests verifying complex world-state outcomes (e.g. flood the guildhall → NPCs alarmed, clock advances, no GM-only fact in response).  
**Current state:** No fixture-based behavioral tests exist.  
**Gap:** Add at least one scenario fixture test demonstrating turn→delta→state→projection correctness end-to-end.  
**Spec reference:** rust-implementation-2.md lines 1469-1483 "Behavioral fixtures".

---

### Priority 4 — Explicitly deferred by spec ("What Not To Build Yet")

These are called out in the spec as future work only after the core is stable:

| Item | Why deferred |
|------|-------------|
| Typed mutation endpoints (`POST /facts`, `POST /quests/:id/complete`, etc.) | "Prefer typed mutations" but spec also warns against "huge endpoint surface"; not in MVP |
| ConsistencyAuditor | Mentioned in engine module list but not in MVP definition; no spec section defines it |
| Vector memory (`pgvector`, `memories` table) | Explicitly deferred: "Do not add vector memory until structured world state works" |
| Realtime multiplayer | Explicitly deferred |
| Complex auth/permissions | Deferred: "Add authentication later, not now" |
| Autonomous multi-agent loops | Explicitly deferred |

---

## Execution Order for Next Plan

Based on spec priority and dependency ordering:

1. **NPC status transition validation** (1.3) — pure validation logic, no dependencies, high safety value
2. **TurnMode behavior** (1.1) — affects prompt quality; needs scene classifier + prompt builder changes
3. **ProviderConfigRepository** (2.2) — unblocks provider registry
4. **Provider registry** (2.1) — makes session-scoped provider selection real
5. **DebugTurnResponse** (1.2) — admin debug path, low risk
6. **wiremock integration tests** (3.1) — validates existing behavior against spec test list
7. **Behavioral fixture tests** (3.2) — scenario-level correctness proof

Defer: ConsistencyAuditor, typed mutation endpoints, vector memory, auth.

---

## Summary

The engine has completed its entire hardening phase and satisfies the full MVP definition. The remaining work is post-MVP enhancement:
- Two spec-required behavioral rules not yet enforced (TurnMode shaping, extended NPC status validation)
- One admin feature missing (DebugTurnResponse)  
- One architecture gap blocking real multi-provider use (provider registry + ProviderConfigRepository)
- Test suite not yet at spec-recommended quality (wiremock, behavioral fixtures)
