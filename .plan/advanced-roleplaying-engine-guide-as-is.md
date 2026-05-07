# Advanced Roleplaying Engine — Pragmatic Architecture Guide

## Purpose

Build a backend engine that sits between one or more LLM providers and one or more frontends.

The engine must:

1. receive player actions from a frontend,
2. assemble the right roleplaying context,
3. call an LLM provider,
4. return storyteller-friendly output to the frontend,
5. update persistent world state safely,
6. preserve NPC/faction/world consistency over long sessions.

The engine is not a frontend. It is not a game renderer. It is not a full D&D rules simulator unless you explicitly add that later.

The goal is a reliable roleplaying orchestration backend.

---

## Core Design Principles

### 1. The LLM proposes; the engine disposes

Never let the LLM directly replace the full world state.

The LLM may propose:

- visible narration,
- dialogue,
- action consequences,
- world-state deltas,
- new facts,
- clock changes,
- quest updates.

The engine must validate and apply those changes deterministically.

Bad:

```text
world_state = llm_output.world_state
```

Good:

```text
validated_delta = validate(llm_output.delta)
world_state = reduce(world_state, validated_delta)
```

### 2. Roleplay quality comes from structured context, not longer prompts

The important thesis insight is practical:

- models drift when they stop reasoning as the active role,
- models become generic when reasoning style does not match the scene,
- longer unguided thinking can make roleplay worse.

Use short, targeted, role-aware context. Do not dump the whole campaign into every prompt.

### 3. Separate player-visible text from engine state updates

The frontend needs immersive output.

The engine needs structured updates.

Use either:

1. a single validated JSON response for non-streaming requests, or
2. two calls for streaming:
   - call 1: stream player-visible narration,
   - call 2: generate and validate world-state delta.

For an MVP, the two-call streaming approach is easier and safer.

### 4. World state is the memory layer

Do not depend on raw chat history as the only memory.

Persist structured state:

- current location,
- active NPCs,
- faction standings,
- quests,
- clocks,
- discovered facts,
- hidden facts,
- relationships,
- recent summary,
- important events.

Chat history is supporting context, not the source of truth.

### 5. Hidden information must be explicitly protected

The engine can know secrets. The player should only learn them through justified discovery.

Store facts with visibility:

```text
player_known
npc_known
gm_only
faction_known
```

Do not place all secrets into a generic visible prompt without labels. Only retrieve GM-only facts when they are relevant to the current scene, and attach reveal conditions where possible.

```json
{
  "id": "void-mark-source-unknown",
  "visibility": "gm_only",
  "text": "The player's soul-mark was not created by the goddess.",
  "reveal_conditions": [
    "the player directly questions Seraphyne about the soul-mark",
    "a divine relic reacts to the mark",
    "the player researches previous world-crossers"
  ]
}
```

The model may use GM-only facts to shape foreshadowing and consequences, but it must not reveal them unless a reveal condition is satisfied.

### 6. The frontend receives projected state, not raw GM state

Never return the complete authoritative world state to normal frontends if it contains GM-only facts, hidden NPC knowledge, unrevealed secrets, or backend metadata.

Add a projection layer:

```text
FrontendStateProjector:
  WorldState + ViewerContext -> FrontendVisibleState
```

The frontend-visible state should contain only what the player is allowed to know:

- visible current location,
- visible NPCs,
- visible quests,
- visible clocks,
- player-known facts,
- recent public events,
- state version,
- changed entity IDs.

This prevents accidental secret leaks and keeps turn responses small.

---

## Minimal Engine Responsibilities

The engine should provide these capabilities:

1. **Scenario management**
   - create/import scenario,
   - validate scenario schema,
   - list available scenarios.

2. **Session management**
   - start session from scenario,
   - load session,
   - save session state,
   - export session.

3. **Turn handling**
   - receive player action,
   - build context,
   - call LLM,
   - return player-visible response,
   - update world state.

4. **Streaming**
   - stream visible response to frontend,
   - finalize state update after stream completes.

5. **World-state updates**
   - validate deltas,
   - apply deltas,
   - persist events.

6. **Frontend state projection**
   - convert authoritative world state into player-visible state,
   - never expose GM-only data by default,
   - return compact patches instead of full state where possible.

7. **LLM provider abstraction**
   - support local and remote models,
   - support OpenAI-compatible APIs,
   - support provider capabilities.

8. **Consistency checks**
   - basic validation against role identity,
   - no hidden-secret leaks,
   - no illegal state transitions.

Everything else is optional.

---

## Data Model

### Scenario

A scenario is the static campaign seed.

```json
{
  "id": "isekai-aurethia",
  "title": "Chosen Beyond the Goddess",
  "scenario_type": "adventure",
  "setting": "A high fantasy isekai world of sword and magic.",
  "tone": "heroic, consequence-driven, high fantasy",
  "rules": [],
  "locations": [],
  "factions": [],
  "npcs": [],
  "quests": [],
  "secrets": [],
  "clocks": []
}
```

### NPC

An NPC needs more than a description. It needs behavior-driving identity.

```json
{
  "id": "seraphyne",
  "name": "Seraphyne",
  "description": "Goddess who guides summoned souls.",
  "role_identity": {
    "core_emotion": "protective but worried",
    "motivation": "guide the player without letting unknown forces exploit them",
    "worldview": "power requires responsibility",
    "fear": "the player may become a calamity",
    "speech_style": "warm, solemn, restrained",
    "boundaries": [
      "cannot remove powers she did not grant",
      "does not know the full truth about the force beyond the gods",
      "will not encourage reckless use of power"
    ]
  }
}
```

### Faction

A faction needs incentives and behavior patterns.

```json
{
  "id": "adventurer-guild",
  "name": "Continental Adventurer Guild",
  "description": "Ranks adventurers and controls quest access.",
  "faction_identity": {
    "public_goal": "assign quests and protect settlements",
    "hidden_goal": "monitor calamity-level individuals",
    "values": ["competence", "contracts", "reputation"],
    "fears": ["uncontrolled power", "political capture", "public panic"],
    "methods": ["ranking exams", "quest restrictions", "senior observers"]
  }
}
```

### World State

World state is mutable session data.

```json
{
  "session_id": "uuid",
  "scenario_id": "isekai-aurethia",
  "current_location_id": "hall-of-the-goddess",
  "current_scene": "class_selection",
  "active_speaker_id": "seraphyne",
  "facts": [],
  "npcs": {},
  "factions": {},
  "quests": {},
  "clocks": {},
  "relationships": {},
  "inventory": [],
  "summary": "",
  "recent_events": []
}
```

### Fact

```json
{
  "id": "void-mark-source-unknown",
  "text": "The player's soul-mark was not created by the goddess.",
  "visibility": "gm_only",
  "source": "scenario",
  "reveal_conditions": [
    "the player asks Seraphyne about the soul-mark",
    "a relevant divine or void artifact is inspected"
  ]
}
```

### Clock

Use clocks for threats that advance when ignored.

```json
{
  "id": "player-fame-spreads",
  "title": "The player's fame spreads",
  "current": 1,
  "max": 6,
  "consequence": "Major factions start treating the player as a strategic threat."
}
```

---

## Role-Aware Runtime Pipeline

Every player turn should follow this pipeline.

```text
1. Receive player input.
2. Acquire a session-level turn lock. Reject concurrent turns for the same session with `409 Conflict`.
3. Load session and world state.
4. Classify scene type.
5. Select active speaker or narrator mode.
6. Activate role identity for the speaker or narrator.
7. Retrieve relevant location, NPCs, factions, quests, facts, clocks, and recent summary.
8. Build prompt from compact structured context.
9. Call LLM provider.
10. Parse/validate output.
11. Strip hidden reasoning if present.
12. Validate and apply world-state delta.
13. Persist message, delta, and updated state.
14. Project authoritative world state into frontend-visible state.
15. Return visible response, applied delta, state version, and frontend state patch.
16. Release the session-level turn lock.
```

This is the practical form of role-aware reasoning.

---

## Scene Types

Use scene type to control response style and internal reasoning shape.

```text
character_dialogue
emotional_scene
political_negotiation
mystery_investigation
tactical_combat
world_simulation
rules_adjudication
travel_exploration
downtime
quest_resolution
```

### Minimal scene classification

Start rule-based. Do not overbuild.

```text
if combat is active -> tactical_combat
if input mentions attack/cast/strike/dodge -> tactical_combat
if input mentions negotiate/convince/threaten/deal -> political_negotiation
if input mentions inspect/search/investigate/clue -> mystery_investigation
if input asks about class/stats/rules/mechanics -> rules_adjudication
otherwise -> character_dialogue or travel_exploration
```

You can replace this with an LLM classifier later.

---

## Reasoning Style Directives

Each scene type needs a directive.

### Character Dialogue

Prioritize:

- NPC motivation,
- relationship memory,
- speech style,
- what the NPC knows,
- what the NPC wants right now.

Avoid:

- generic assistant advice,
- exposition dumps,
- revealing secrets too early,
- NPCs becoming submissive because the player is powerful.

### Political Negotiation

Prioritize:

- leverage,
- incentives,
- faction reputation,
- public consequences,
- hidden costs.

Avoid:

- reducing politics to one persuasion success,
- instant loyalty changes,
- NPCs ignoring their faction interests.

### Tactical Combat

Prioritize:

- clear action resolution,
- positioning,
- enemy intent,
- stakes beyond the player's HP,
- damage to allies, objectives, terrain, or time.

Avoid:

- vague cinematic fog,
- denying established invulnerability,
- making invulnerability solve every problem.

### Mystery Investigation

Prioritize:

- clue discipline,
- evidence,
- partial reveals,
- contradictions,
- player-known vs GM-only facts.

Avoid:

- revealing the full mystery too early,
- giving answers without discovery,
- contradicting established clues.

### Rules Adjudication

Prioritize:

- concise ruling,
- fairness,
- clear options,
- consequences,
- consistency with scenario rules.

Avoid:

- long lore monologues,
- hidden arbitrary restrictions,
- changing rules mid-scene without cause.

---

## Prompt Construction

The prompt should be layered and compact.

### Base system prompt

```text
You are a roleplaying engine that generates immersive storyteller output.
Stay in-world unless rules adjudication is required.
Respect world state, role identities, faction goals, clocks, and known facts.
Do not reveal hidden reasoning.
Do not reveal GM-only secrets unless the player has discovered them through justified action.
The player-visible response must be immersive and usable by a frontend storyteller UI.
```

### Role activation block

```text
ACTIVE ROLE:
Name: {name}
Description: {description}
Core emotion: {core_emotion}
Motivation: {motivation}
Worldview: {worldview}
Fear: {fear}
Speech style: {speech_style}
Boundaries: {boundaries}
Known facts: {known_facts}
Suspicions: {suspicions}

Instruction:
Respond from this role's perspective where appropriate.
Do not use knowledge this role does not have.
```

### Scene directive block

```text
SCENE TYPE: {scene_type}
Prioritize: {priorities}
Avoid: {avoid_list}
```

### World-state block

```text
CURRENT STATE:
Location: {location}
Scene: {scene}
Active quests: {quests}
Active clocks: {clocks}
Faction standings: {factions}
Player-known facts: {player_known_facts}
Relevant GM-only facts: {gm_only_facts_labeled}
Recent summary: {summary}
```

### Output instruction

For non-streaming:

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

For streaming:

- Stream only `player_response`.
- After the stream completes, make a second non-streaming LLM call to extract `world_state_delta` from the user action and generated response.

---

## World-State Delta

The LLM should propose typed deltas. Do not use arbitrary `field`/`value` patches as the default because they let the model mutate unknown parts of state.

Preferred shape:

```json
{
  "facts_to_add": [
    {
      "text": "The guild examiner suspects the player has abnormal mana.",
      "visibility": "npc_known",
      "known_by": ["guild-examiner"],
      "reason": "The examiner witnessed the mana surge."
    }
  ],
  "npc_changes": [
    {
      "type": "attitude_changed",
      "npc_id": "guildmaster-brannic",
      "attitude": "cautious interest",
      "reason": "The player revealed impossible mana during registration."
    },
    {
      "type": "knowledge_added",
      "npc_id": "guildmaster-brannic",
      "fact": "The player has abnormal mana output.",
      "reason": "Guild instruments overloaded during the test."
    }
  ],
  "faction_changes": [
    {
      "type": "standing_changed",
      "faction_id": "adventurer-guild",
      "standing_delta": -5,
      "reason": "The player caused panic in the guildhall."
    }
  ],
  "clock_changes": [
    {
      "type": "advanced",
      "clock_id": "player-fame-spreads",
      "delta": 1,
      "reason": "Multiple witnesses saw the player's mana surge."
    }
  ],
  "quest_changes": [],
  "location_change": null,
  "event_log_entries": [
    "The player revealed abnormal mana during guild registration."
  ]
}
```

Every consequential change must include a reason. The reducer must reject unknown change types.

---

## Delta Validation Rules

Reject or repair deltas when:

- entity IDs do not exist,
- clock values exceed max or drop below zero,
- faction standings exceed allowed range,
- a GM-only fact becomes player-known without a reveal event,
- an NPC gains knowledge they had no way to learn,
- a quest is completed without a reason,
- the model tries to replace the entire world state,
- the model proposes arbitrary field mutation instead of a known typed change,
- a frontend/debug patch attempts to mutate GM-only state without admin permissions,
- the delta contradicts hard scenario rules.

Validation should be deterministic and testable.

---

## Overpowered Player Handling

If the scenario gives the player extreme abilities, preserve them. Do not cheat them away casually.

Example powers:

- unlimited mana,
- physical invulnerability,
- divine blessing,
- world-crossing knowledge.

The engine should enforce consequences through things the power does not automatically protect:

- allies,
- civilians,
- time,
- reputation,
- trust,
- laws,
- faction politics,
- secrets,
- promises,
- access to information,
- world stability.

Bad:

```text
Every enemy suddenly has anti-invulnerability magic.
```

Good:

```text
Enemies avoid attacking the player directly and instead target objectives, hostages, public opinion, contracts, supply lines, or time-sensitive threats.
```

---

## API Surface

This is the pragmatic endpoint set.

### Health

```http
GET /health
```

Returns service status and active provider status.

### Provider management

```http
GET /providers
POST /providers/test
PATCH /sessions/{session_id}/provider
```

Use session-scoped provider selection by default. A global active provider is acceptable only for a single-user local app.

### Scenario management

```http
POST /scenarios
GET /scenarios
GET /scenarios/{scenario_id}
PUT /scenarios/{scenario_id}
DELETE /scenarios/{scenario_id}
```

### Session management

```http
POST /sessions
GET /sessions
GET /sessions/{session_id}
DELETE /sessions/{session_id}
POST /sessions/{session_id}/export
```

### Main turn endpoint

```http
POST /sessions/{session_id}/turn
```

Request:

```json
{
  "input": "I ask the goddess why she looks afraid.",
  "mode": "dialogue",
  "stream": false
}
```

Response:

```json
{
  "message_id": "uuid",
  "player_response": "Seraphyne's expression softens...",
  "applied_delta": {},
  "scene_type": "character_dialogue",
  "world_state_version": 12,
  "changed_entities": [
    { "type": "npc", "id": "seraphyne" }
  ],
  "frontend_state_patch": {
    "visible_npcs": [],
    "visible_quests": [],
    "visible_clocks": [],
    "player_known_facts": []
  }
}
```

Do not return the full authoritative world state from normal turn responses. Use `GET /sessions/{session_id}/world-state` to retrieve a projected frontend-visible state.

### Streaming turn endpoint

```http
POST /sessions/{session_id}/turn/stream
```

Server-sent events:

```text
event: token
data: {"text":"Seraphyne"}

event: final
data: {"message_id":"...","delta_applied":true}
```

### World state

```http
GET /sessions/{session_id}/world-state
GET /sessions/{session_id}/events
```

`GET /world-state` returns projected player-visible state by default. Add an admin/debug query parameter only if you explicitly need raw authoritative state.

Avoid a generic `PATCH /sessions/{session_id}/world-state` endpoint in production. If needed for development, mark it admin/debug-only and never expose it to normal frontends. Prefer typed mutation endpoints:

```http
POST /sessions/{session_id}/facts
POST /sessions/{session_id}/quests/{quest_id}/complete
POST /sessions/{session_id}/clocks/{clock_id}/advance
POST /sessions/{session_id}/relationships
```

### Utility endpoints

Useful but optional:

```http
POST /sessions/{session_id}/summarize
POST /sessions/{session_id}/audit-last-turn
POST /sessions/{session_id}/oracle
```

Do not build many extra endpoints before the turn pipeline is stable.

---

## Turn Modes

Define turn modes clearly so the prompt builder can behave correctly.

| Mode | Meaning | Engine behavior |
|---|---|---|
| `dialogue` | Player speaks in-character. | Prioritize active speaker identity and relationship context. |
| `action` | Player performs an in-world action. | Resolve consequences, update location/quests/clocks if needed. |
| `direct` | Player asks the GM/system an out-of-character question. | Answer clearly; do not force immersive narration. |
| `remember` | Player provides a memory/fact correction. | Propose memory/world-state delta; avoid unnecessary narration. |

---

## Frontend State Projection

The authoritative world state can contain secrets and backend-only details. Always project it before sending it to normal frontends.

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

The projector must filter out:

- GM-only facts,
- unrevealed secrets,
- NPC-only knowledge,
- hidden faction goals,
- provider/debug metadata,
- raw prompts and model outputs.

---

## Provider Abstraction

The engine should talk to any LLM through a provider interface.

```text
Provider.generate(request) -> response
Provider.stream(request) -> token stream
Provider.health() -> status
Provider.capabilities() -> capabilities
```

Provider capabilities:

```json
{
  "supports_streaming": true,
  "supports_json_mode": false,
  "supports_tool_calls": false,
  "supports_seed": false,
  "max_context_tokens": 32768,
  "request_timeout_seconds": 120,
  "stream_idle_timeout_seconds": 30,
  "max_retries": 1
}
```

Retry only transport errors, timeouts, and provider `5xx` responses. Do not blindly retry malformed model output as if it were a network failure; use a controlled repair prompt at most once.

Start with OpenAI-compatible HTTP APIs. This covers many local and hosted models.

---

## Persistence

Use a real database, not only files, once sessions matter.

Minimum persisted entities:

- scenarios,
- sessions,
- world states,
- messages,
- deltas/events,
- provider configs.

Optional later:

- vector memories,
- evaluation runs,
- audit results.

---

## Testing Requirements

Test the engine with behavior fixtures, not only unit tests.

### Required tests

1. **Role consistency**
   - NPC does not break motivation or speech style.

2. **Secret protection**
   - GM-only facts are not revealed without discovery.

3. **Delta validation**
   - illegal deltas are rejected.

4. **Clock advancement**
   - ignored threats advance.

5. **Overpowered player handling**
   - powers are respected, but external stakes remain.

6. **Streaming finalization**
   - visible response streams first, delta applies only after finalization.

7. **Provider abstraction**
   - mock provider can replace real LLM in tests.

### Example acceptance test

Input:

```text
I flood the guildhall with infinite mana to prove I am powerful.
```

Expected:

```text
- The player is not physically harmed.
- NPCs are alarmed rather than instantly loyal.
- The guild standing changes.
- A fame or suspicion clock advances.
- The event is persisted.
```

---

## Implementation Order

Build in this order.

### Phase 1: Core domain

- Define scenario schema.
- Define world-state schema.
- Define role identity and faction identity.
- Define delta schema.
- Define provider interface.

### Phase 2: Turn pipeline

- Implement scene classifier.
- Implement context builder.
- Implement prompt builder.
- Implement LLM provider adapter.
- Implement response parser.
- Implement delta validator.
- Implement world-state reducer.

### Phase 3: API

- Add health endpoint.
- Add provider endpoints.
- Add scenario endpoints.
- Add session endpoints.
- Add non-streaming turn endpoint.
- Add streaming turn endpoint.

### Phase 4: Persistence

- Persist scenarios.
- Persist sessions.
- Persist world state.
- Persist messages.
- Persist deltas/events.

### Phase 5: Reliability

- Add tests.
- Add tracing/logging.
- Add prompt snapshots.
- Add retry/repair for invalid JSON.
- Add audit endpoint for high-impact turns.

### Phase 6: Optional improvements

- Vector memory.
- Better scene classifier.
- More detailed rules engine.
- Advanced evaluation harness.
- Frontend-specific adapters.

---

## Technology Guidance

### Best pragmatic default

Use **Python + FastAPI** if you want fastest iteration and easiest LLM integration.

Good fit when:

- experimenting with prompts,
- changing schemas frequently,
- using Python LLM libraries,
- building quickly,
- validating product direction.

Suggested stack:

```text
FastAPI
Pydantic
SQLAlchemy or SQLModel
PostgreSQL
Redis optional
httpx
sse-starlette
pytest
```

### Best systems implementation

Use **Rust + Axum** if you want strong correctness, performance, type safety, and a robust service boundary.

Good fit when:

- the API contract is clear,
- you want strict state validation,
- you want predictable performance,
- you want fewer runtime surprises,
- you are comfortable with Rust complexity.

Suggested stack:

```text
Axum
Tokio
Serde
SQLx
PostgreSQL
reqwest
utoipa
tracing
tower-http
```

### Practical recommendation

For a serious but still evolving roleplaying engine:

```text
Prototype: Python + FastAPI
Production-grade rewrite or strict service: Rust + Axum
```

If the coding agent is expected to generate a clean, long-lived backend and you are comfortable maintaining Rust, use the Rust guide.

---

## What Not To Build Yet

Do not start with:

- multi-agent autonomous simulation loops,
- fine-tuning pipelines,
- complex benchmark reproduction,
- full D&D mechanical automation,
- dozens of endpoints,
- realtime multiplayer,
- custom scripting language,
- complex vector memory before structured world state works.

These can come later. They will obscure the core engine if added too early.

---

## Coding Agent Brief

Build a backend roleplaying engine that exposes HTTP endpoints for frontends and communicates with LLM providers behind a provider abstraction.

The core feature is a validated turn pipeline:

```text
player input -> context builder -> role-aware prompt -> LLM -> visible response + proposed delta -> validation -> reducer -> persisted world state -> frontend response
```

The engine must preserve role identity, scene style, world-state consistency, secrets, faction behavior, clocks, and consequences. It must never allow the LLM to directly overwrite the full world state. It must support streaming visible responses while applying state changes only after validated finalization.

Keep the implementation focused. Build the core turn pipeline first. Everything else is secondary.
