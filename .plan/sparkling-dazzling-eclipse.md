# Plan — Canonical Guide Coverage Review & Gap Plan

## Context

The repo has shipped MVP + hardening + post-MVP enhancements + test improvements (all 4 branches merged into `main`). The stale `.plan/gap-analysis-vs-spec.md` (2026-05-07) lists 7 gaps — every one is now closed.

This plan re-audits the codebase against `.plan/advanced-roleplaying-engine-canonical-guide.md` (2723 lines) end-to-end and inventories what is still missing or weak. Goal: a prioritized, evidence-backed roadmap of the remaining work between "current state" and "fully matches canonical guide".

## Coverage Snapshot

| Spec area | Status |
|---|---|
| Turn pipeline 15 stages | ✅ all present (`crates/engine/src/pipeline.rs:319`, `:385`) |
| Scene types (10 enum variants) | ⚠️ 5 of 10 styles never triggered by classifier |
| Role activation + ReasoningStyleDirective | ✅ `engine/src/context.rs:6,96` |
| WorldState schema | ✅ all entities + `visible_to_player` + `hidden_notes` (`domain/src/state.rs:5`) |
| WorldStateDelta variants | ✅ all 18, all carry `reason` |
| Validator rules | ✅ all spec rules enforced (`engine/src/validation.rs:24`) |
| Reducer | ⚠️ inventory / summary / current_scene / active_speaker / NPC notes / faction notes never mutated |
| Projection | ⚠️ visible_clocks unfiltered; `changed_entities` skips facts_to_add + relationship_changes |
| Provider contract | ⚠️ stream_idle_timeout / request_timeout fields not enforced; health() tautological |
| Persistence | ✅ all 6 repos + Postgres turn lock + optimistic versioning |
| API surface | ⚠️ `/admin/*` routes unauthenticated; ENABLE_ADMIN_ROUTES + STORE_RAW_PROVIDER_OUTPUT not honored |
| Streaming | ✅ two-call model, `<think>` strip inline, 409 on concurrent |
| Eventing | ⚠️ only `world_event` + `turn_error` rows; spec lists 12 pipeline event types |
| Tests | ✅ ~85 tests, ~37 Docker-gated; 1 behavioral fixture (spec asks for several) |
| Prompt assembly | 🔴 `render_context` drops ~60% of `AgentContext` (no recent messages, no NPC list, no factions, no quests, no clocks, no location, no role boundaries) |
| Memory / summarization | ⚠️ `WorldState.summary` field exists but is dead — never written |
| Multi-session | ✅ per-session lock + version conflict |

🔴 = high impact correctness gap, ⚠️ = behavioral or hardening gap, ✅ = matches spec.

## Prioritized Gaps

### Tier 1 — Correctness (the LLM is missing context it needs to play the game)

**T1.1 — Prompt context is gutted** 🔴 (`crates/engine/src/prompt.rs:132`)
`render_context` only emits scenario title, setting, scene type, priorities, avoid, active role name, player-known facts, GM-only facts (labeled), and recent_summary. It DROPS: recent_messages, current_location, NPC list, faction list, active quests, active clocks, knowledge_boundaries, forbidden_moves, speech_constraints. Spec §15 (lines 1356–1389) makes these required prompt layers. Effect: LLM has almost no situational awareness — it doesn't see who's in the room, what NPCs know, what's on the clock, or what was said last turn.
- **Fix scope:** Rewrite `render_context` to serialize the full 8-layer prompt (SYSTEM RULES, SCENE STYLE DIRECTIVE, ACTIVE ROLE ACTIVATION, CURRENT WORLD STATE, RELEVANT FACTS, RECENT SUMMARY, RECENT MESSAGES, PLAYER INPUT, OUTPUT CONTRACT) per guide lines 1411–1421.
- **Risk:** prompt growth → token budget. Add a soft cap and selection rules per guide lines 1380–1389 (only NPCs at current location / scene, factions tied to active quests/clocks, etc.).

**T1.2 — Clocks not filtered for player visibility** 🔴 (`crates/engine/src/projection.rs:82`)
All clocks in `state.clocks` are returned unconditionally as `visible_clocks`. Spec §8 (lines 411–420, 1640–1646) requires hiding unrevealed clocks from player projection. A "doom clock" the GM tracks for tension should not appear in the player UI.
- **Fix scope:** Add `visible_to_player: bool` to `ClockState` (default `true`, like `NpcState`). Filter in `BasicFrontendStateProjector::project`. Add a `ClockChange::VisibilityChanged` variant or set on creation only.
- **Risk:** schema migration. Use `#[serde(default = "default_true")]` for backwards compat.

**T1.3 — Reducer can't mutate inventory, summary, current_scene, active_speaker** ⚠️ (`crates/engine/src/reducer.rs`)
The reducer ignores `WorldState.inventory`, `summary`, `current_scene`, `active_speaker_id`, `NpcState.notes`, `FactionState.public_notes/hidden_notes`, `RelationshipState.notes`. There is no delta variant that can update them. Notable consequence: combat-stickiness in the classifier (`scene.rs:14`) reads `state.current_scene == "combat"`, but no delta path can SET `current_scene`, so combat never becomes sticky.
- **Fix scope:** Add delta variants `SceneChange { scene, reason }`, `ActiveSpeakerChange { npc_id, reason }`, `InventoryChange::{Added/Removed/Updated}`, and `SummaryUpdated { summary, reason }`. Wire reducer + validator + projector + tests.
- **Risk:** widens delta surface. Implement only the variants that scenarios actually need (start with SceneChange — required by combat sticky logic).

### Tier 2 — Security / hardening (admin attack surface)

**T2.1 — `/admin/*` routes are unauthenticated** ⚠️ (`crates/api/src/app.rs`)
`/admin/sessions/:id/export/raw` and `/admin/sessions/:id/turn/debug` are publicly registered. Spec §11 (lines 200–206, 1680–1697) requires `ADMIN_TOKEN` middleware and `ENABLE_ADMIN_ROUTES` flag. Currently no token check, no env flag.
- **Fix scope:** Add `ADMIN_TOKEN` env var + `ENABLE_ADMIN_ROUTES` flag in `shared/src/config.rs`. Implement `axum::middleware::from_fn` that checks `Authorization: Bearer <token>` against config; reject 401/404. Conditionally register admin routes only if `ENABLE_ADMIN_ROUTES=true`.
- **Risk:** breaking change for any deployment relying on current open admin path. Document in release log.

**T2.2 — `STORE_RAW_PROVIDER_OUTPUT` flag dead** ⚠️ (`crates/shared/src/config.rs:78`)
`DebugConfig.store_raw_provider_output` is read on startup but never consulted in `persist_successful_turn` (`crates/persistence/src/repositories.rs:511`) — the column is always written as NULL.
- **Fix scope:** Pass `store_raw_provider_output` into `TurnStateStore` (via `AppState`) and gate the column write on it. Same for the in-memory store.
- **Risk:** trivial wiring; no schema change.

**T2.3 — Provider per-request timeouts not enforced** ⚠️ (`crates/providers/src/openai_compatible.rs`)
`ProviderCapabilities.request_timeout_seconds` and `stream_idle_timeout_seconds` are config fields but the reqwest client uses defaults; SSE streaming has no idle-timeout watchdog. Spec §9 (line 1142) makes both timeouts mandatory.
- **Fix scope:** Build the `reqwest::Client` with `.timeout(Duration::from_secs(request_timeout_seconds))`. Wrap streaming chunks in `tokio::time::timeout(stream_idle_timeout_seconds, …)` per chunk; on timeout, abort stream and emit `provider_stream_idle_timeout` event.
- **Risk:** existing tests using slow mocks may need the bound raised.

### Tier 3 — Spec-required behaviors that are partial

**T3.1 — Pipeline events not persisted** ⚠️ (`crates/engine/src/pipeline.rs`)
Spec §12 (lines 2095–2108) lists 12 event types: `turn_started, turn_lock_acquired, context_built, provider_called, provider_stream_started, provider_stream_finished, delta_generated, delta_validation_failed, delta_applied, frontend_state_projected, turn_finished, turn_lock_released`. Currently the pipeline emits `tracing::info!` for these but only `world_event` (per `event_log_entries` string) and `turn_error` reach the `events` table.
- **Fix scope:** Add `EventRepository::record(session_id, event_type, payload_json)` calls at each pipeline boundary. OR introduce a `PipelineEventSink` trait and have `DefaultTurnPipeline` call it at every named milestone, with the Postgres impl writing rows.
- **Risk:** event-row volume per turn. Add an env flag `PERSIST_PIPELINE_EVENTS` if desired.

**T3.2 — Scene classifier coverage holes** ⚠️ (`crates/engine/src/scene.rs:11`)
5 of 10 `SceneReasoningStyle` variants are unreachable from classifier rules: `EmotionalScene`, `WorldSimulation` (only via `TurnMode::Remember`), `TravelExploration`, `Downtime`, `QuestResolution`. Spec §2 (lines 263–270) defines per-scene priorities for each, so these styles being orphaned means scenarios can never opt into them via natural language.
- **Fix scope:** Add keyword sets for the missing 5: emotional (cry, comfort, grieve, embrace, tears), travel (walk, journey, ride, road, climb), downtime (rest, recover, study, train, craft), questresolution (return quest, deliver quest, claim reward, conclude). Or accept this as a deferred LLM-classifier path (spec line 1207 explicitly says LLM classifier is deferred).
- **Risk:** keyword false positives. Mitigate via scene stickiness similar to combat.

**T3.3 — `changed_entities` incomplete** ⚠️ (`crates/engine/src/projection.rs:131`)
`patch_from_delta` collects entity refs from npc/faction/quest/clock/location changes only. `facts_to_add` and `relationship_changes` are silently dropped.
- **Fix scope:** Add `("fact", &fact.id_after_reduce)` and `("relationship", "{source_id}->{target_id}")` to the dedup loop. Requires a way to know the new fact id post-reduce; either the reducer emits the assigned id, or treat `facts_to_add` as anonymous via index.
- **Risk:** small.

**T3.4 — `WorldState.summary` is dead code** ⚠️ (`crates/domain/src/state.rs:19`)
The field is loaded into context (`crates/engine/src/context.rs:325`) but no code writes it. Spec §16 says recent summary is part of context; without it, every turn loses long-context memory after the 6-message window.
- **Fix scope:** EITHER (a) add a `SummaryUpdated` delta variant the LLM can emit per turn (cheapest), OR (b) add a periodic admin endpoint `POST /admin/sessions/:id/summary/regenerate` that calls the provider with a summarization prompt. (a) is simpler but trusts the LLM; (b) is more controlled.
- **Risk:** modest — touches reducer + validator + prompt builder.

### Tier 4 — Test depth (spec calls out fixtures we don't have)

**T4.1 — Behavioral fixture coverage** ⚠️
Spec §14 (lines 2179–2181) names 5 behavioral fixtures: role drift, secret leakage, NPC knowledge boundary, OP-player consequences, missing NPC visibility. Repo has 1 (`flood_guildhall_advances_state_and_projection_hides_gm_only_facts` in `crates/api/tests/behavioral_fixtures.rs`).
- **Fix scope:** Add 4 more fixtures. Each is one Docker-gated test asserting a multi-step pipeline outcome (e.g., NPC knowledge boundary: NPC in location A can't reference an event the player did in location B that no NPC witnessed).

**T4.2 — Prompt snapshot tests** ⚠️
Spec §14 names prompt snapshots for dialogue, politics, combat, mystery, rules adjudication. None exist.
- **Fix scope:** Snapshot the full rendered prompt for each scene style using a fixed `AgentContext`. Use `insta` or a manual `assert_eq!` against a stored fixture file. Most useful AFTER T1.1 lands so snapshots reflect the fixed prompt.

### Tier 5 — Deferred (the spec itself defers these)

| Item | Spec ref |
|---|---|
| Typed mutation endpoints (`POST /facts`, `/quests/:id/complete`, `/clocks/:id/advance`, `/relationships`) | conflicting — spec also warns "huge endpoint surface"; deferred |
| `ConsistencyAuditor` engine module | mentioned in module list (line 668), no spec section |
| Vector memory / `memories` table / `pgvector` | explicit defer (lines 482, 1954) |
| Realtime multiplayer | explicit defer (line 484) |
| Complex auth/permissions beyond ADMIN_TOKEN | explicit defer (line 485) |
| LLM-based scene classifier | explicit defer (line 1207) |
| Multi-agent NPC autonomous loops | explicit defer |

## Recommended Execution Order

Group A — high impact, low risk, do first:
1. **T1.1 Prompt context completion** — biggest correctness win for one focused PR.
2. **T2.1 Admin auth (ADMIN_TOKEN + ENABLE_ADMIN_ROUTES)** — security, isolated to api crate + config.
3. **T2.2 STORE_RAW_PROVIDER_OUTPUT honored** — trivial, ride-along.

Group B — schema-touching, plan together:
4. **T1.2 Clock visible_to_player** — small migration + projection filter.
5. **T1.3 SceneChange delta variant (only)** — unblocks combat stickiness.
6. **T3.4 SummaryUpdated delta variant** — keeps long-context memory alive.

Group C — observability + tests:
7. **T2.3 Provider request/idle timeouts** — reqwest builder + stream timeout.
8. **T3.1 Persist pipeline events** — fills out the events table.
9. **T3.3 changed_entities completeness** — tiny, do as a follow-up.

Group D — quality bar:
10. **T4.1 Four more behavioral fixtures** — incremental, one PR per fixture.
11. **T4.2 Prompt snapshots** — only after T1.1 lands so snapshots are stable.
12. **T3.2 Scene classifier coverage** — optional.

## Critical Files

- `crates/engine/src/prompt.rs:132` — `render_context` (T1.1)
- `crates/engine/src/projection.rs:82,131` — clocks + changed_entities (T1.2, T3.3)
- `crates/engine/src/reducer.rs` — new variants (T1.3, T3.4)
- `crates/engine/src/scene.rs:11` — classifier (T3.2)
- `crates/engine/src/pipeline.rs:319,385` — event sink wiring (T3.1)
- `crates/engine/src/validation.rs` — new variants validation
- `crates/domain/src/state.rs:5,99,147` — schema additions
- `crates/providers/src/openai_compatible.rs` — timeouts (T2.3)
- `crates/api/src/app.rs` — admin middleware + route gating (T2.1)
- `crates/api/src/state.rs` — pass DebugConfig flags into store (T2.2)
- `crates/persistence/src/repositories.rs:511` — gate raw_provider_output write (T2.2)
- `crates/persistence/migrations/` — new migration if T1.2 adds `clocks.visible_to_player`
- `crates/shared/src/config.rs` — ADMIN_TOKEN, ENABLE_ADMIN_ROUTES (T2.1)
- `crates/api/tests/behavioral_fixtures.rs` — additional fixtures (T4.1)

## Verification

For each shipped tier:

- `cargo test --workspace` must pass with zero failures (no Docker required).
- `cargo test -p api --test postgres_api_flows -- --ignored` and `cargo test -p persistence --test repository_tests -- --ignored` must pass on a Docker host.
- After T1.1: snapshot the rendered prompt for one fixture turn; manually inspect that NPC list, faction list, quests, clocks, recent messages, and role boundaries all appear.
- After T1.2: a test asserts a clock with `visible_to_player=false` is absent from `GET /sessions/:id/world-state` for `ViewerContext::player()` but present in `/admin/.../export/raw`.
- After T2.1: a test asserts `GET /admin/sessions/:id/export/raw` returns 401 without `Authorization: Bearer …`, 200 with the correct token, and 404 when `ENABLE_ADMIN_ROUTES=false`.
- After T2.2: with `STORE_RAW_PROVIDER_OUTPUT=true`, `messages.raw_provider_output` is non-NULL after a turn; with `false`, it remains NULL.
- After T2.3: a wiremock test that delays beyond `request_timeout_seconds` proves the request aborts with `ProviderError::Timeout`.
- After T3.1: a test asserts the `events` table contains rows for `turn_started`, `provider_called`, `delta_applied`, `turn_finished` after one successful turn.
- Update `release-log/release.md` and `.plan/gap-analysis-vs-spec.md` after each tier ships.
