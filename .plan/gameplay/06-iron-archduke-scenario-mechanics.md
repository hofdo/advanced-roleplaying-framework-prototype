# Iron Archduke Scenario Mechanics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add scenario-specific mechanics for trust, suspicion, imperial pressure, sabotage, and romance boundaries in "The Bride of the Iron Archduke".

**Architecture:** Use generic systems from earlier gameplay plans wherever possible: social pressure, action resolution, clues, NPC agency, and player state. Keep the sample JSON valid under the domain schema; add scenario-specific rules, clocks, secrets, and tests rather than hard-coding Iron Archduke logic into the engine.

**Tech Stack:** Rust, Cargo workspace, Axum, SQLx/Postgres, Clap CLI, serde, tokio tests

---

## Current State

- `crates/cli/scenarios/samples/bride-of-the-iron-archduke.json` already includes rich setting, rules, locations, factions, NPCs, secrets, clocks, and romance boundaries.
- Domain scenario schema supports locations, factions, NPCs, quests, secrets, and clocks.
- Runtime systems already support facts, NPC state, faction standing, clocks, relationships, inventory, summary, and event logs.
- Generic plans above add social pressure, clues, NPC agency, action resolution, and player character state.

## Target Behavior

- The sample encodes starting clocks for imperial pressure, wedding approach, sabotage, and public trust where supported.
- Runtime turns can move trust/suspicion, faction pressure, and sabotage clues without bespoke engine code.
- Romance boundaries stay explicit in rules and prompt context.
- Behavioral fixture proves an early Iron Archduke scene can update trust/suspicion and pressure while preserving secrets.
- The sample remains valid and playable through `rp chat --sample bride-of-the-iron-archduke`.

## File Structure

- Modify: `crates/cli/scenarios/samples/bride-of-the-iron-archduke.json`
  - Add or refine clocks, secrets, rules, and quest objectives under existing schema.
- Modify: `crates/cli/src/samples.rs`
  - Add tests for sample-specific opening assumptions if needed.
- Modify: `crates/api/tests/behavioral_fixtures.rs`
  - Add Iron Archduke scenario fixture.
- Optional create: `crates/api/tests/fixtures/replay/iron-archduke-arrival.json`
  - If replay fixtures plan has landed.
- No engine hard-coding for this scenario.

## Dependencies

This plan is strongest after:

1. `gameplay/03-relationships-faction-pressure.md`
2. `gameplay/04-secrets-clues-discovery.md`
3. `gameplay/05-npc-agency.md`
4. `features/05-replayable-exports-fixtures.md`

It can still improve sample rules and clocks before those generic systems land, but runtime tests should wait for the generic systems.

## Tasks

### Task 1: Audit Sample Against Current Schema

**Files:**
- Read: `crates/cli/scenarios/samples/bride-of-the-iron-archduke.json`
- Read: `crates/domain/src/scenario.rs`
- Read: `crates/domain/src/validation.rs`

- [ ] **Step 1: Run sample validation**

Run: `cargo test -p cli all_builtin_samples_deserialize_and_validate bride_of_the_iron_archduke_opens_with_marta`

Expected: sample validates before changes.

- [ ] **Step 2: List current scenario mechanics**

Run: `rg -n "\"id\":|\"title\":|\"rules\":|\"secrets\":|\"clocks\":|romance|pressure|sabotage|trust|suspicion" crates/cli/scenarios/samples/bride-of-the-iron-archduke.json`

Expected: identify existing rules and mechanics encoded as schema-supported data.

- [ ] **Step 3: Decide supported edits**

Only edit keys supported by `Scenario`, `Faction`, `Npc`, `Quest`, `Secret`, and `ClockTemplate`. Do not add runtime-only state fields to sample JSON unless the domain schema has been extended.

### Task 2: Strengthen Scenario Clocks And Secrets

**Files:**
- Modify: `crates/cli/scenarios/samples/bride-of-the-iron-archduke.json`
- Modify: `crates/cli/src/samples.rs`

- [ ] **Step 1: Write failing sample-specific test**

Add tests:

```rust
#[test]
fn bride_of_the_iron_archduke_tracks_core_pressure_clocks() {
    let scenario = build_sample("bride-of-the-iron-archduke").expect("sample should build");
    let clock_ids = scenario.clocks.iter().map(|clock| clock.id.as_str()).collect::<Vec<_>>();

    assert!(clock_ids.contains(&"wedding-approaches"));
    assert!(clock_ids.contains(&"imperial-pressure"));
    assert!(clock_ids.contains(&"ashen-court-sabotage"));
}

#[test]
fn bride_of_the_iron_archduke_has_romance_boundary_rule() {
    let scenario = build_sample("bride-of-the-iron-archduke").expect("sample should build");
    assert!(scenario.rules.iter().any(|rule| rule.contains("Romance should emerge")));
}
```

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p cli bride_of_the_iron_archduke_tracks_core_pressure_clocks`

Expected: fails if clock IDs differ or are missing.

- [ ] **Step 3: Edit clocks**

Ensure sample clocks include:

```json
{
  "id": "wedding-approaches",
  "title": "The wedding approaches",
  "current": 1,
  "max": 6,
  "consequence": "The chapel ceremony arrives before Elowen and Severin have resolved trust."
}
```

```json
{
  "id": "imperial-pressure",
  "title": "Imperial pressure tightens",
  "current": 1,
  "max": 6,
  "consequence": "The emperor demands public proof that the marriage binds Falkenmark to the throne."
}
```

```json
{
  "id": "ashen-court-sabotage",
  "title": "The Ashen Court sabotage advances",
  "current": 0,
  "max": 6,
  "consequence": "A forged scandal or proxy attack threatens the betrothal."
}
```

- [ ] **Step 4: Add secrets using existing schema**

Ensure secrets cover:

```json
{
  "id": "ashen-court-forged-rumors",
  "text": "Several of the worst stories about Severin were seeded by Ashen Court agents.",
  "reveal_conditions": ["compare imperial rumor accounts with Falkenmark witnesses", "find forged correspondence"]
}
```

and:

```json
{
  "id": "severin-protects-orphan-house",
  "text": "Severin quietly funds and inspects the Winter Orphan House because border wars orphaned children under his protection.",
  "reveal_conditions": ["visit the Winter Orphan House", "speak with Sister Adela privately"]
}
```

- [ ] **Step 5: Run sample tests**

Run: `cargo test -p cli all_builtin_samples_deserialize_and_validate bride_of_the_iron_archduke`

Expected: sample-specific tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/cli/scenarios/samples/bride-of-the-iron-archduke.json crates/cli/src/samples.rs
git commit -m "feat(samples): strengthen iron archduke mechanics"
```

### Task 3: Add Behavioral Fixture For Arrival Scene

**Files:**
- Modify: `crates/api/tests/behavioral_fixtures.rs`
- Optional create: `crates/api/tests/fixtures/replay/iron-archduke-arrival.json`

- [ ] **Step 1: Write failing fixture**

Add ignored test `iron_archduke_arrival_updates_trust_without_revealing_secrets`. It should:

1. Build sample scenario `bride-of-the-iron-archduke`.
2. Create a session.
3. Submit action/dialogue input where Elowen treats Marta respectfully but remains wary of Severin.
4. Provider returns trust/suspicion/faction pressure deltas if generic systems are available, or relationship/faction note deltas if only current systems exist.
5. Assert projection excludes `ashen-court-forged-rumors` secret.

- [ ] **Step 2: Run expected failing command**

Run: `TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test behavioral_fixtures iron_archduke_arrival -- --ignored --test-threads=1`

Expected: fails until fixture and generic deltas are wired.

- [ ] **Step 3: Implement fixture response**

Use provider JSON that changes:

- Marta or House Falkenrath relationship/trust.
- Imperial pressure or Ashen Court sabotage clock.
- Public event log entry about Elowen's conduct.
- No player-known fact that duplicates GM-only secret text.

- [ ] **Step 4: Assert raw and projected state**

Admin raw export should show the runtime changes. Public export should show visible changes and omit hidden facts and hidden pressure notes.

- [ ] **Step 5: Run fixture**

Run the same ignored command.

Expected: fixture passes.

- [ ] **Step 6: Commit**

```bash
git add crates/api/tests/behavioral_fixtures.rs crates/api/tests/fixtures/replay
git commit -m "test: cover iron archduke arrival mechanics"
```

### Task 4: Verify Playability Through CLI

**Files:**
- Modify: `crates/cli/tests/cli_smoke.rs`
- Modify: `README.md` if the sample list or play instructions need refresh.

- [ ] **Step 1: Add CLI smoke test**

Add a test that starts a CLI session with:

```bash
cargo run -p cli -- chat --sample bride-of-the-iron-archduke
```

Use scripted lines if the existing chat test helper supports it. Submit `/status`, `/world`, and `/exit`. Assert the sample loads and the world contains `frostmere-citadel`.

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p cli --test cli_smoke iron_archduke_chat_sample`

Expected: fails until test helper is wired or sample assumptions are updated.

- [ ] **Step 3: Implement smoke test support**

Use existing `run_with_source` from `chat.rs` tests if possible rather than spawning an interactive terminal.

- [ ] **Step 4: Run CLI tests**

Run: `cargo test -p cli --test cli_smoke iron_archduke_chat_sample`

Run: `cargo test -p cli`

Expected: CLI tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/cli/tests/cli_smoke.rs README.md
git commit -m "test(cli): smoke iron archduke sample"
```

## Verification

Run:

```bash
cargo test -p cli all_builtin_samples_deserialize_and_validate bride_of_the_iron_archduke
cargo test -p cli --test cli_smoke iron_archduke_chat_sample
cargo test -p api --test memory_api_flows
TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test behavioral_fixtures iron_archduke_arrival -- --ignored --test-threads=1
```

## Acceptance Criteria

- Iron Archduke sample has explicit clocks for wedding pressure, imperial pressure, and sabotage.
- Sample secrets support rumor investigation and Severin reputation reveals.
- Romance boundaries remain encoded as scenario rules.
- Behavioral fixture proves trust/pressure can change without revealing GM-only secrets.
- CLI can load the sample and show initial world state.

## Risks

- Scenario JSON must not include runtime-only fields unless domain schema supports them.
- Generic systems should carry mechanics; avoid hard-coded Iron Archduke branches in engine code.
- Romance boundary text should constrain pacing without forcing affection or rejection outcomes.

