
# Advanced Roleplaying Engine — Canonical Guide and Current Fix Plan

## Purpose

This is the consolidated, up-to-date Markdown guide for the advanced roleplaying engine project.

It merges the current concept guide, the Rust implementation blueprint, and the current phased fix plan into one document for coding agents. The previous two fix-plan files contained identical content, so the fix plan is included once here.

Repository target:

```text
https://github.com/hofdo/advanced-roleplaying-framework-prototype
```

## How to Use This Document

Use this file as the canonical handoff for Codex or another coding agent.

Recommended execution order:

1. Read **Part I** to understand the product and architecture concept.
2. Read **Part II** to implement or modify the Rust backend correctly.
3. Execute **Part III** phase by phase to close the remaining hardening gaps.

Hard rules that apply across the whole project:

- The frontend must not build prompts.
- The frontend must not mutate authoritative world state directly.
- The frontend must not receive GM-only or raw authoritative state.
- The LLM must never directly overwrite full world state.
- The LLM may only propose typed deltas.
- The backend validates, reduces, persists, and projects state.
- Normal responses return projected frontend-safe state, not raw deltas.
- Admin/debug access must be explicitly guarded before any non-local use.

## Document Map

- **Part I — Current Concept Guide**: product concept, runtime architecture, endpoint contract, state model, and technology guidance.
- **Part II — Rust Implementation Blueprint**: concrete Rust module layout, types, traits, SQL schema, API routes, provider abstraction, and implementation order.
- **Part III — Current Fix Plan and Phased Execution**: current repository status, remaining issues, and ordered coding tasks.


---

# Part I — Current Concept Guide

## Advanced Roleplaying Engine — Current Concept Guide

### Purpose

This guide defines the target architecture for an advanced roleplaying engine that feeds LLMs and exposes clean endpoints to frontend storyteller applications.

The engine is responsible for:

1. receiving player input,
2. loading scenario/session/world state,
3. building compact role-aware context,
4. calling an LLM provider,
5. returning player-visible storyteller output,
6. validating proposed world-state changes,
7. applying those changes through deterministic reducers,
8. projecting safe frontend-visible state.

The frontend should not build prompts, mutate authoritative world state directly, or receive GM-only state.

---

### Current Prototype State

The current Rust prototype now implements the intended high-level runtime architecture:

```text
api crate
+ domain crate
+ engine crate
+ providers crate
+ persistence crate
+ shared crate
```

Implemented or mostly implemented:

- Axum API surface,
- scenario/session/world-state concepts,
- role identity and faction identity,
- `NpcStatus`,
- typed `WorldStateDelta`,
- provider abstraction and provider registry,
- OpenAI-compatible provider,
- session-scoped provider route and persistence shape,
- non-streaming turn pipeline,
- streaming turn route with second-call delta finalization,
- frontend state projection,
- normal turn responses without raw deltas,
- projected normal session export,
- PostgreSQL schema baseline,
- prompt version metadata,
- hidden reasoning stripping,
- JSON repair for structured delta parsing,
- provider retry policy for transport/provider errors,
- session turn lock abstraction and Postgres lock wiring,
- stronger secret validation fields such as `related_secret_ids` and `reveal_condition_satisfied`,
- explicit NPC visibility for projection.

The current priority is hardening and test coverage, not adding more gameplay features.

Remaining guide-level priorities:

```text
P1: protect /admin/* routes with an admin guard.
P1: filter GM-only facts by relevance before prompt construction.
P2: prove Postgres turn locking with integration tests.
P2: clamp clocks and faction standing defensively inside reducers.
P2: make explicit session-provider lookup strict when selected provider is missing.
P2: enforce/debug-test raw_provider_output remains null by default.
P3: add roleplay-quality fixtures and prompt snapshot tests.
```

---

### Non-Negotiable Rules

#### 1. The LLM proposes; the engine disposes

The LLM may propose narration and deltas, but it must never directly overwrite full world state.

```text
LLM output -> parse -> validate -> reducer -> persisted state
```

#### 2. The frontend receives projected state only

Normal frontend routes must receive `FrontendVisibleState` or `FrontendStatePatch`, not raw authoritative `WorldState`.

Raw state may contain:

- GM-only facts,
- hidden NPC knowledge,
- hidden faction goals,
- unrevealed clocks,
- raw deltas,
- backend/debug metadata.

#### 3. No generic world-state PATCH in production

Avoid:

```http
PATCH /sessions/{session_id}/world-state
```

Prefer typed mutations:

```http
POST /sessions/{session_id}/facts
POST /sessions/{session_id}/quests/{quest_id}/complete
POST /sessions/{session_id}/clocks/{clock_id}/advance
POST /sessions/{session_id}/relationships
```

#### 4. One active turn per session

Only one turn may process per session at a time.

Concurrent turns should return:

```http
409 Conflict
```

This is especially important for streaming turns.

#### 5. Streaming must not mutate from partial text

Use this safe streaming model:

```text
Call 1: stream visible response
Call 2: extract structured delta
Validate delta
Apply reducer
Send final SSE event
```

#### 6. Provider choice should be session-scoped

Avoid global active provider state except in single-user local mode.

Preferred resolution order:

```text
if session.provider_id is None -> default provider
if session.provider_id is Some(id) -> that exact provider or explicit error
```

Do not silently fall back to default when a session explicitly references a missing, disabled, or unhealthy provider.

#### 7. Admin/debug routes must be guarded

Routes under `/admin/*` may expose raw world state, raw deltas, raw provider output, hidden reasoning, or GM-only facts. They must be disabled or protected before any non-local use.

Minimum rule:

```text
/admin/* requires ADMIN_TOKEN or equivalent admin authorization.
```

#### 8. GM-only context must be relevant

Do not pass unrelated secrets to the model. Retrieve GM-only facts only when relevant to the current scene, active role, active location, active quest, active clock, or explicit reveal condition.

---

### Core Runtime Pipeline

Every player turn should follow this flow:

```text
1. Acquire session turn lock.
2. Load session, scenario, world state, and recent messages.
3. Classify scene type.
4. Resolve active speaker or narrator mode.
5. Activate role identity.
6. Select scene reasoning directive.
7. Build compact prompt context.
8. Call LLM provider.
9. Parse output.
10. Strip hidden reasoning.
11. Validate proposed delta.
12. Apply reducer.
13. Persist messages, delta, events, and new world-state version.
14. Project frontend-visible state.
15. Return response to frontend.
16. Release session turn lock.
```

---

### Role-Aware Context

The engine should not ask the model to “roleplay better” in a vague way. It should provide structured context.

#### Role Identity Activation

For active NPCs or narrator modes, include:

- name,
- description,
- current emotion,
- motivation,
- worldview,
- fear/desire,
- speech style,
- boundaries,
- known facts,
- forbidden behavior.

#### Scene Reasoning Style

Scene type controls response behavior:

| Scene | Prioritize | Avoid |
|---|---|---|
| `character_dialogue` | NPC voice, motivation, relationship | generic assistant tone |
| `political_negotiation` | leverage, faction goals, reputation | instant loyalty |
| `tactical_combat` | clear action, objectives, collateral stakes | vague cinematic fog |
| `mystery_investigation` | clues, evidence, reveal thresholds | premature secret reveal |
| `rules_adjudication` | clear ruling, consistency | hidden arbitrary rule changes |
| `world_simulation` | clocks, faction movement, consequences | static world |

---

### Domain Model Essentials

#### Scenario

Static campaign seed:

```json
{
  "id": "uuid",
  "title": "Chosen Beyond the Goddess",
  "scenario_type": "adventure",
  "setting": "High fantasy isekai world",
  "tone": "heroic, consequence-driven",
  "locations": [],
  "factions": [],
  "npcs": [],
  "quests": [],
  "secrets": [],
  "clocks": [],
  "rules": []
}
```

#### NPC

```json
{
  "id": "seraphyne",
  "name": "Seraphyne",
  "status": "active",
  "role_identity": {
    "core_emotion": "protective but worried",
    "motivation": "guide the player without letting unknown forces exploit them",
    "worldview": "power requires responsibility",
    "speech_style": "warm, solemn, restrained",
    "boundaries": [
      "cannot remove powers she did not grant",
      "does not know the full truth about the force beyond the gods"
    ]
  }
}
```

#### NPC Status

Use status to model whether an NPC can act and how the frontend should present them.

Recommended statuses:

```text
active
injured
unconscious
missing
captured
dead
hidden
unknown
```

Do not hide NPCs only because they are `missing`. A missing NPC can be player-visible if the player knows they are missing.

Use explicit visibility:

```json
{
  "status": "missing",
  "visible_to_player": true
}
```

#### Fact

```json
{
  "id": "void-mark-origin",
  "text": "The player's soul-mark was not created by the goddess.",
  "visibility": "gm_only",
  "reveal_conditions": [
    "the player directly asks Seraphyne about the mark",
    "the player studies a void relic"
  ]
}
```

#### World-State Delta

Use typed deltas. Avoid arbitrary `field/value` patches.

Good:

```json
{
  "npc_changes": [
    {
      "type": "knowledge_added",
      "npc_id": "guildmaster-brannic",
      "fact": "The player has abnormal mana output.",
      "visibility": "npc_known",
      "reason": "The guild instruments overloaded during the test."
    }
  ]
}
```

Bad:

```json
{
  "npc_changes": [
    {
      "npc_id": "guildmaster-brannic",
      "field": "whatever",
      "value": "anything"
    }
  ]
}
```

---

### Frontend State Projection

The engine must expose projected state to frontends.

```json
{
  "state_version": 12,
  "current_location": {},
  "active_speaker": {},
  "visible_npcs": [],
  "visible_quests": [],
  "visible_clocks": [],
  "player_known_facts": [],
  "recent_public_events": []
}
```

Projection must remove:

- GM-only facts,
- NPC-only knowledge,
- unrevealed secrets,
- hidden faction goals,
- internal delta reasons if not player-visible,
- raw prompts,
- raw provider output,
- hidden reasoning.

---

### API Surface

Minimum useful API:

```http
GET  /health
GET  /providers
POST /providers/test
PATCH /sessions/{session_id}/provider

POST /scenarios
GET  /scenarios
GET  /scenarios/{scenario_id}
PUT  /scenarios/{scenario_id}
DELETE /scenarios/{scenario_id}

POST /sessions
GET  /sessions
GET  /sessions/{session_id}
DELETE /sessions/{session_id}
GET  /sessions/{session_id}/export

POST /sessions/{session_id}/turn
POST /sessions/{session_id}/turn/stream

GET /sessions/{session_id}/world-state
GET /sessions/{session_id}/events
```

Normal exports must be player-visible. Raw exports must be admin/debug-only.

---

### Current Priority Fixes

The current prototype should be improved in this order:

1. Implement real session provider persistence and resolution.
2. Make normal session export player-visible only; move raw export to admin/debug.
3. Add database-backed/advisory turn locking for PostgreSQL mode.
4. Refactor streaming and non-streaming turn finalization to share validation/reducer/persistence logic.
5. Ensure `NpcChange::KnowledgeAdded` writes to `NpcState.known_facts`, not notes.
6. Add one controlled JSON repair retry.
7. Implement provider retry policy for timeout/transport/5xx only.
8. Strengthen secret leak validation beyond exact string matching.
9. Add `visible_to_player` to NPC projection logic.
10. Distinguish provider configuration health from provider readiness.
11. Align repository license and Cargo license metadata.

---

### What Not To Build Yet

Do not add these until the core turn pipeline is hardened:

- autonomous multi-agent loops,
- fine-tuning or distillation pipelines,
- complex benchmark reproduction,
- full D&D rules automation,
- vector memory,
- realtime multiplayer,
- complex auth/permissions beyond admin/debug protection,
- huge endpoint surface.

---

### Coding Agent Brief

Build and harden the roleplaying engine around this core flow:

```text
player input
-> session turn lock
-> context builder
-> role-aware prompt
-> LLM provider
-> visible response + proposed typed delta
-> parser/stripper
-> delta validator
-> reducer
-> persistence
-> frontend state projector
-> frontend response
```

Do not let the LLM overwrite full world state. Do not expose GM-only state. Do not mutate state from streamed partial text. Do not let provider-specific behavior leak into the domain model.

---

# Part II — Rust Implementation Blueprint

## Advanced Roleplaying Engine — Rust Implementation Blueprint

### Purpose

Build a Rust backend service that:

1. exposes HTTP APIs for roleplaying frontends,
2. calls one or more LLM providers,
3. builds role-aware prompts,
4. returns player-visible storyteller output,
5. validates and applies world-state deltas,
6. persists sessions, messages, events, and world state,
7. projects safe frontend-visible state without leaking GM-only data.

This is a pragmatic implementation guide. It intentionally excludes research-reproduction, fine-tuning, and benchmark-heavy work from the core path.

---


### Current Prototype Alignment

The current prototype now implements most of the intended runtime architecture:

- Axum API scaffold,
- scenario/session/world-state domain concepts,
- role identity and faction identity,
- `NpcStatus`,
- typed `WorldStateDelta`,
- frontend state projection,
- provider abstraction and provider registry,
- OpenAI-compatible provider,
- provider persistence shape,
- session-scoped provider route,
- PostgreSQL persistence baseline,
- non-streaming turn pipeline,
- streaming visible response with shared finalization,
- hidden reasoning stripping,
- prompt version metadata,
- JSON repair for structured delta parsing,
- provider retries for retryable transport/provider failures,
- stronger secret validation fields,
- projected normal turn responses and projected normal export.

The current priority is safety and reliability hardening. Do not add advanced gameplay features until the following gaps are handled.

#### Current remaining gaps

```text
P1: protect /admin/* routes with admin authorization or disable them outside local debug.
P1: GM-only fact retrieval must be relevance-based, not first-N secret retrieval.
P2: prove Postgres turn locking with integration tests.
P2: reducer should defensively clamp clocks and faction standing.
P2: session-selected provider resolution should error if selected provider is missing.
P2: raw_provider_output must remain NULL by default and be tested.
P3: add prompt snapshots and roleplay-quality regression fixtures.
```

---

### Recommended Rust Stack

Use:

```text
axum            HTTP API framework
tokio           async runtime
serde           serialization
serde_json      JSON handling
sqlx            async PostgreSQL access
postgres        primary database
reqwest         outbound LLM HTTP calls
utoipa          OpenAPI generation
tracing         logs/spans
tower-http      CORS, tracing, compression, timeouts
validator       request validation
thiserror       domain errors
anyhow          application setup errors
uuid            IDs
time            timestamps
async-trait     async provider traits
futures         streams
insta           snapshot tests
wiremock        mock LLM provider tests
```

Optional later:

```text
pgvector        semantic memory
redis           distributed locks / rate limits
opentelemetry   distributed tracing
```

---

### Workspace Layout

Use a Rust workspace.

```text
roleplaying-engine/
  Cargo.toml
  crates/
    api/
    domain/
    engine/
    providers/
    persistence/
    shared/
```

#### `domain`

Pure domain types and validation helpers.

Contains:

```text
Scenario
WorldState
Npc
NpcStatus
Faction
Quest
Clock
Fact
WorldStateDelta
SceneReasoningStyle
RoleIdentity
FactionIdentity
FrontendVisibleState
```

No HTTP, no SQL, no LLM code.

#### `engine`

Core orchestration.

Contains:

```text
TurnPipeline
ContextBuilder
SceneClassifier
RoleIdentityActivator
ReasoningStyleOptimizer
PromptBuilder
ResponseParser
DeltaValidator
WorldStateReducer
FrontendStateProjector
SessionTurnLock
ConsistencyAuditor
```

#### `providers`

LLM provider abstraction.

Contains:

```text
LlmProvider trait
OpenAiCompatibleProvider
MockProvider
ProviderCapabilities
```

#### `persistence`

Database access.

Contains:

```text
ScenarioRepository
SessionRepository
WorldStateRepository
MessageRepository
EventRepository
ProviderConfigRepository
```

#### `api`

Axum routes, request/response DTOs, OpenAPI.

Contains:

```text
routes
handlers
middleware
app state
error mapping
SSE support
```

#### `shared`

Shared utilities.

Contains:

```text
ids
time helpers
config
error helpers
```

---

### Domain Types

#### IDs

Use UUIDs for persisted entities. Scenario content may still contain stable string IDs for locations/NPCs/factions.

```rust
pub type SessionId = uuid::Uuid;
pub type ScenarioId = uuid::Uuid;
pub type MessageId = uuid::Uuid;

pub type EntityKey = String;
```

---

### Scenario

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub id: ScenarioId,
    pub title: String,
    pub scenario_type: ScenarioType,
    pub setting: String,
    pub tone: String,
    pub rules: Vec<String>,
    pub locations: Vec<Location>,
    pub factions: Vec<Faction>,
    pub npcs: Vec<Npc>,
    pub quests: Vec<Quest>,
    pub secrets: Vec<Secret>,
    pub clocks: Vec<ClockTemplate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioType {
    Adventure,
}
```

---

### Role Identity

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleIdentity {
    pub core_emotion: String,
    pub motivation: String,
    pub worldview: String,
    pub fear: Option<String>,
    pub desire: Option<String>,
    pub speech_style: String,
    pub boundaries: Vec<String>,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Npc {
    pub id: EntityKey,
    pub name: String,
    pub description: String,
    pub role_identity: RoleIdentity,
    pub stats: Option<CharacterStats>,
    pub initial_status: NpcStatus,
}
```

Keep this small. Long character essays hurt context quality.

---

### NPC Status

NPC status must be typed. Do not leave this as free-form text because status changes affect prompt context, quest logic, combat, visibility, and frontend rendering.

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NpcStatus {
    Active,
    Injured,
    Unconscious,
    Missing,
    Captured,
    Dead,
    Hidden,
    Unknown,
}
```

#### Status semantics

| Status | Meaning | Prompt/engine behavior |
|---|---|---|
| `Active` | NPC is alive and able to act. | Can speak, move, plan, and participate normally. |
| `Injured` | NPC is alive but impaired. | May need help, may avoid combat, can create urgency. |
| `Unconscious` | NPC is alive but cannot act. | Cannot speak or make decisions unless magic/setting allows. |
| `Missing` | NPC location is unknown. | Can be used for quests, rumors, searches. |
| `Captured` | NPC is held by another actor/faction. | Enables rescue, ransom, coercion, faction leverage. |
| `Dead` | NPC is dead. | Cannot act unless undeath/resurrection is established. |
| `Hidden` | NPC is deliberately concealed. | GM may know location; player may not. |
| `Unknown` | Engine lacks reliable status. | Use uncertainty in narration and avoid hard claims. |

#### NPC runtime state

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcState {
    pub npc_id: EntityKey,
    pub status: NpcStatus,
    pub location_id: Option<EntityKey>,
    pub attitude_to_player: Option<String>,
    pub known_facts: Vec<EntityKey>,
    pub notes: Vec<String>,
}
```

#### Status validation rules

- Reject `Dead -> Active` unless a resurrection/revival event exists.
- Reject `Dead` NPCs speaking unless the scenario supports ghosts, undeath, recordings, or resurrection.
- Reject `Unconscious` NPCs making plans or negotiations.
- Reject player-visible certainty about `Hidden` or `Missing` NPCs unless discovered.
- Require a reason for every status change.

---

### Faction Identity

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactionIdentity {
    pub public_goal: String,
    pub hidden_goal: Option<String>,
    pub values: Vec<String>,
    pub fears: Vec<String>,
    pub methods: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Faction {
    pub id: EntityKey,
    pub name: String,
    pub description: String,
    pub faction_identity: FactionIdentity,
    pub initial_standing: i32,
}
```

---

### World State

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldState {
    pub session_id: SessionId,
    pub scenario_id: ScenarioId,
    pub version: i64,
    pub current_location_id: Option<EntityKey>,
    pub current_scene: Option<String>,
    pub active_speaker_id: Option<EntityKey>,
    pub facts: Vec<Fact>,
    pub npcs: Vec<NpcState>,
    pub factions: Vec<FactionState>,
    pub quests: Vec<QuestState>,
    pub clocks: Vec<ClockState>,
    pub relationships: Vec<RelationshipState>,
    pub inventory: Vec<InventoryItem>,
    pub summary: Option<String>,
    pub recent_events: Vec<String>,
}
```

#### Facts

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    pub id: EntityKey,
    pub text: String,
    pub visibility: FactVisibility,
    pub known_by: Vec<EntityKey>,
    pub source: FactSource,
    pub reveal_conditions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactVisibility {
    PlayerKnown,
    GmOnly,
    NpcKnown,
    FactionKnown,
}
```

#### Clocks

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockState {
    pub id: EntityKey,
    pub title: String,
    pub current: u8,
    pub max: u8,
    pub consequence: String,
}
```

---

### Scene Reasoning Style

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SceneReasoningStyle {
    CharacterDialogue,
    EmotionalScene,
    PoliticalNegotiation,
    MysteryInvestigation,
    TacticalCombat,
    WorldSimulation,
    RulesAdjudication,
    TravelExploration,
    Downtime,
    QuestResolution,
}
```

---

### World-State Delta

Use typed delta enums. Do not support arbitrary JSON Patch or generic `field`/`value` mutations as the default. The model may only propose known change variants.

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorldStateDelta {
    pub facts_to_add: Vec<FactToAdd>,
    pub npc_changes: Vec<NpcChange>,
    pub faction_changes: Vec<FactionChange>,
    pub quest_changes: Vec<QuestChange>,
    pub clock_changes: Vec<ClockChange>,
    pub relationship_changes: Vec<RelationshipChange>,
    pub location_change: Option<LocationChange>,
    pub event_log_entries: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactToAdd {
    pub text: String,
    pub visibility: FactVisibility,
    pub known_by: Vec<EntityKey>,
    pub reveal_conditions: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NpcChange {
    AttitudeChanged {
        npc_id: EntityKey,
        attitude: String,
        reason: String,
    },
    KnowledgeAdded {
        npc_id: EntityKey,
        fact: String,
        visibility: FactVisibility,
        reason: String,
    },
    StatusChanged {
        npc_id: EntityKey,
        status: NpcStatus,
        reason: String,
    },
    LocationChanged {
        npc_id: EntityKey,
        location_id: EntityKey,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FactionChange {
    StandingChanged {
        faction_id: EntityKey,
        standing_delta: i32,
        reason: String,
    },
    GoalRevealed {
        faction_id: EntityKey,
        goal: String,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClockChange {
    Advanced {
        clock_id: EntityKey,
        delta: i8,
        reason: String,
    },
    SetValue {
        clock_id: EntityKey,
        value: u8,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QuestChange {
    Started { quest_id: EntityKey, reason: String },
    ObjectiveCompleted { quest_id: EntityKey, objective_id: EntityKey, reason: String },
    Completed { quest_id: EntityKey, reason: String },
    Failed { quest_id: EntityKey, reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RelationshipChange {
    Changed {
        source_id: EntityKey,
        target_id: EntityKey,
        attitude_delta: i32,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationChange {
    pub location_id: EntityKey,
    pub reason: String,
}
```

Every consequential change must include a reason. Unknown change types must be rejected by deserialization or validation.

---

### Provider Resolution Policy

Provider selection must be predictable and session-scoped.

Resolution rules:

```text
if session.provider_id is None:
    use default provider

if session.provider_id is Some(id):
    use that exact provider
    if missing/disabled/unhealthy -> return provider_not_available
```

Do not silently fall back to default when a session explicitly references a provider. Silent fallback hides configuration errors and makes debugging model behavior difficult.

Provider health and readiness are different:

```text
health: provider is configured in the engine
readiness: provider endpoint is reachable and usable
```

---

### Provider Abstraction

```rust
use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn health(&self) -> Result<ProviderHealth, ProviderError>;
    fn capabilities(&self) -> ProviderCapabilities;
    async fn generate(&self, request: LlmRequest) -> Result<LlmResponse, ProviderError>;
    async fn stream(
        &self,
        request: LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String, ProviderError>> + Send>>, ProviderError>;
}

#[derive(Debug, Clone)]
pub struct ProviderCapabilities {
    pub supports_streaming: bool,
    pub supports_json_mode: bool,
    pub supports_tool_calls: bool,
    pub supports_seed: bool,
    pub max_context_tokens: Option<u32>,
    pub request_timeout_seconds: u64,
    pub stream_idle_timeout_seconds: u64,
    pub max_retries: u8,
}
```

Start with an OpenAI-compatible provider.

Provider policy:

- set request timeout and stream idle timeout explicitly,
- retry only transport errors, timeouts, and provider `5xx` responses,
- do not retry malformed model output as a transport failure,
- use one controlled repair prompt at most for malformed structured output,
- do not log API keys, raw prompts, or raw provider responses unless explicit local debug mode is enabled.

```rust
pub struct OpenAiCompatibleProvider {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub client: reqwest::Client,
    pub capabilities: ProviderCapabilities,
}
```

Supported targets can include:

- local `llama-server`,
- vLLM,
- Ollama with compatible API if configured,
- OpenAI-compatible hosted APIs.

---

### Engine Pipeline

#### Turn Pipeline

```rust
pub struct TurnPipeline<C, P, R, V, L, S> {
    pub context_builder: C,
    pub provider: P,
    pub repositories: R,
    pub validator: V,
    pub turn_lock: L,
    pub frontend_state_projector: S,
}
```

Execution flow:

```text
1. Acquire session turn lock. Return 409 Conflict if another turn is already active.
2. Load scenario/session/world state.
3. Classify scene.
4. Activate role identity.
5. Build compact context.
6. Build prompt.
7. Call provider.
8. Parse response.
9. Strip hidden reasoning.
10. Validate delta.
11. Apply delta.
12. Persist message, event, delta, world state.
13. Project frontend-visible state.
14. Return API response.
15. Release session turn lock.
```

---

### Scene Classifier

Start rule-based.

```rust
pub trait SceneClassifier {
    fn classify(&self, input: &str, world_state: &WorldState) -> SceneReasoningStyle;
}

pub struct RuleBasedSceneClassifier;

impl SceneClassifier for RuleBasedSceneClassifier {
    fn classify(&self, input: &str, world_state: &WorldState) -> SceneReasoningStyle {
        let lower = input.to_lowercase();

        if world_state.current_scene.as_deref() == Some("combat") {
            return SceneReasoningStyle::TacticalCombat;
        }

        if contains_any(&lower, &["attack", "cast", "strike", "dodge", "shoot"]) {
            return SceneReasoningStyle::TacticalCombat;
        }

        if contains_any(&lower, &["negotiate", "convince", "threaten", "deal", "bargain"]) {
            return SceneReasoningStyle::PoliticalNegotiation;
        }

        if contains_any(&lower, &["inspect", "search", "investigate", "clue", "examine"]) {
            return SceneReasoningStyle::MysteryInvestigation;
        }

        if contains_any(&lower, &["class", "stats", "rule", "ability", "level"]) {
            return SceneReasoningStyle::RulesAdjudication;
        }

        SceneReasoningStyle::CharacterDialogue
    }
}
```

Do not use an LLM classifier until this becomes insufficient.

---

### Role Identity Activation

This is the practical mechanism that keeps roleplay from becoming generic.

```rust
pub struct RoleActivationContext {
    pub active_role_name: Option<String>,
    pub emotion_now: Option<String>,
    pub motivation_now: Option<String>,
    pub knowledge_boundaries: Vec<String>,
    pub forbidden_moves: Vec<String>,
    pub speech_constraints: Vec<String>,
}

pub trait RoleIdentityActivator {
    fn activate(
        &self,
        scenario: &Scenario,
        world_state: &WorldState,
        scene_style: SceneReasoningStyle,
    ) -> RoleActivationContext;
}
```

Implementation rule:

- if an NPC is active, activate that NPC,
- otherwise activate narrator/GM mode,
- include only relevant identity fields,
- include explicit knowledge boundaries,
- do not activate NPCs with statuses that prevent action unless the setting explicitly allows it.

---

### Reasoning Style Optimization

Do not make the model “think more” everywhere. Give it the right reasoning shape for the scene.

```rust
pub struct ReasoningStyleDirective {
    pub style: SceneReasoningStyle,
    pub priorities: Vec<String>,
    pub avoid: Vec<String>,
    pub visible_response_shape: String,
}

pub trait ReasoningStyleOptimizer {
    fn directive_for(&self, style: SceneReasoningStyle) -> ReasoningStyleDirective;
}
```

Example directive:

```rust
ReasoningStyleDirective {
    style: SceneReasoningStyle::PoliticalNegotiation,
    priorities: vec![
        "track leverage".into(),
        "preserve faction interests".into(),
        "show public and private consequences".into(),
    ],
    avoid: vec![
        "instant loyalty change".into(),
        "generic exposition".into(),
        "ignoring reputation".into(),
    ],
    visible_response_shape: "immersive dialogue plus visible social consequence".into(),
}
```

---

### GM-Only Fact Relevance

Do not include arbitrary GM-only facts in prompts. The model should see only secrets relevant to the current scene.

Recommended deterministic relevance rules:

```text
include if linked to active location
include if linked to active NPC/speaker
include if linked to active quest
include if linked to active clock
include if referenced by related_secret_ids
include if player input has strong keyword overlap
otherwise exclude
```

Suggested function:

```rust
fn relevant_gm_only_facts(
    input: &str,
    scene: SceneReasoningStyle,
    current_location: Option<&Location>,
    active_role: Option<&RoleActivationContext>,
    quests: &[QuestState],
    clocks: &[ClockState],
    facts: &[Fact],
) -> Vec<Fact>
```

When a GM-only fact is included, include its reveal conditions in the prompt and explicitly mark it as not player-visible unless a reveal condition has been satisfied.

---

### Context Builder

```rust
pub struct AgentContext {
    pub scenario_title: String,
    pub setting_summary: String,
    pub current_location: Option<Location>,
    pub active_role: RoleActivationContext,
    pub scene_directive: ReasoningStyleDirective,
    pub relevant_npcs: Vec<NpcContext>,
    pub relevant_factions: Vec<FactionContext>,
    pub active_quests: Vec<QuestState>,
    pub active_clocks: Vec<ClockState>,
    pub player_known_facts: Vec<Fact>,
    pub gm_only_facts: Vec<Fact>,
    pub recent_summary: Option<String>,
    pub recent_messages: Vec<MessageContext>,
    pub rules: Vec<String>,
}

pub trait ContextBuilder {
    fn build(&self, input: BuildContextInput) -> AgentContext;
}
```

Context selection rules:

- include active location,
- include active speaker,
- include NPCs in current scene,
- include factions involved in active quests/clocks,
- include player-known facts relevant to the scene,
- include only relevant GM-only facts and label them clearly,
- include GM-only facts only when they are needed for foreshadowing, consequence logic, or justified reveal checks,
- include reveal conditions for GM-only facts when available,
- include compact recent summary,
- include only the last few messages.

Do not include the whole scenario every turn.

---

### Prompt Builder

```rust
pub trait PromptBuilder {
    fn build_non_streaming_prompt(&self, context: &AgentContext, player_input: &str) -> LlmRequest;
    fn build_streaming_prompt(&self, context: &AgentContext, player_input: &str) -> LlmRequest;
    fn build_delta_extraction_prompt(
        &self,
        context: &AgentContext,
        player_input: &str,
        visible_response: &str,
    ) -> LlmRequest;
}
```

#### Prompt layers

```text
SYSTEM RULES
SCENE STYLE DIRECTIVE
ACTIVE ROLE ACTIVATION
CURRENT WORLD STATE
RELEVANT FACTS
RECENT SUMMARY
PLAYER INPUT
OUTPUT CONTRACT
```

#### Non-streaming output contract

```json
{
  "player_response": "string",
  "world_state_delta": {
    "facts_to_add": [],
    "npc_changes": [],
    "faction_changes": [],
    "quest_changes": [],
    "clock_changes": [],
    "location_change": null,
    "event_log_entries": []
  }
}
```

---

### Streaming Design

Use two calls.

#### Call 1: stream visible response

- Prompt asks only for immersive player-visible response.
- Stream tokens through SSE.
- Do not stream raw JSON.
- Strip `<think>` blocks or reject provider output that exposes them.

#### Call 2: extract delta

After streaming completes:

- send player input,
- generated visible response,
- relevant context,
- request structured `WorldStateDelta`,
- validate delta,
- apply delta,
- send final SSE event.

Hard rule:

```text
Do not mutate world state from streamed partial text.
Only mutate after a validated delta finalization step.
```

---

### Response Parsing

```rust
pub struct PlayerTurnModelOutput {
    pub player_response: String,
    pub world_state_delta: WorldStateDelta,
}

pub trait ResponseParser {
    fn parse_turn_output(&self, raw: &str) -> Result<PlayerTurnModelOutput, ParseError>;
    fn parse_delta_output(&self, raw: &str) -> Result<WorldStateDelta, ParseError>;
}
```

If JSON parsing fails:

1. retry once with a repair prompt,
2. if repair fails, return visible text but do not apply state changes,
3. persist an error event for debugging.

---

### Hidden Reasoning Stripper

```rust
pub trait HiddenReasoningStripper {
    fn strip(&self, text: &str) -> String;
}
```

Remove:

```text
<think>...</think>
Internal reasoning:
Chain of thought:
Hidden reasoning:
GM reasoning:
```

The frontend must never receive hidden reasoning.

---

### Delta Validator

```rust
pub trait DeltaValidator {
    fn validate(
        &self,
        scenario: &Scenario,
        world_state: &WorldState,
        delta: &WorldStateDelta,
    ) -> Result<ValidatedWorldStateDelta, DeltaValidationError>;
}
```

Validation rules:

- reject unknown NPC IDs,
- reject unknown faction IDs,
- reject unknown quest IDs,
- reject unknown clock IDs,
- reject unknown delta variants,
- reject clock values outside `0..=max`,
- clamp or reject faction standing outside configured range,
- reject GM-only fact becoming player-known without a satisfied reveal condition,
- reject `player_known` facts if their text reveals a GM-only fact and no reveal condition was satisfied,
- reject NPC knowledge updates that leak facts the NPC could not know,
- reject full world-state replacement,
- reject quest completion without reason,
- reject combat outcome that contradicts hard scenario rules,
- reject invalid NPC status transitions,
- require non-empty `reason` for faction, clock, quest, status, and relationship changes.

Prefer rejecting unsafe deltas over guessing.

---

### Reducer Defensive Clamping

Validation should reject invalid deltas, but the reducer must still be defensive. The reducer is the last line of state integrity.

Required behavior:

```rust
clock.current = (clock.current as i16 + delta as i16)
    .clamp(0, clock.max as i16) as u8;

faction.standing = (faction.standing + standing_delta).clamp(-100, 100);
```

Add unit tests that call the reducer directly with edge-case deltas. Do not rely only on validator tests.

---

### World-State Reducer

```rust
pub trait WorldStateReducer {
    fn apply(
        &self,
        state: WorldState,
        delta: ValidatedWorldStateDelta,
    ) -> WorldState;
}
```

Reducer rules:

- deterministic,
- no LLM calls,
- no side effects except returning new state,
- unit-tested heavily,
- preserve event history,
- increment state version exactly once per successful turn.

---

### Frontend State Projection

The authoritative `WorldState` may contain GM-only secrets, NPC-only knowledge, hidden faction goals, raw reasoning metadata, and backend-only fields. Normal frontend responses must use a projected state.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendVisibleState {
    pub state_version: i64,
    pub current_location: Option<VisibleLocation>,
    pub active_speaker: Option<VisibleNpc>,
    pub visible_npcs: Vec<VisibleNpc>,
    pub visible_quests: Vec<VisibleQuest>,
    pub visible_clocks: Vec<VisibleClock>,
    pub player_known_facts: Vec<VisibleFact>,
    pub recent_public_events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendStatePatch {
    pub state_version: i64,
    pub changed_entities: Vec<EntityRef>,
    pub visible_state: Option<FrontendVisibleState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRef {
    pub entity_type: String,
    pub id: EntityKey,
}

pub struct ViewerContext {
    pub include_debug_state: bool,
    pub is_admin: bool,
}

pub trait FrontendStateProjector {
    fn project(&self, state: &WorldState, viewer: &ViewerContext) -> FrontendVisibleState;
    fn patch_from_delta(
        &self,
        state: &WorldState,
        delta: &ValidatedWorldStateDelta,
        viewer: &ViewerContext,
    ) -> FrontendStatePatch;
}
```

Projection rules:

- never expose `gm_only` facts to normal viewers,
- never expose unrevealed secrets,
- never expose NPC-only knowledge unless it became player-known,
- never expose hidden faction goals unless discovered,
- never expose raw prompts, raw provider outputs, hidden reasoning, or internal deltas,
- return full authoritative state only through an explicit admin/debug path.

---

### Session Turn Locking

Only one turn may be processed per session at a time. Streaming makes this mandatory.

```rust
#[async_trait]
pub trait SessionTurnLock {
    async fn acquire(&self, session_id: SessionId) -> Result<TurnLockGuard, TurnLockError>;
}
```

If a second turn arrives while one is active for the same session, return `409 Conflict`.

Implementation options:

- in-memory mutex for single-instance local development,
- PostgreSQL advisory lock for server deployments,
- `sessions.processing_turn` flag with timeout recovery,
- Redis lock if Redis is already part of the deployment.

Optimistic locking on `world_states.version` is still required, but it is not enough by itself for long streaming turns.

---

### Admin and Debug Route Guard

Admin/debug routes may expose raw state, raw deltas, raw provider output, hidden reasoning, or GM-only facts. They must be disabled or protected before any non-local use.

Required behavior:

```text
/admin/* requires ADMIN_TOKEN or equivalent admin authorization.
```

Suggested config:

```text
ENABLE_ADMIN_ROUTES=false
ADMIN_TOKEN=<secret>
STORE_RAW_PROVIDER_OUTPUT=false
```

Rules:

- Do not mount admin routes when disabled.
- Do not include admin routes in public frontend route config.
- Do not expose admin routes in public OpenAPI unless debug docs are explicitly enabled.
- Keep `raw_provider_output` null by default.

---

### API Endpoints

#### Health

```http
GET /health
```

Response:

```json
{
  "status": "ok",
  "active_provider": "local-llama",
  "database": "ok"
}
```

#### Providers

```http
GET /providers
POST /providers/test
PATCH /sessions/{session_id}/provider
```

Use session-scoped provider selection. `PATCH /providers/active` is acceptable only for a single-user local deployment.

#### Scenarios

```http
POST /scenarios
GET /scenarios
GET /scenarios/{scenario_id}
PUT /scenarios/{scenario_id}
DELETE /scenarios/{scenario_id}
```

#### Sessions

```http
POST /sessions
GET /sessions
GET /sessions/{session_id}
DELETE /sessions/{session_id}
POST /sessions/{session_id}/export
```

#### Turns

```http
POST /sessions/{session_id}/turn
POST /sessions/{session_id}/turn/stream
```

Request:

```rust
#[derive(Debug, Deserialize)]
pub struct TurnRequest {
    pub input: String,
    pub mode: Option<TurnMode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnMode {
    /// Player speaks in-character.
    Dialogue,
    /// Player performs an in-world action.
    Action,
    /// Player asks the GM/system an out-of-character question.
    Direct,
    /// Player provides a memory/fact correction rather than taking an in-world action.
    Remember,
}
```

Response:

```rust
#[derive(Debug, Serialize)]
pub struct TurnResponse {
    pub message_id: uuid::Uuid,
    pub player_response: String,
    pub scene_type: SceneReasoningStyle,
    pub world_state_version: i64,
    pub changed_entities: Vec<EntityRef>,
    pub frontend_state_patch: FrontendStatePatch,
}
```

Raw `WorldStateDelta` must not be included in normal frontend responses because it may contain hidden facts, NPC-only knowledge, internal reasons, or unrevealed state changes.

Admin/debug-only variant:

```rust
#[derive(Debug, Serialize)]
pub struct DebugTurnResponse {
    pub message_id: uuid::Uuid,
    pub player_response: String,
    pub scene_type: SceneReasoningStyle,
    pub world_state_version: i64,
    pub changed_entities: Vec<EntityRef>,
    pub frontend_state_patch: FrontendStatePatch,
    pub applied_delta: WorldStateDelta,
}
```

#### World state

```http
GET /sessions/{session_id}/world-state
GET /sessions/{session_id}/events
```

`GET /world-state` returns `FrontendVisibleState` by default. Raw authoritative state must be admin/debug-only.

Avoid a generic `PATCH /sessions/{session_id}/world-state` endpoint in production. If needed, put it behind admin/debug authorization. Prefer typed mutation endpoints:

```http
POST /sessions/{session_id}/facts
POST /sessions/{session_id}/quests/{quest_id}/complete
POST /sessions/{session_id}/clocks/{clock_id}/advance
POST /sessions/{session_id}/relationships
```

---

### Axum App Shape

```rust
pub fn router(app_state: AppState) -> axum::Router {
    axum::Router::new()
        .route("/health", get(health))
        .route("/providers", get(list_providers))
        .route("/providers/test", post(test_provider))
        .route("/sessions/:session_id/provider", patch(set_session_provider))
        .route("/scenarios", post(create_scenario).get(list_scenarios))
        .route("/scenarios/:scenario_id", get(get_scenario).put(update_scenario).delete(delete_scenario))
        .route("/sessions", post(create_session).get(list_sessions))
        .route("/sessions/:session_id", get(get_session).delete(delete_session))
        .route("/sessions/:session_id/turn", post(turn))
        .route("/sessions/:session_id/turn/stream", post(turn_stream))
        .route("/sessions/:session_id/world-state", get(get_world_state))
        .route("/sessions/:session_id/events", get(list_events))
        .with_state(app_state)
}
```

---

### PostgreSQL Schema

Use one schema. Do not create duplicate table definitions.

```sql
CREATE TABLE scenarios (
    id UUID PRIMARY KEY,
    title TEXT NOT NULL,
    scenario_type TEXT NOT NULL,
    definition JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE provider_configs (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    provider_type TEXT NOT NULL,
    base_url TEXT NOT NULL,
    model TEXT NOT NULL,
    api_key_secret_ref TEXT NULL,
    capabilities JSONB NOT NULL DEFAULT '{}'::jsonb,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE sessions (
    id UUID PRIMARY KEY,
    scenario_id UUID NOT NULL REFERENCES scenarios(id) ON DELETE RESTRICT,
    provider_id UUID NULL REFERENCES provider_configs(id) ON DELETE SET NULL,
    title TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    processing_turn BOOLEAN NOT NULL DEFAULT FALSE,
    processing_turn_started_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE world_states (
    session_id UUID PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    state JSONB NOT NULL,
    version BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE messages (
    id UUID PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    speaker_id TEXT NULL,
    content TEXT NOT NULL,
    scene_type TEXT NULL,
    prompt_template_version TEXT NULL,
    raw_provider_output JSONB NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_messages_session_created_at
ON messages(session_id, created_at);

CREATE TABLE world_state_deltas (
    id UUID PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    message_id UUID NULL REFERENCES messages(id) ON DELETE SET NULL,
    delta JSONB NOT NULL,
    validation_status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_deltas_session_created_at
ON world_state_deltas(session_id, created_at);

CREATE TABLE events (
    id UUID PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    description TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_events_session_created_at
ON events(session_id, created_at);
```

Use JSONB snapshots plus a delta/event log for the MVP. Do not normalize every RPG entity too early. Normalize only entities the frontend must query independently or that become performance bottlenecks.

Optional later:

```sql
CREATE TABLE memories (
    id UUID PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    embedding VECTOR(1536),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

Do not add vector memory until structured world state works.

#### Debug logging rule

`messages.raw_provider_output` must be `NULL` by default in production. Store raw provider output only when explicit local debug logging is enabled. Raw provider output may contain hidden reasoning, GM-only facts, prompt fragments, or provider metadata.

---

### Repository Interfaces

```rust
#[async_trait]
pub trait ScenarioRepository {
    async fn create(&self, scenario: Scenario) -> Result<Scenario, RepoError>;
    async fn get(&self, id: ScenarioId) -> Result<Option<Scenario>, RepoError>;
    async fn list(&self) -> Result<Vec<ScenarioSummary>, RepoError>;
    async fn update(&self, scenario: Scenario) -> Result<Scenario, RepoError>;
    async fn delete(&self, id: ScenarioId) -> Result<(), RepoError>;
}

#[async_trait]
pub trait WorldStateRepository {
    async fn get(&self, session_id: SessionId) -> Result<Option<WorldState>, RepoError>;
    async fn save(&self, state: &WorldState, expected_version: Option<i64>) -> Result<(), RepoError>;
}

#[async_trait]
pub trait MessageRepository {
    async fn append(&self, message: MessageRecord) -> Result<(), RepoError>;
    async fn recent(&self, session_id: SessionId, limit: i64) -> Result<Vec<MessageRecord>, RepoError>;
}
```

Use both session-level turn locking and optimistic locking on `world_states.version` to avoid concurrent turn corruption. Return `409 Conflict` when a turn is already being processed for the same session.

---

### Error Handling

Use explicit app errors.

```rust
#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("not found")]
    NotFound,
    #[error("validation error: {0}")]
    Validation(String),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("turn already in progress for this session")]
    TurnInProgress,
    #[error("database error")]
    Database(#[from] sqlx::Error),
}
```

Map to HTTP status codes:

```text
400 validation/parse request errors
404 not found
409 version conflict or turn already in progress
422 unsafe delta rejected
502 provider failure
500 internal failure
```

---

### Prompt Versioning and Imported Content Safety

Store `prompt_template_version` on generated messages or turn events. It is required for debugging behavior changes across prompt, model, or provider updates.

Treat imported scenario text, NPC text, and user-authored rules as content, not system authority. Scenario content must never override engine-level rules for secret protection, hidden reasoning protection, output validation, provider policy, or reducer validation.

---

### Testing Strategy

#### Unit tests

Required:

```text
scene classifier
role activator
reasoning style optimizer
prompt builder snapshots
response parser
delta validator
world-state reducer
frontend state projector
session turn lock
hidden reasoning stripper
NPC status transitions
```

#### Integration tests

Use `wiremock` for LLM provider tests.

Required cases:

1. valid non-streaming turn applies delta,
2. invalid delta is rejected,
3. provider failure returns 502,
4. malformed JSON triggers repair once,
5. streaming response does not mutate state until final delta,
6. secret leak in delta is rejected,
7. unknown entity ID in delta is rejected,
8. normal turn response does not include raw `WorldStateDelta`,
9. debug/admin turn response may include `WorldStateDelta`,
10. concurrent turn for same session returns 409,
11. invalid NPC status transition is rejected.

#### Behavioral fixtures

Add scenario-level tests.

Example:

```text
Given player has invulnerability and unlimited mana
When player floods guildhall with mana
Then player is unharmed
And guild NPCs are alarmed
And fame clock advances
And guild standing changes
And no NPC becomes instantly loyal without cause
And no GM-only fact is returned in frontend_state_patch
```

---

### Observability

Log structured events:

```text
turn_started
turn_lock_acquired
context_built
provider_called
provider_stream_started
provider_stream_finished
delta_generated
delta_validation_failed
delta_applied
frontend_state_projected
turn_finished
turn_lock_released
```

Do not log API keys. Do not log full prompts, raw provider responses, raw deltas, or GM-only context by default in production. Allow that only behind an explicit local debug flag.

---

### Configuration

Example config:

```toml
[server]
host = "0.0.0.0"
port = 8080

[database]
url = "postgres://roleplay:roleplay@localhost:5432/roleplay"

[provider.default]
name = "local-llama"
provider_type = "openai_compatible"
base_url = "http://localhost:8081/v1"
model = "local-model"
supports_streaming = true
supports_json_mode = false
max_context_tokens = 32768
request_timeout_seconds = 120
stream_idle_timeout_seconds = 30
max_retries = 1
```

---

### Implementation Order for Codex

The prototype is already scaffolded. Continue with these hardening phases instead of rebuilding from scratch.

#### Phase 1: Admin and debug safety

1. Add admin guard for `/admin/*`.
2. Make raw export/debug turn endpoints admin-only.
3. Keep `raw_provider_output` null by default.
4. Add tests for missing/invalid/valid admin token.

#### Phase 2: Provider correctness

1. Make explicit session provider resolution strict.
2. Add error when selected provider is missing, disabled, or unavailable.
3. Keep fallback to default provider only when session has no provider.
4. Add tests for provider selection and missing provider behavior.

#### Phase 3: GM-only relevance

1. Implement relevance-based GM-only fact retrieval.
2. Include reveal conditions in prompt context.
3. Add tests proving unrelated GM-only facts do not appear in prompts.

#### Phase 4: State integrity

1. Add reducer-side clamping for clocks and faction standing.
2. Add direct reducer unit tests for edge cases.
3. Add Postgres concurrent-turn integration tests.
4. Verify lock release after provider, parse, validation, and streaming failures.

#### Phase 5: LLM robustness tests

1. Add JSON repair success/failure tests.
2. Add provider retry tests for timeout, transport, 5xx, and optionally 429.
3. Verify malformed model output does not trigger provider retry.

#### Phase 6: Roleplay quality tests

1. Add prompt snapshots for dialogue, politics, combat, mystery, and rules adjudication.
2. Add fixtures for role drift, secret leakage, NPC knowledge boundary, overpowered-player consequences, and missing NPC visibility.

---

### MVP Definition

The MVP is complete when:

1. a scenario can be imported,
2. a session can be started,
3. a frontend can send a player action,
4. the engine acquires a session turn lock,
5. the engine builds context,
6. the engine calls an LLM provider,
7. the frontend receives an immersive response,
8. the engine validates and applies a delta,
9. authoritative world state persists,
10. frontend-visible projected state is returned without GM-only data,
11. a second turn uses the updated state,
12. streaming works without mutating state prematurely.

Do not add advanced features until this works.

---

### What To Avoid

Avoid these in the first implementation:

- full autonomous multi-agent simulation,
- fine-tuning pipelines,
- benchmark reproduction,
- complex rule engines,
- vector memory before structured memory,
- too many endpoints,
- generic long reasoning prompts,
- letting the LLM overwrite state,
- returning raw authoritative world state to normal frontends,
- returning raw deltas in normal turn responses,
- mixing frontend logic into the engine,
- provider-specific hacks inside domain or engine crates.

---

### Codex Task Brief

Build a Rust backend roleplaying engine using Axum, Tokio, SQLx, PostgreSQL, Serde, Reqwest, and Tracing.

The engine must expose endpoints for scenarios, sessions, turns, streaming turns, projected world state, providers, events, and health.

The central feature is the turn pipeline:

```text
player input
-> acquire session turn lock
-> load scenario/session/world state
-> classify scene
-> activate role identity
-> build context
-> build prompt
-> call LLM provider
-> parse response
-> strip hidden reasoning
-> validate proposed delta
-> apply delta with reducer
-> project frontend-visible state
-> persist state/messages/events
-> return frontend response
-> release session turn lock
```

The LLM must never directly replace the full world state. It can only propose typed deltas. The engine validates and applies them. Normal frontend responses must not include raw authoritative world state or raw deltas. Use projected frontend state instead.

Implement the MVP first. Keep the code modular, testable, and provider-agnostic.

---

# Part III — Current Fix Plan and Phased Execution

## Advanced Roleplaying Framework Prototype — Current Fix Plan

### Purpose

This document is a focused, phase-by-phase task plan for coding agents working on:

```text
https://github.com/hofdo/advanced-roleplaying-framework-prototype
```

It reflects the current repository state after the latest review. The prototype already has a strong roleplaying-engine architecture. The remaining work is hardening, not feature expansion.

---

### Current Status

The repository now appears to implement the important runtime architecture:

```text
Axum API
+ domain crate
+ engine pipeline
+ provider registry
+ PostgreSQL persistence
+ typed world-state deltas
+ frontend projection
+ streaming finalization
+ role identity activation
+ scene reasoning style optimization
```

#### Implemented or mostly implemented

- Rust workspace split into `api`, `domain`, `engine`, `providers`, `persistence`, and `shared`.
- Domain model for scenarios, NPCs, factions, facts, clocks, quests, world state, frontend projection, and typed deltas.
- `NpcStatus` and NPC state handling.
- Non-streaming turn pipeline.
- Streaming turn route with visible streaming and second-call delta finalization.
- Session-scoped provider route and provider registry/persistence shape.
- Normal turn responses return projected frontend state, not raw world state or raw deltas.
- Normal export appears to use projected frontend state.
- JSON repair is implemented for delta parsing.
- Provider retry policy exists for transport/timeout/provider failures.
- `FactToAdd` supports secret-related fields.
- `NpcChange::KnowledgeAdded` appears to write into NPC knowledge rather than notes.
- Projection supports visible missing NPCs through explicit visibility.

#### Remaining risks

```text
P1: /admin/* routes need an admin guard before any non-local use.
P1: GM-only fact retrieval is still too broad; context should include only relevant secrets.
P2: Postgres turn locking must be proven with integration tests.
P2: Reducer should clamp clocks/faction standing defensively even after validation.
P2: Provider fallback should be strict when a session explicitly references a missing provider.
P2: Debug/raw provider output policy should be enforced and tested.
P3: Add roleplay-quality prompt/evaluator tests.
```

---

### Phase 1 — Safety hardening

Goal: prevent accidental leaks and make admin/debug behavior explicit.

#### 1.1 Add admin auth guard for `/admin/*`

Problem:

Raw admin routes expose sensitive state and/or raw model output. They are acceptable for local development only if protected or explicitly disabled.

Required behavior:

```text
/admin/* requires ADMIN_TOKEN or equivalent admin guard.
```

Minimum implementation:

```http
Authorization: Bearer <admin-token>
```

Environment/config:

```text
ADMIN_TOKEN=local-dev-token
ENABLE_ADMIN_ROUTES=true|false
```

Rules:

- If `ENABLE_ADMIN_ROUTES=false`, do not mount admin routes.
- If admin routes are enabled, require token.
- Never expose admin routes to normal frontend clients.
- Do not include admin routes in public OpenAPI unless debug docs are enabled.

Acceptance criteria:

- `/admin/sessions/{id}/export/raw` without token returns `401` or `403`.
- Same route with valid token succeeds.
- Normal frontend export remains projected/player-visible.
- Tests cover missing token, invalid token, valid token.

---

#### 1.2 Enforce raw provider output policy

Problem:

`raw_provider_output` is useful for debugging, but it may contain hidden reasoning, GM-only facts, prompt fragments, or provider metadata.

Required behavior:

```text
raw_provider_output must remain NULL unless explicit debug mode is enabled and admin access is active.
```

Suggested config:

```text
STORE_RAW_PROVIDER_OUTPUT=false
```

Rules:

- Default: never store raw provider output.
- Debug mode: store only if admin/debug flag is enabled.
- Production: keep disabled.
- Never return raw provider output through normal frontend routes.

Acceptance criteria:

- Normal turn persists `raw_provider_output = NULL`.
- Debug mode can persist it only through explicit config.
- Tests assert default behavior keeps it null.

---

### Phase 2 — Provider correctness

Goal: ensure provider selection behaves predictably and failures are visible.

#### 2.1 Make session provider fallback strict

Problem:

If a session has an explicit provider ID and that provider cannot be resolved, silently falling back to the default provider hides misconfiguration.

Required behavior:

```text
session.provider_id == None       -> use default provider
session.provider_id == Some(id)   -> use exactly that provider or return error
```

Do not fallback if a selected provider is missing, disabled, or unhealthy.

Recommended error:

```text
422 provider_not_available
```

Acceptance criteria:

- Session with no provider uses default provider.
- Session with valid provider uses that provider.
- Session with missing provider returns error.
- Deleted provider assigned to a session causes an explicit error, not fallback.

---

#### 2.2 Provider readiness and health distinction

Problem:

Configured provider does not necessarily mean reachable provider.

Required behavior:

Separate:

```text
health: service/config state
readiness: provider reachable and usable
```

Acceptance criteria:

- `/providers/health` can report configured provider.
- `/providers/readiness` actually tests provider reachability.
- Provider failures are reported clearly.

---

### Phase 3 — GM-only context relevance

Goal: reduce accidental secret leakage by not showing irrelevant secrets to the model.

#### 3.1 Filter GM-only facts before prompt construction

Problem:

Including the first N GM-only facts is too broad. If the model sees unrelated secrets, it can leak or bias narration.

Required behavior:

Only include GM-only facts relevant to the current scene.

Start with deterministic relevance:

```text
include if linked to active location
include if linked to active NPC/speaker
include if linked to active quest
include if linked to active clock
include if listed in related_secret_ids
include if keyword overlap with player input is strong
otherwise exclude
```

Suggested function:

```rust
fn relevant_gm_only_facts(
    input: &str,
    scene: SceneReasoningStyle,
    current_location: Option<&Location>,
    active_role: Option<&RoleActivationContext>,
    active_quests: &[QuestState],
    active_clocks: &[ClockState],
    facts: &[Fact],
) -> Vec<Fact>
```

Prompt requirement:

- GM-only facts must be clearly labeled.
- Reveal conditions must be included when available.
- Prompt must say not to reveal unless a reveal condition is satisfied.

Acceptance criteria:

- Irrelevant GM-only facts are not included in prompt context.
- Relevant GM-only facts include reveal conditions.
- Tests cover unrelated secret not appearing in prompt.

---

### Phase 4 — State integrity hardening

Goal: make reducers safe even if validation is bypassed or later changed.

#### 4.1 Defensive clamping in reducer

Problem:

Validator should reject invalid clock/faction deltas, but the reducer should still be the last line of defense.

Required reducer behavior:

```rust
clock.current = (clock.current as i16 + delta as i16)
    .clamp(0, clock.max as i16) as u8;

faction.standing = (faction.standing + standing_delta).clamp(-100, 100);
```

Acceptance criteria:

- Reducer cannot produce clock below 0 or above max.
- Reducer cannot produce faction standing outside `-100..=100`.
- Unit tests call reducer directly with edge-case deltas.

---

#### 4.2 Postgres turn-lock integration tests

Problem:

The schema and app wiring support Postgres locking, but concurrent behavior must be proven.

Required tests:

```text
given two concurrent turns for the same Postgres session
when both start simultaneously
then one succeeds and one receives 409 Conflict or equivalent turn-lock error
```

Also test lock release on:

- provider error,
- parse error,
- validation error,
- streaming cancellation if supported.

Acceptance criteria:

- Concurrent turns cannot corrupt world state.
- Lock is released after success.
- Lock is released after failure.
- Test uses real Postgres or testcontainers.

---

### Phase 5 — LLM robustness and regression tests

Goal: make local/hosted model failures manageable.

#### 5.1 JSON repair test coverage

The implementation appears to support one repair attempt for malformed structured output. Add regression tests.

Required tests:

- malformed delta JSON -> repair succeeds -> delta applies,
- malformed delta JSON -> repair fails -> no delta applies,
- visible response may be saved only if the system explicitly supports that partial-success mode.

Acceptance criteria:

- No invalid delta mutates world state.
- Repair is attempted once, not in an infinite loop.

---

#### 5.2 Provider retry test coverage

Provider retry exists conceptually. Test it.

Retry only:

- timeout,
- transport error,
- HTTP 5xx,
- optionally 429.

Do not retry:

- malformed JSON,
- schema validation failure,
- unsafe delta.

Acceptance criteria:

- Mock provider returns `500` once, then success -> request succeeds.
- Mock provider returns malformed JSON -> no provider retry, use repair path instead.

---

### Phase 6 — Roleplay quality tests

Goal: ensure architecture produces better roleplay, not just valid HTTP responses.

Add fixtures for:

1. Role identity consistency.
2. Scene style alignment.
3. Secret leakage prevention.
4. NPC knowledge boundary.
5. Overpowered player consequences.
6. Missing NPC visibility.
7. Political/faction behavior.

Example fixture:

```text
Given the player has invulnerability and unlimited mana
When they flood the guildhall with mana
Then the player is not harmed
And NPCs are alarmed rather than instantly loyal
And fame/suspicion clock advances
And faction standing changes
And no GM-only secret is revealed
```

Acceptance criteria:

- Tests are executable without a real LLM by using mock provider responses.
- Prompt snapshot tests exist for at least dialogue, politics, combat, mystery, and rules adjudication.

---

### Phase 7 — Production readiness later

Do this only after the core prototype is stable:

- auth/session ownership for normal users,
- OpenAPI polishing,
- provider config CRUD UI support,
- structured audit logs,
- deployment config,
- backup/export format,
- optional vector memory,
- optional roleplay evaluator service.

Do not build these before Phase 1–6 are complete.

---

### Current priority order

```text
1. Admin guard for /admin/*
2. Strict provider resolution
3. GM-only relevance filtering
4. Reducer clamping
5. Postgres concurrent-turn integration tests
6. JSON repair tests
7. Provider retry tests
8. Roleplay-quality fixture tests
```

---

### Coding Agent Brief

Implement the remaining hardening tasks for the Rust roleplaying engine.

Do not add new gameplay features yet. Focus on safety, correctness, and guide alignment:

```text
admin route protection
strict provider resolution
relevant GM-only context retrieval
reducer-side clamping
Postgres turn-lock tests
JSON repair tests
provider retry tests
roleplay-quality regression fixtures
```

The engine must continue to follow the core architecture:

```text
player input
-> role-aware context
-> LLM provider
-> visible response
-> proposed typed delta
-> validation
-> deterministic reducer
-> projected frontend state
```

Never expose raw world state, raw deltas, raw provider output, hidden reasoning, or GM-only facts through normal frontend routes.


---

# Appendix — Source Files Consolidated

This canonical file was created from these current files:

```text
advanced-roleplaying-engine-guide-as-is.md
advanced-roleplaying-engine-rust-implementation.md
roleplaying-engine-current-fix-plan.md
roleplaying-engine-code-review-action-plan.md
```

`roleplaying-engine-code-review-action-plan.md` and `roleplaying-engine-current-fix-plan.md` had identical content at consolidation time, so the fix plan appears once in Part III.
