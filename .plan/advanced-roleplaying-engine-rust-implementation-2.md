# Advanced Roleplaying Engine — Rust Implementation Blueprint

## Purpose

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


## Current Prototype Alignment

The current prototype already implements the intended high-level shape:

```text
api crate
+ domain crate
+ engine crate
+ providers crate
+ persistence crate
+ shared crate
```

The following pieces are already present or partially present and should be preserved:

- Axum API scaffold,
- scenario/session/world-state domain concepts,
- role identity and faction identity,
- `NpcStatus`,
- typed `WorldStateDelta`,
- frontend state projection,
- provider abstraction,
- OpenAI-compatible provider,
- PostgreSQL persistence baseline,
- non-streaming turn pipeline,
- streaming visible response with finalization,
- hidden reasoning stripping,
- prompt version metadata.

The current priority is hardening, not adding new roleplay features.

### Known prototype gaps to fix

```text
P1: session provider selection exists but must persist and be used at turn time.
P1: normal export must not expose raw authoritative world state.
P1: PostgreSQL mode needs deployment-safe turn locking, not only in-memory locking.
P1: streaming and non-streaming turn finalization should share validation/reducer/persistence logic.
P2: NpcChange::KnowledgeAdded must update known_facts, not notes.
P2: malformed JSON needs one controlled repair retry.
P2: provider max_retries must be implemented for transport/timeout/5xx only.
P2: secret leakage validation must go beyond exact string matching.
P2: NPC projection should use explicit visible_to_player instead of hiding by status alone.
P3: provider health should distinguish configured vs reachable.
P3: license metadata should be aligned.
```

These gaps are reflected in the phased implementation plan near the end of this file.

---

## Recommended Rust Stack

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

## Workspace Layout

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

### `domain`

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

### `engine`

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

### `providers`

LLM provider abstraction.

Contains:

```text
LlmProvider trait
OpenAiCompatibleProvider
MockProvider
ProviderCapabilities
```

### `persistence`

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

### `api`

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

### `shared`

Shared utilities.

Contains:

```text
ids
time helpers
config
error helpers
```

---

## Domain Types

### IDs

Use UUIDs for persisted entities. Scenario content may still contain stable string IDs for locations/NPCs/factions.

```rust
pub type SessionId = uuid::Uuid;
pub type ScenarioId = uuid::Uuid;
pub type MessageId = uuid::Uuid;

pub type EntityKey = String;
```

---

## Scenario

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

## Role Identity

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

## NPC Status

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

### Status semantics

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

### NPC runtime state

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcState {
    pub npc_id: EntityKey,
    pub status: NpcStatus,
    pub visible_to_player: bool,
    pub location_id: Option<EntityKey>,
    pub attitude_to_player: Option<String>,
    pub known_facts: Vec<EntityKey>,
    pub notes: Vec<String>,
}
```

### Status validation rules

- Reject `Dead -> Active` unless a resurrection/revival event exists.
- Reject `Dead` NPCs speaking unless the scenario supports ghosts, undeath, recordings, or resurrection.
- Reject `Unconscious` NPCs making plans or negotiations.
- Reject player-visible certainty about `Hidden` NPCs unless discovered. `Missing`, `Captured`, or `Dead` may be player-visible when `visible_to_player` is true or the player discovered that status.
- Require a reason for every status change.

---

## Faction Identity

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

## World State

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

### Facts

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

### Clocks

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

## Scene Reasoning Style

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

## World-State Delta

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
    pub related_secret_ids: Vec<EntityKey>,
    pub reveal_condition_satisfied: Option<String>,
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

## Provider Abstraction

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

## Engine Pipeline

### Turn Pipeline

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

## Scene Classifier

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

## Role Identity Activation

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

## Reasoning Style Optimization

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

## Context Builder

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

## Prompt Builder

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

### Prompt layers

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

### Non-streaming output contract

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

## Streaming Design

Use two calls.

### Call 1: stream visible response

- Prompt asks only for immersive player-visible response.
- Stream tokens through SSE.
- Do not stream raw JSON.
- Strip `<think>` blocks or reject provider output that exposes them.

### Call 2: extract delta

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

## Response Parsing

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

## Hidden Reasoning Stripper

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

## Delta Validator

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

## World-State Reducer

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

## Frontend State Projection

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

## Session Turn Locking

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

## API Endpoints

### Health

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

### Providers

```http
GET /providers
POST /providers/test
PATCH /sessions/{session_id}/provider
```

Use session-scoped provider selection. `PATCH /providers/active` is acceptable only for a single-user local deployment.

### Scenarios

```http
POST /scenarios
GET /scenarios
GET /scenarios/{scenario_id}
PUT /scenarios/{scenario_id}
DELETE /scenarios/{scenario_id}
```

### Sessions

```http
POST /sessions
GET /sessions
GET /sessions/{session_id}
DELETE /sessions/{session_id}
POST /sessions/{session_id}/export
```

### Turns

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

### World state

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

## Axum App Shape

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

## PostgreSQL Schema

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

### Debug logging rule

`messages.raw_provider_output` must be `NULL` by default in production. Store raw provider output only when explicit local debug logging is enabled. Raw provider output may contain hidden reasoning, GM-only facts, prompt fragments, or provider metadata.

---

## Repository Interfaces

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

## Error Handling

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

## Prompt Versioning and Imported Content Safety

Store `prompt_template_version` on generated messages or turn events. It is required for debugging behavior changes across prompt, model, or provider updates.

Treat imported scenario text, NPC text, and user-authored rules as content, not system authority. Scenario content must never override engine-level rules for secret protection, hidden reasoning protection, output validation, provider policy, or reducer validation.

---

## Testing Strategy

### Unit tests

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

### Integration tests

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

### Behavioral fixtures

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

## Observability

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

## Configuration

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

## Implementation Order for Codex

The prototype already has a scaffold. Continue with hardening phases rather than adding advanced features.

### Phase 1: Correctness and safety

1. Implement real session-scoped provider persistence.
2. Resolve provider at turn time with this priority:

```text
session provider -> configured default provider -> error
```

3. Make normal session export use `FrontendStateProjector`.
4. Move raw authoritative export to an admin/debug-only route.
5. Ensure raw deltas and raw world state are never returned to normal frontend routes.
6. Ensure `raw_provider_output` remains `NULL` unless explicit local debug mode is enabled.

### Phase 2: Turn locking and concurrency

1. Keep the in-memory lock for local-only mode.
2. Add PostgreSQL-backed turn locking for database mode.
3. Use either PostgreSQL advisory locks or `sessions.processing_turn` with stale-lock recovery.
4. Return `409 Conflict` when another turn is already active for the same session.
5. Keep optimistic locking on `world_states.version`.

### Phase 3: Shared turn finalization

1. Extract shared turn preparation into engine code.
2. Extract shared delta finalization into engine code.
3. Make non-streaming and streaming turns use the same validator, reducer, projector, and persistence path.
4. Keep streaming-specific code limited to SSE token emission.

### Phase 4: Reducer and projection fixes

1. Fix `NpcChange::KnowledgeAdded` so it writes to `NpcState.known_facts`.
2. Add `visible_to_player` to `NpcState` or equivalent projection metadata.
3. Do not hide `Missing` NPCs automatically; hide only when visibility says to hide.
4. Add projection tests for `Hidden`, `Missing`, `Captured`, and `Dead` NPC statuses.

### Phase 5: LLM robustness

1. Add one JSON repair retry for malformed structured output.
2. Persist an error event and skip state mutation if repair fails.
3. Implement provider retry policy using `max_retries` for transport errors, timeouts, HTTP `5xx`, and optionally HTTP `429`.
4. Do not retry unsafe deltas, parse failures after repair, or validation failures as provider errors.

### Phase 6: Secret protection

1. Add `related_secret_ids` and `reveal_condition_satisfied` to `FactToAdd` or equivalent reveal metadata.
2. Reject player-known facts linked to secrets unless a reveal condition is satisfied.
3. Add tests for paraphrased secret leaks.
4. Keep exact-string checks only as a secondary defense.

### Phase 7: API and metadata polish

1. Distinguish provider configuration health from actual provider readiness.
2. Add readiness check endpoint or health response fields.
3. Align root license and `Cargo.toml` license metadata.
4. Add admin/debug gating before exposing raw export, raw deltas, raw provider output, or raw world state.

---

## MVP Definition

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

## What To Avoid

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

## Codex Task Brief

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
