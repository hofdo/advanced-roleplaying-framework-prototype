# NPC Agency Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add NPC offscreen actions, availability, and location-aware presence so NPCs can pursue goals when they are not the active speaker.

**Architecture:** Extend `NpcState` with availability and intent fields, add typed agency changes, and update context selection to distinguish present NPCs from offscreen actors. Keep offscreen actions validated through clocks, facts, notes, and event logs.

**Tech Stack:** Rust, Cargo workspace, Axum, SQLx/Postgres, Clap CLI, serde, tokio tests

---

## Current State

- `NpcState` has status, visibility, location, attitude, known facts, and notes.
- `BasicContextBuilder` includes NPCs that are active speaker or at the current location.
- `NpcChange` supports attitude, knowledge, status, location, note, and visibility.
- NPCs can be hidden or missing, but there is no explicit availability or offscreen intent.
- `initial_world_state` respects `initial_location_id` and `initial_visible_to_player`.

## Target Behavior

- NPC state tracks availability (`present`, `nearby`, `offscreen`, `unavailable`) and current intent.
- Deltas can record offscreen actions and intent changes.
- Context builder includes present NPCs and a compact list of relevant offscreen activity.
- Projection exposes player-visible NPC availability when appropriate.
- Validator rejects offscreen actions from NPCs that cannot act, such as dead or unconscious NPCs.

## File Structure

- Modify: `crates/domain/src/state.rs`
  - Add `NpcAvailability`, fields on `NpcState`, and new `NpcChange` variants.
- Modify: `crates/persistence/src/store.rs`
  - Seed default availability and intent.
- Modify: `crates/engine/src/validation.rs`
  - Validate agency changes.
- Modify: `crates/engine/src/reducer.rs`
  - Apply availability, intent, and offscreen actions.
- Modify: `crates/engine/src/context.rs`
  - Select present NPCs and offscreen activity.
- Modify: `crates/engine/src/projection.rs`
  - Project visible availability and public offscreen events.
- Modify: `crates/engine/src/prompt.rs`
  - Render NPC agency context.

## Tasks

### Task 1: Add NPC Agency Types

**Files:**
- Modify: `crates/domain/src/state.rs`
- Modify: `crates/domain/tests/serde_roundtrip_tests.rs`

- [ ] **Step 1: Write failing serde tests**

Deserialize old `NpcState` and assert default availability is `present` when visible and active. Round-trip:

```json
{
  "type": "offscreen_action_recorded",
  "npc_id": "archduke-severin",
  "intent": "question the captured courier",
  "result": "learned the Ashen Court used forged seals",
  "visible_to_player": false,
  "reason": "Severin was acting in the war room while Elowen met Marta."
}
```

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p domain npc_agency`

Expected: fails until types exist.

- [ ] **Step 3: Add fields and variants**

Add:

```rust
pub enum NpcAvailability {
    Present,
    Nearby,
    Offscreen,
    Unavailable,
}
```

Add `availability`, `current_intent`, and `offscreen_actions` to `NpcState` with serde defaults.

Add `NpcChange` variants:

```rust
AvailabilityChanged
IntentChanged
OffscreenActionRecorded
```

- [ ] **Step 4: Run domain tests**

Run: `cargo test -p domain`

Expected: domain tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/domain/src/state.rs crates/domain/tests/serde_roundtrip_tests.rs
git commit -m "feat(domain): add npc agency state"
```

### Task 2: Validate And Reduce NPC Agency

**Files:**
- Modify: `crates/engine/src/validation.rs`
- Modify: `crates/engine/src/reducer.rs`

- [ ] **Step 1: Write validation tests**

Add tests:

```rust
rejects_offscreen_action_from_dead_npc()
rejects_intent_change_for_unknown_npc()
rejects_offscreen_action_without_result()
```

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p engine npc_agency`

Expected: fails before validation support.

- [ ] **Step 3: Implement validation**

Reuse known NPC ID checks. Require reasons and non-empty intent/result. Reject agency changes for `Dead` and `Unconscious` NPCs except availability changes to `Unavailable`.

- [ ] **Step 4: Write reducer tests**

Assert availability changes update the NPC, intent changes set or clear current intent, and offscreen actions append records without changing visibility unless requested by a separate visibility change.

- [ ] **Step 5: Implement reducer**

Add match arms for the new `NpcChange` variants.

- [ ] **Step 6: Run tests**

Run: `cargo test -p engine npc_agency`

Run: `cargo test -p engine`

Expected: all engine tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/engine/src/validation.rs crates/engine/src/reducer.rs
git commit -m "feat(engine): apply npc agency deltas"
```

### Task 3: Update Context Selection And Projection

**Files:**
- Modify: `crates/engine/src/context.rs`
- Modify: `crates/engine/src/projection.rs`
- Modify: `crates/domain/src/state.rs`
- Modify: `crates/engine/src/prompt.rs`

- [ ] **Step 1: Write context tests**

Add tests proving:

1. Present NPCs at the current location are included in `relevant_npcs`.
2. Offscreen NPCs are excluded from `relevant_npcs`.
3. Recent visible offscreen actions appear in a separate context list.

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p engine npc_agency_context`

Expected: fails until context fields exist.

- [ ] **Step 3: Add context fields**

Add `offscreen_npc_activity` to `AgentContext`, containing NPC name, intent, result, and visibility-filtered notes.

- [ ] **Step 4: Extend projection**

Add `availability` to `VisibleNpc`. Include it only for visible NPCs. Add public offscreen events only if they are visible to player.

- [ ] **Step 5: Update prompt rendering**

Render:

```text
NPC AGENCY:
Present NPCs: ...
Offscreen activity: ...
```

Keep hidden offscreen actions out of narration-safe prompt context.

- [ ] **Step 6: Run tests**

Run: `cargo test -p engine context projection prompt`

Expected: tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/domain/src/state.rs crates/engine/src/context.rs crates/engine/src/projection.rs crates/engine/src/prompt.rs
git commit -m "feat(engine): render npc agency context"
```

### Task 4: Cover API Behavior

**Files:**
- Modify: `crates/persistence/src/store.rs`
- Modify: `crates/api/tests/memory_api_flows.rs`
- Modify: `crates/api/tests/behavioral_fixtures.rs`

- [ ] **Step 1: Seed defaults**

In `initial_world_state`, derive availability from `initial_visible_to_player` and location:

```text
visible and current location -> present
visible and other location -> offscreen
hidden/missing -> offscreen or unavailable depending on status
```

- [ ] **Step 2: Add memory flow**

Submit a turn that records an invisible offscreen action for an NPC and assert normal projection does not reveal it.

- [ ] **Step 3: Add behavioral fixture**

Use Iron Archduke style scenario: while Elowen speaks with Marta, Severin performs a hidden offscreen action in the war room. Admin raw export includes it; player projection does not.

- [ ] **Step 4: Run tests**

Run: `cargo test -p api --test memory_api_flows npc_agency`

Run: `TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test behavioral_fixtures npc_agency -- --ignored --test-threads=1`

Expected: tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/persistence/src/store.rs crates/api/tests/memory_api_flows.rs crates/api/tests/behavioral_fixtures.rs
git commit -m "feat(gameplay): track npc offscreen agency"
```

## Verification

Run:

```bash
cargo test -p domain
cargo test -p engine
cargo test -p api --test memory_api_flows
TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test behavioral_fixtures npc_agency -- --ignored --test-threads=1
```

## Acceptance Criteria

- NPCs have explicit availability and current intent.
- Offscreen actions can be recorded without becoming player-visible.
- Dead or unconscious NPCs cannot perform agency actions.
- Context selection distinguishes present NPCs from offscreen activity.
- Projection respects visibility for offscreen agency.

## Risks

- Availability can conflict with location and status. Define precedence clearly: dead/unconscious status limits agency regardless of location.
- Hidden offscreen actions can leak through prompt context if visible/oracle rendering is not split.
- Too much offscreen activity can bloat prompts; bound context selection.

