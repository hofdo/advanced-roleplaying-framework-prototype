# Test Improvement Plan

Generated: 2026-05-08  
Based on: full test catalog + untested-path audit across all 6 crates

---

## Current State Summary

| Category | Count | Notes |
|----------|-------|-------|
| Unit / inline tests | 58 | providers, engine, shared |
| Non-Docker integration tests | 5 | wiremock HTTP provider tests only |
| Docker-gated integration tests | 13 | all API-layer tests |
| **Total** | **76** | |

### Critical gaps found

| Gap | Severity | Why it matters |
|-----|----------|----------------|
| `persistence` crate — 0 tests, 25+ repo methods | **High** | SQL bugs invisible; schema drift goes undetected |
| Zero non-Docker API integration tests | **High** | CI without Docker runs no API-layer tests at all |
| `engine/reducer.rs` — 2 of 14 delta variants tested | **High** | QuestChange, AttitudeChanged, GoalRevealed, etc. untested |
| `domain/state.rs` — 0 serde roundtrip tests | **Medium** | Tagged enum rename_all rules silently break JSON contract |
| `engine/scene.rs` — 2 of 10 scene types tested | **Medium** | Classifier regressions invisible |
| `AppState::resolve_provider()` — 0 unit tests | **Medium** | Provider registry logic never verified in isolation |
| `process_turn_debug()` — 0 unit tests | **Low** | Only exercised via Docker behavioral fixture |

### Overlooked test category

**In-memory HTTP layer tests** — Axum routes can be tested end-to-end with `ApiStore` +
`InMemorySessionTurnLock` + `MockProvider` via `tower::ServiceExt::oneshot()`, entirely without
Docker. Currently zero such tests exist. This means every route (CRUD, error paths, validation)
is only exercised if Docker is available.

---

## Execution Order

1. Domain serde roundtrips (pure unit, no deps)
2. Reducer completeness (pure unit, no deps)
3. Scene classifier completeness (pure unit, no deps)
4. In-memory API integration tests (no Docker — highest value)
5. process_turn_debug unit test (no Docker)
6. Persistence repository tests (Docker-gated)

---

## Phase 1 — Domain serde roundtrip tests

**File to create:** `crates/domain/tests/serde_roundtrip_tests.rs`  
**Requires Docker:** No  
**Estimated tests:** ~18

### What to implement

All domain delta and enum types use `#[serde(tag = "type", rename_all = "snake_case")]`.
A typo in a variant name or a missing `rename` attribute silently produces wrong JSON.
These tests prove the wire format is stable.

#### 1.1 NpcChange variants (copy pattern from `crates/domain/src/state.rs:174-197`)

```rust
// Template for each variant:
let change = NpcChange::AttitudeChanged { npc_id: "npc-1".into(), attitude: "hostile".into(), reason: "provoked".into() };
let json = serde_json::to_string(&change).unwrap();
let round: NpcChange = serde_json::from_str(&json).unwrap();
assert_eq!(change, round);
// Also assert the wire tag:
assert!(json.contains(r#""type":"attitude_changed""#));
```

Variants to cover:
- `AttitudeChanged` → tag `"attitude_changed"`
- `KnowledgeAdded` → tag `"knowledge_added"`
- `StatusChanged` → tag `"status_changed"`
- `LocationChanged` → tag `"location_changed"`

#### 1.2 FactionChange variants

- `StandingChanged` → tag `"standing_changed"`
- `GoalRevealed` → tag `"goal_revealed"`

#### 1.3 ClockChange variants

- `Advanced` → tag `"advanced"`
- `SetValue` → tag `"set_value"`

#### 1.4 QuestChange variants

- `Started` → tag `"started"`
- `ObjectiveCompleted` → tag `"objective_completed"`
- `Completed` → tag `"completed"`
- `Failed` → tag `"failed"`

#### 1.5 Scalar enum serialization

```rust
// FactVisibility
assert_eq!(serde_json::to_value(FactVisibility::GmOnly).unwrap(), json!("gm_only"));
assert_eq!(serde_json::to_value(FactVisibility::PlayerKnown).unwrap(), json!("player_known"));

// TurnMode
assert_eq!(serde_json::to_value(TurnMode::Action).unwrap(), json!("action"));

// SceneReasoningStyle
assert_eq!(serde_json::to_value(SceneReasoningStyle::TacticalCombat).unwrap(), json!("tactical_combat"));
```

#### 1.6 Default value tests

```rust
// NpcState.visible_to_player defaults to true when field absent from JSON
let json = r#"{"npc_id":"x","status":"active","location_id":null,"attitude_to_player":null,"known_facts":[],"notes":[]}"#;
let npc: NpcState = serde_json::from_str(json).unwrap();
assert!(npc.visible_to_player);  // serde(default = "default_visible_to_player")

// Fact.reveal_condition_satisfied defaults to None
let json = r#"{"id":"f1","text":"t","visibility":"gm_only","known_by":[],"source":"scenario","reveal_conditions":[],"related_secret_ids":[]}"#;
let fact: Fact = serde_json::from_str(json).unwrap();
assert!(fact.reveal_condition_satisfied.is_none());
```

### Verification checklist
- [ ] `cargo test -p domain` passes, 18+ tests
- [ ] Every NpcChange variant tag matches snake_case of the Rust enum variant name
- [ ] Every test asserts both `encode → decode == original` AND the literal wire tag string

### Anti-pattern guards
- Do NOT add `utoipa` import — domain crate does not depend on it
- Do NOT use `serde_json::from_value` for the tag assertion — use `from_str` and `contains()`

---

## Phase 2 — Reducer completeness

**File:** `crates/engine/src/reducer.rs` (add to existing `#[cfg(test)]` module)  
**Requires Docker:** No  
**Estimated tests:** ~12

### Pattern to copy from

Existing tests at `reducer.rs:218` (`reducer_applies_validated_delta_and_increments_version_once`)
and `reducer.rs:272` (`knowledge_added_creates_fact_and_registers_on_npc`).

Use the same `minimal_world_state()` and `minimal_scenario()` helper pattern.

### What to implement

#### 2.1 NpcChange::AttitudeChanged

```
delta: NpcChange::AttitudeChanged { npc_id: "npc-1", attitude: "hostile", reason: "…" }
assert: world_state.npcs[0].attitude_to_player == Some("hostile")
```

#### 2.2 NpcChange::StatusChanged

```
delta: NpcChange::StatusChanged { npc_id: "npc-1", status: NpcStatus::Unconscious, reason: "…" }
assert: world_state.npcs[0].status == NpcStatus::Unconscious
```

#### 2.3 NpcChange::LocationChanged

```
delta: NpcChange::LocationChanged { npc_id: "npc-1", location_id: "tavern", reason: "…" }
assert: world_state.npcs[0].location_id == Some("tavern")
```

#### 2.4 FactionChange::GoalRevealed

```
delta: FactionChange::GoalRevealed { faction_id: "guild", goal: "monitor calamity-levels" }
assert: world_state.factions[0].revealed_goals contains "monitor calamity-levels"
```

#### 2.5 QuestChange::Started

```
delta: QuestChange::Started { quest_id: "register" }
assert: world_state.quests[0].status == QuestStatus::Active
```

#### 2.6 QuestChange::ObjectiveCompleted

```
delta: QuestChange::ObjectiveCompleted { quest_id: "register", objective_id: "sign-form" }
assert: world_state.quests[0].completed_objectives contains "sign-form"
```

#### 2.7 QuestChange::Completed

```
delta: QuestChange::Completed { quest_id: "register", reason: "…" }
assert: world_state.quests[0].status == QuestStatus::Completed
```

#### 2.8 QuestChange::Failed

```
delta: QuestChange::Failed { quest_id: "register", reason: "…" }
assert: world_state.quests[0].status == QuestStatus::Failed
```

#### 2.9 ClockChange::SetValue

```
delta: ClockChange::SetValue { clock_id: "fame", value: 5, reason: "…" }
assert: world_state.clocks[0].current == 5
```

#### 2.10 RelationshipChange::Changed — create new relationship

```
delta: RelationshipChange::Changed { entity_a: "player", entity_b: "examiner", relationship: "cautious ally" }
world_state.relationships is empty before apply
assert: world_state.relationships.len() == 1, relationship == "cautious ally"
```

#### 2.11 RelationshipChange::Changed — update existing

```
Start with world_state.relationships = [{ entity_a: "player", entity_b: "examiner", relationship: "neutral" }]
Apply RelationshipChange with relationship: "hostile"
assert: world_state.relationships.len() == 1, relationship == "hostile"
```

#### 2.12 location_change

```
delta: WorldStateDelta { location_change: Some(LocationChange { location_id: "dungeon" }), … }
assert: world_state.current_location_id == Some("dungeon")
```

### Verification checklist
- [ ] `cargo test -p engine` passes, 12+ new tests
- [ ] Each test verifies only the one field changed by its delta; version increment also verified

### Anti-pattern guards
- Do NOT use `ValidatedWorldStateDelta::validate()` — wrap the delta directly:
  `BasicWorldStateReducer.apply(state, ValidatedWorldStateDelta(delta))`
- Check actual field names against `state.rs` before writing — `attitude_to_player`, `location_id`, etc.

---

## Phase 3 — Scene classifier completeness

**File:** `crates/engine/src/scene.rs` (add to existing `#[cfg(test)]` module)  
**Requires Docker:** No  
**Estimated tests:** ~6

### Pattern to copy from

Existing tests at `scene.rs:75` (`combat_scene_overrides_input`) and `scene.rs:83` (`investigation_keywords_select_mystery_style`).

```rust
fn classify(input: &str) -> SceneReasoningStyle {
    RuleBasedSceneClassifier.classify(input, &minimal_world_state())
}
fn minimal_world_state() -> WorldState { /* same as engine tests */ }
```

### What to implement

| Test name | Input keyword | Expected style |
|-----------|---------------|---------------|
| `political_keywords_select_negotiation` | `"I want to negotiate a deal"` | `PoliticalNegotiation` |
| `rules_keywords_select_adjudication` | `"What is my ability score?"` | `RulesAdjudication` |
| `default_input_selects_character_dialogue` | `"Hello there"` | `CharacterDialogue` |
| `combat_keywords_without_scene_override_select_combat` | `"I strike the enemy"` (no current_scene set) | `TacticalCombat` |
| `scene_override_takes_priority_over_input_keywords` | input `"negotiate deal"` but `current_scene = "combat"` | `TacticalCombat` (scene override wins) |
| `investigation_keywords_with_combat_scene_still_use_scene` | input `"I investigate"` but `current_scene = "combat"` | `TacticalCombat` |

### Verification checklist
- [ ] `cargo test -p engine` passes, 6+ new scene tests
- [ ] Confirm keyword strings actually trigger the expected branch by reading `scene.rs:11-41` before writing

---

## Phase 4 — In-memory API integration tests (no Docker)

**File to create:** `crates/api/tests/memory_api_flows.rs`  
**Requires Docker:** No  
**Estimated tests:** ~20  
**Why this is the highest-value phase:** Currently zero API routes are tested without Docker. Every route error path is invisible in CI without a running Postgres container.

### Setup pattern

Copy `common/mod.rs` helpers (`send_json`, `send_empty`, `json_body`, `mock_provider`, `sample_scenario`).
Add a `memory_test_context(provider)` helper (no testcontainers, no async, no cleanup):

```rust
pub fn memory_test_context(provider: Arc<dyn LlmProvider>) -> Router {
    let mut config = AppConfig::default();
    config.storage.backend = StorageBackend::Memory;
    let state = AppState::from_parts(
        config,
        Arc::new(ApiStore::default()),
        provider,
        Arc::new(InMemorySessionTurnLock::default()),
    );
    app_router(state)
}
```

**Source:** `crates/api/src/state.rs:from_parts()`, `crates/api/src/app.rs:app_router()`

### What to implement

#### 4.1 Health check

```
GET /health → 200, status=ok, database=memory
```

#### 4.2 Scenario CRUD (4 tests)

```
POST /scenarios → 200, returns created scenario
GET /scenarios → 200, lists created scenario
PUT /scenarios/:id → 200, title updated
DELETE /scenarios/:id → 200, deleted:true
GET /scenarios/:id after delete → 404
```

#### 4.3 Session lifecycle (3 tests)

```
POST /sessions with unknown scenario_id → 404
POST /sessions with valid scenario_id → 200, SessionRecord returned
GET /sessions/:id → 200, matches created record
DELETE /sessions/:id → 200, deleted:true
GET /sessions after delete → empty list
```

#### 4.4 Session provider assignment

```
PATCH /sessions/:id/provider with { provider_id: uuid } → 200
GET /sessions/:id → provider_id matches assigned uuid
PATCH with null provider_id → clears provider_id
```

#### 4.5 Provider management (3 tests)

```
POST /providers → 201, ProviderRecord returned
GET /providers → 200, contains created provider
DELETE /providers/:id → 200
GET /providers after delete → empty list
```

#### 4.6 Turn on missing session → 404

```
POST /sessions/nonexistent-uuid/turn → 404
```

#### 4.7 World-state on fresh session has version 0

```
POST /sessions → GET /sessions/:id/world-state → version field = 0
```

#### 4.8 Full in-memory turn cycle (no Docker!)

```
POST /scenarios → POST /sessions → POST /sessions/:id/turn
mock_provider returns valid delta
assert: 200, world_state_version = 1, player_response non-empty
```

#### 4.9 AppState::resolve_provider unit test

Not in the router test — add directly to `state.rs` test module:

```rust
#[tokio::test]
async fn resolve_provider_returns_registry_entry_when_session_has_provider_id() {
    let default_provider = Arc::new(MockProvider::new("default", []));
    let registry_provider = Arc::new(MockProvider::new("registry", []));
    let id = Uuid::new_v4();
    let mut registry = HashMap::new();
    registry.insert(id, registry_provider.clone() as Arc<dyn LlmProvider>);
    let state = AppState { /* ... */ provider_registry: Arc::new(RwLock::new(registry)), … };
    let resolved = state.resolve_provider(Some(id)).await;
    assert_eq!(resolved.capabilities().supports_streaming, registry_provider.capabilities().supports_streaming);
}

#[tokio::test]
async fn resolve_provider_falls_back_to_default_when_id_not_in_registry() {
    // provider_id Some(unknown_uuid) → falls back to state.provider
}

#[tokio::test]
async fn resolve_provider_returns_default_when_no_provider_id() {
    // provider_id None → returns state.provider
}
```

### Verification checklist
- [ ] `cargo test -p api` passes all new tests without any Docker
- [ ] `memory_test_context()` helper is in `common/mod.rs` (not duplicated per test)
- [ ] No `#[ignore]` on any Phase 4 test

### Anti-pattern guards
- Do NOT use `postgres_test_context` in this phase — these tests must run without Docker
- Do NOT call `ctx.cleanup()` — no cleanup needed for in-memory store

---

## Phase 5 — process_turn_debug unit test

**File:** `crates/engine/src/pipeline.rs` (add to existing `#[cfg(test)]` module)  
**Requires Docker:** No  
**Estimated tests:** 2

### Pattern to copy

Existing at `pipeline.rs:570` (`non_streaming_turn_applies_valid_delta_and_projects_state`).
Same `InMemoryTurnStore` + `MockProvider` scaffolding.

### What to implement

#### 5.1 debug turn returns applied_delta matching provider output

```
MockProvider queued with valid delta JSON
process_turn_debug() called
assert: result.turn.world_state_version == 1
assert: result.applied_delta.faction_changes.len() == 1
assert: result.applied_delta.faction_changes[0] matches the faction_id in the mock response
```

#### 5.2 debug turn applies state normally (same as regular turn)

```
assert: persisted world state version == 1 (not 0)
assert: messages persisted == 2 (user + assistant)
```

### Verification checklist
- [ ] `cargo test -p engine` passes, 2 new pipeline tests
- [ ] Assert `applied_delta` fields, not just turn response fields

---

## Phase 6 — Persistence repository tests (Docker-gated)

**File to create:** `crates/persistence/tests/repository_tests.rs`  
**Requires Docker:** Yes (testcontainers)  
**Estimated tests:** ~22

Add `testcontainers-modules` to `crates/persistence/Cargo.toml` dev-deps:

```toml
[dev-dependencies]
testcontainers-modules = { version = "0.15", features = ["postgres"] }
tokio.workspace = true
uuid.workspace = true
```

### Setup pattern

```rust
async fn pg() -> (PgPersistence, ContainerAsync<Postgres>) {
    let container = Postgres::default().with_db_name("test").with_user("test").with_password("test").start().await.unwrap();
    let url = format!("postgres://test:test@{}:{}/test", container.get_host().await.unwrap(), container.get_host_port_ipv4(5432).await.unwrap());
    let p = PgPersistence::connect(&url).await.unwrap();
    p.migrate().await.unwrap();
    (p, container)
}
```

### What to implement

#### 6.1 ScenarioRepository (5 tests)
- `create_and_get_scenario` — insert scenario, get by id, assert title and definition match
- `list_scenarios` — insert 2, list returns both
- `update_scenario` — create, update title, get confirms new title
- `delete_scenario` — create, delete, get returns None
- `get_scenario_not_found` — get with unknown uuid returns Ok(None)

#### 6.2 SessionRepository (5 tests)
- `create_and_get_session` — create scenario first, create session, get returns record
- `list_sessions` — create 2 sessions, list returns both
- `delete_session` — create, delete, get returns None
- `set_provider` — create session, set provider_id, get confirms provider_id matches
- `set_provider_unknown_session` — set_provider on unknown uuid returns RepoError::NotFound

#### 6.3 WorldStateRepository (3 tests)
- `save_and_get_world_state` — persist world state, get returns same state
- `save_increments_version` — save version 1, then save version 2, get returns v2
- `get_unknown_session_returns_none` — get with no world state returns Ok(None)

#### 6.4 MessageRepository (2 tests)
- `append_and_recent_returns_last_n` — append 4 messages, recent(3) returns 3 most recent in order
- `append_stores_all_fields` — verify role, speaker_id, scene_type, raw_provider_output round-trip

#### 6.5 EventRepository (2 tests)
- `append_and_list_events` — append 2 events, list returns both in order
- `list_returns_empty_for_unknown_session` — list for unknown session id returns Ok([])

#### 6.6 ProviderConfigRepository (4 tests)
- `create_and_get_provider` — create record, get by id, fields match
- `get_by_name` — create, get_by_name returns same record
- `delete_provider` — create, delete, get returns None
- `get_default_returns_default_provider` — create 2 providers, one is_default=true, get_default returns it

#### 6.7 PostgresSessionTurnLock (3 tests)
- `acquire_lock_succeeds_on_fresh_session` — create session, acquire lock returns guard
- `second_acquire_returns_already_in_progress` — hold guard, second acquire returns TurnLockError::AlreadyInProgress  
- `stale_lock_recovered` — manually set processing_turn=true AND processing_turn_started_at to 10 minutes ago in DB, then acquire succeeds (stale recovery)

**Source for SQL:** `crates/persistence/src/lock.rs:19-56`

### Verification checklist
- [ ] All tests `#[ignore = "requires docker daemon via testcontainers"]`
- [ ] `cargo test -p persistence -- --ignored` passes (requires Docker)
- [ ] `cargo test -p persistence` (without `--ignored`) passes in CI (0 tests run, 0 fail)

### Anti-pattern guards
- Do NOT test `TurnStateStore::persist_successful_turn` here — that is complex and already tested via API integration tests
- DO test each repository trait method in isolation; do NOT chain repository calls across methods

---

## Final Phase — Verification

Run full suite and confirm counts:

```bash
# Non-Docker tests (should all pass in CI):
cargo test --workspace

# Docker tests (requires local Docker daemon):
cargo test --workspace -- --ignored
```

Expected final counts (approximate):
| Category | Before | After |
|----------|--------|-------|
| Unit / inline | 58 | ~88 (+30) |
| Non-Docker integration | 5 | ~27 (+22) |
| Docker-gated integration | 13 | ~35 (+22) |
| **Total** | **76** | **~150** |

---

## Glossary of Existing Test Helpers

| Helper | Location | Purpose |
|--------|----------|---------|
| `mock_provider(responses)` | `crates/api/tests/common/mod.rs:67` | Create `Arc<dyn LlmProvider>` with queued string responses |
| `postgres_test_context(provider)` | `crates/api/tests/common/mod.rs:33` | Spin up Docker Postgres + run migrations + return TestContext |
| `sample_scenario()` | `crates/api/tests/common/mod.rs:119` | Canonical test scenario with NPC "examiner", faction "guild", clock "fame" |
| `send_json(router, method, path, body)` | `crates/api/tests/common/mod.rs:71` | Make HTTP request, return (StatusCode, Bytes) |
| `send_empty(router, method, path)` | `crates/api/tests/common/mod.rs:94` | Same but no body |
| `json_body::<T>(bytes)` | `crates/api/tests/common/mod.rs:115` | Deserialize response bytes to T |
| `InMemoryTurnStore` | `crates/engine/src/pipeline.rs` (test module) | In-memory store used by engine unit tests |
| `minimal_world_state()` | `crates/engine/src/reducer.rs` (test module) | WorldState with 1 NPC "npc-1", 1 faction "guild", 1 clock "fame" |
