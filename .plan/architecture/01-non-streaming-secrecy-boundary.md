# Non-Streaming Secrecy Boundary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split non-streaming visible narration from hidden-context delta extraction so player-visible generation never receives GM-only facts.

**Architecture:** Reuse the existing streaming separation in `BasicPromptBuilder`: narration-safe context first, oracle context second. Keep `DefaultTurnPipeline::finalize_with_parsed_delta` as the single validation/reduction/persistence path so streaming and non-streaming flows continue to converge after provider output.

**Tech Stack:** Rust, Cargo workspace, Axum, SQLx/Postgres, Clap CLI, serde, tokio tests

---

## Current State

- `crates/engine/src/prompt.rs` already exposes `build_streaming_prompt`, which calls `render_narration_context` and excludes GM-only facts.
- `build_delta_extraction_prompt` calls `render_context`, includes selected GM-only facts, and asks for a strict `WorldStateDelta`.
- `build_non_streaming_prompt` currently asks for both `player_response` and `world_state_delta` from a shared prompt that includes GM-only facts.
- `DefaultTurnPipeline::process_turn` parses `PlayerTurnModelOutput` and then calls `finalize_with_parsed_delta`.
- `DefaultTurnPipeline::finalize_turn_delta` can parse a raw delta string, repair malformed delta JSON once, and call `finalize_with_parsed_delta`.
- `crates/engine/tests/streaming_pipeline.rs` proves streaming token order, hidden-reasoning stripping, event persistence, version increments, and delta extraction errors.
- `crates/api/tests/memory_api_flows.rs` and `crates/api/tests/postgres_api_flows.rs` cover non-streaming turn behavior, visible projections, and raw export boundaries.

## Target Behavior

- The non-streaming player-visible provider call receives the same narration-safe context as streaming.
- A second non-streaming provider call extracts `WorldStateDelta` using the full oracle context plus the already generated visible response.
- Normal turn responses preserve the existing JSON shape returned by API and CLI.
- The provider still gets raw GM-only facts only in the delta-extraction request.
- If delta extraction fails, the world state is not mutated and a turn error event is persisted.
- Existing streaming behavior remains unchanged.

## File Structure

- Modify: `crates/engine/src/prompt.rs`
  - Add a non-streaming narration-only prompt method or reuse `build_streaming_prompt` through a clearer name.
  - Keep `build_delta_extraction_prompt` as the oracle-context request.
- Modify: `crates/engine/src/pipeline.rs`
  - Change `process_turn` and `process_turn_debug` to call provider twice: visible response, then delta extraction.
  - Keep `finalize_with_parsed_delta` as the shared finalizer.
  - Preserve raw provider output policy for the assistant message.
- Modify: `crates/engine/tests/streaming_pipeline.rs`
  - Add provider-request inspection helpers if the existing mock provider cannot expose prompt content.
- Modify: `crates/api/tests/memory_api_flows.rs`
  - Add a non-streaming memory flow that proves the first provider request does not contain a GM-only fact.
- Modify: `crates/api/tests/postgres_api_flows.rs`
  - Mirror the memory flow under the ignored Postgres suite to protect durable execution.

## Tasks

### Task 1: Lock Prompt-Level Boundary

**Files:**
- Modify: `crates/engine/src/prompt.rs`

- [ ] **Step 1: Write failing prompt tests**

Add tests in `crates/engine/src/prompt.rs` under the existing test module or create one if absent:

```rust
#[test]
fn non_streaming_visible_prompt_excludes_gm_only_facts() {
    let context = context_with_secret("The chancellor poisoned the treaty.");
    let request = BasicPromptBuilder.build_visible_response_prompt(
        &context,
        "I greet the chancellor.",
    );
    let combined = request
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(!combined.contains("The chancellor poisoned the treaty."));
    assert!(combined.contains("Player-known facts"));
    assert!(!request.json_mode);
}

#[test]
fn delta_extraction_prompt_includes_gm_only_facts() {
    let context = context_with_secret("The chancellor poisoned the treaty.");
    let request = BasicPromptBuilder.build_delta_extraction_prompt(
        &context,
        "I inspect the treaty.",
        "The seal smells faintly bitter.",
    );
    let combined = request
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(combined.contains("The chancellor poisoned the treaty."));
    assert!(request.json_mode);
}
```

Use a helper `context_with_secret(secret_text: &str) -> AgentContext` copied from the smallest existing context fixture style in `context.rs` tests.

- [ ] **Step 2: Run the expected failing command**

Run: `cargo test -p engine prompt::tests::non_streaming_visible_prompt_excludes_gm_only_facts prompt::tests::delta_extraction_prompt_includes_gm_only_facts`

Expected: the first test fails because `build_visible_response_prompt` is not defined, or because the existing non-streaming prompt includes the GM-only fact.

- [ ] **Step 3: Add the prompt API**

Change `PromptBuilder` to include:

```rust
fn build_visible_response_prompt(&self, context: &AgentContext, player_input: &str) -> LlmRequest;
```

Implement it in `BasicPromptBuilder` using `render_narration_context`, `json_mode: false`, and a system message that asks for player-visible narration only. Then change `build_streaming_prompt` to call the same rendering logic so the two visible paths share the same boundary.

- [ ] **Step 4: Run prompt tests**

Run: `cargo test -p engine prompt::tests::non_streaming_visible_prompt_excludes_gm_only_facts prompt::tests::delta_extraction_prompt_includes_gm_only_facts`

Expected: both tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/prompt.rs
git commit -m "test: lock prompt secrecy boundary"
```

### Task 2: Split Non-Streaming Pipeline Calls

**Files:**
- Modify: `crates/engine/src/pipeline.rs`
- Modify: `crates/engine/tests/streaming_pipeline.rs` or add `crates/engine/tests/non_streaming_pipeline.rs`

- [ ] **Step 1: Write failing pipeline test**

Add a test with a recording provider that captures each `LlmRequest`. Seed it with two responses:

```json
"The examiner watches you without lowering her hand from the alarm bell."
```

and:

```json
{
  "facts_to_add": [],
  "npc_changes": [],
  "faction_changes": [],
  "quest_changes": [],
  "clock_changes": [],
  "relationship_changes": [],
  "inventory_changes": [],
  "location_change": null,
  "summary_update": null,
  "event_log_entries": []
}
```

Assert:

```rust
assert_eq!(requests.len(), 2);
assert!(!joined_request_text(&requests[0]).contains("The soul-mark was not created by the goddess."));
assert!(joined_request_text(&requests[1]).contains("The soul-mark was not created by the goddess."));
```

- [ ] **Step 2: Run the expected failing command**

Run: `cargo test -p engine non_streaming_pipeline -- --nocapture`

Expected: failure showing only one provider request or the first request containing the secret.

- [ ] **Step 3: Implement the two-call process**

In `DefaultTurnPipeline::process_turn`:

1. Build the visible prompt with `build_visible_response_prompt`.
2. Call `provider.generate`.
3. Strip hidden reasoning from the returned text.
4. Build `build_delta_extraction_prompt` with the same `PreparedTurn`, player input, and stripped visible response.
5. Call `provider.generate` again.
6. Call `finalize_turn_delta` with the raw delta text.

Keep event recording order compatible with existing consumers: `ProviderCalled`, `ProviderResponded`, `DeltaApplied`, `FrontendStateProjected`, `TurnFinished`. If more detail is useful, add new event descriptions while keeping current event types present.

- [ ] **Step 4: Preserve debug turn behavior**

Apply the same two-call flow to `process_turn_debug`. Return `applied_delta` from the parsed delta passed into finalization, not from a combined turn-output object.

- [ ] **Step 5: Run pipeline tests**

Run: `cargo test -p engine`

Expected: all engine unit and integration tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/engine/src/pipeline.rs crates/engine/tests
git commit -m "feat: split non-streaming turn generation"
```

### Task 3: Cover API Memory Flow

**Files:**
- Modify: `crates/api/tests/memory_api_flows.rs`
- Modify: `crates/api/tests/common/mod.rs` if a recording provider helper belongs with existing test helpers.

- [ ] **Step 1: Write failing API test**

Add a test named `non_streaming_turn_visible_prompt_does_not_receive_gm_only_fact`. Use a recording provider seeded with visible narration and a valid empty delta JSON. Create a scenario using `sample_scenario()`, create a session, submit `/turn`, and inspect the first captured request.

Assert:

```rust
assert_eq!(status, StatusCode::OK);
assert_eq!(recorded_requests.len(), 2);
assert!(!joined_request_text(&recorded_requests[0]).contains("soul-mark was not created"));
assert!(joined_request_text(&recorded_requests[1]).contains("soul-mark was not created"));
```

- [ ] **Step 2: Run the expected failing command**

Run: `cargo test -p api --test memory_api_flows non_streaming_turn_visible_prompt_does_not_receive_gm_only_fact -- --nocapture`

Expected: failure before the pipeline split, pass after Task 2.

- [ ] **Step 3: Keep normal response contract stable**

Assert the response still contains:

```rust
assert!(payload.get("message_id").is_some());
assert_eq!(payload["world_state_version"], 1);
assert!(payload.get("frontend_state_patch").is_some());
assert!(payload.get("raw_provider_output").is_none());
```

- [ ] **Step 4: Run memory API suite**

Run: `cargo test -p api --test memory_api_flows`

Expected: all memory API flow tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/api/tests/memory_api_flows.rs crates/api/tests/common/mod.rs
git commit -m "test: cover non-streaming secrecy boundary"
```

### Task 4: Cover Postgres Flow

**Files:**
- Modify: `crates/api/tests/postgres_api_flows.rs`

- [ ] **Step 1: Add ignored Postgres boundary test**

Add `#[ignore = "requires Docker-backed Postgres integration"]` test `postgres_non_streaming_visible_prompt_does_not_receive_gm_only_fact`. It should mirror the memory test and use the Postgres test context.

- [ ] **Step 2: Run the ignored test**

Run: `docker compose up -d postgres`

Run: `TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test postgres_api_flows postgres_non_streaming_visible_prompt_does_not_receive_gm_only_fact -- --ignored --test-threads=1`

Expected: the test passes and leaves no mutated state when the boundary is violated.

- [ ] **Step 3: Run Postgres API suite**

Run: `TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test postgres_api_flows -- --ignored --test-threads=1`

Expected: all ignored Postgres API tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/api/tests/postgres_api_flows.rs
git commit -m "test: cover postgres non-streaming secrecy"
```

## Verification

Run:

```bash
cargo test -p engine
cargo test -p api --test memory_api_flows
TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test postgres_api_flows -- --ignored --test-threads=1
```

If Docker is unavailable, record that the Postgres command was not run and keep the ignored test ready for CI.

## Acceptance Criteria

- `DefaultTurnPipeline::process_turn` no longer sends GM-only facts to the visible-response provider request.
- The delta-extraction request still receives the oracle context needed for safe state mutation.
- Normal turn responses, debug turn responses, CLI turn output, and streaming final events keep their existing public shape.
- Player-visible generation never receives GM-only facts in engine, memory API, or Postgres API tests.
- Secret-leak validation remains active as a second layer of defense.

## Risks

- Provider call count doubles for non-streaming turns; update tests that seed `MockProvider` with only one response.
- Some provider backends may return quoted JSON strings for visible narration; strip surrounding quotes only when the provider response is a valid JSON string literal.
- Raw provider output storage must not persist the oracle delta-extraction prompt or hidden context in normal player-facing exports.

