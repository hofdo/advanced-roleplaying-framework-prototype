# Advanced Roleplaying Engine — Current Concept Guide

## Purpose

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

## Current Prototype State

The current Rust prototype already implements much of the intended architecture:

```text
api crate
+ domain crate
+ engine crate
+ providers crate
+ persistence crate
+ shared crate
```

Implemented or partially implemented:

- Axum API surface,
- scenario/session/world-state concepts,
- role identity and faction identity,
- `NpcStatus`,
- typed `WorldStateDelta`,
- provider abstraction,
- OpenAI-compatible provider,
- turn pipeline,
- streaming turn route,
- frontend state projection,
- PostgreSQL schema baseline,
- prompt version metadata,
- hidden reasoning stripping,
- session turn lock abstraction.

The next work is not “add more agents.” The next work is hardening the core pipeline so the engine is safe and reliable.

---

## Non-Negotiable Rules

### 1. The LLM proposes; the engine disposes

The LLM may propose narration and deltas, but it must never directly overwrite full world state.

```text
LLM output -> parse -> validate -> reducer -> persisted state
```

### 2. The frontend receives projected state only

Normal frontend routes must receive `FrontendVisibleState` or `FrontendStatePatch`, not raw authoritative `WorldState`.

Raw state may contain:

- GM-only facts,
- hidden NPC knowledge,
- hidden faction goals,
- unrevealed clocks,
- raw deltas,
- backend/debug metadata.

### 3. No generic world-state PATCH in production

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

### 4. One active turn per session

Only one turn may process per session at a time.

Concurrent turns should return:

```http
409 Conflict
```

This is especially important for streaming turns.

### 5. Streaming must not mutate from partial text

Use this safe streaming model:

```text
Call 1: stream visible response
Call 2: extract structured delta
Validate delta
Apply reducer
Send final SSE event
```

### 6. Provider choice should be session-scoped

Avoid global active provider state except in single-user local mode.

Preferred resolution order:

```text
session provider -> default provider -> error
```

---

## Core Runtime Pipeline

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

## Role-Aware Context

The engine should not ask the model to “roleplay better” in a vague way. It should provide structured context.

### Role Identity Activation

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

### Scene Reasoning Style

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

## Domain Model Essentials

### Scenario

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

### NPC

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

### NPC Status

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

### Fact

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

### World-State Delta

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

## Frontend State Projection

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

## API Surface

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

## Current Priority Fixes

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

## What Not To Build Yet

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

## Coding Agent Brief

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
