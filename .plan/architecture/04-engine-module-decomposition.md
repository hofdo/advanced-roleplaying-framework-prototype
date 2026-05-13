# Engine Module Decomposition Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split large engine modules into focused submodules after safety behavior is locked, without changing runtime behavior.

**Architecture:** Extract by responsibility, keep public re-exports stable in `crates/engine/src/lib.rs`, and run tests after every extraction. Do not mix behavior changes with movement; behavior changes belong in feature plans before this decomposition pass.

**Tech Stack:** Rust, Cargo workspace, Axum, SQLx/Postgres, Clap CLI, serde, tokio tests

---

## Current State

- `crates/engine/src/prompt.rs` owns prompt traits, prompt rendering, fact relevance, output parser types, JSON parser, repair prompt, and hidden reasoning stripping.
- `crates/engine/src/pipeline.rs` owns request/response types, store traits, pipeline events, preparation, finalization, normal turns, debug turns, and error types.
- `crates/engine/src/validation.rs` owns delta validation and validation error types.
- `crates/engine/src/reducer.rs` owns all delta application behavior.
- `crates/engine/src/projection.rs` owns frontend projection and changed entity detection.
- Existing tests live partly inline and partly under `crates/engine/tests/`.

## Target Behavior

- Behavior stays byte-for-byte equivalent from caller perspective.
- Module boundaries make secrecy rendering, parsing, pipeline orchestration, validation, reduction, and projection easier to review.
- `api`, `cli`, and tests keep importing the same public engine names unless there is a narrow, justified rename.
- Each extraction is a small compile-and-test step.

## File Structure

- Modify: `crates/engine/src/lib.rs`
  - Re-export public names from new module paths.
- Split: `crates/engine/src/prompt.rs`
  - Create `crates/engine/src/prompt/mod.rs`
  - Create `crates/engine/src/prompt/render.rs`
  - Create `crates/engine/src/prompt/parser.rs`
  - Create `crates/engine/src/prompt/repair.rs`
  - Create `crates/engine/src/prompt/strip.rs`
- Split: `crates/engine/src/pipeline.rs`
  - Create `crates/engine/src/pipeline/mod.rs`
  - Create `crates/engine/src/pipeline/types.rs`
  - Create `crates/engine/src/pipeline/events.rs`
  - Create `crates/engine/src/pipeline/non_streaming.rs` only if it reduces file size without adding indirection.
- Keep: `validation.rs`, `reducer.rs`, and `projection.rs` until prompt and pipeline splits are stable.

## Tasks

### Task 1: Snapshot Public API Before Moving Code

**Files:**
- Read: `crates/engine/src/lib.rs`
- Read: `crates/api/src/app.rs`
- Read: `crates/cli/src/commands/turn.rs`

- [ ] **Step 1: Capture current public exports**

Run: `sed -n '1,220p' crates/engine/src/lib.rs`

Expected: list of module declarations and re-exports used by `api`, `cli`, and tests.

- [ ] **Step 2: Run baseline tests**

Run: `cargo test -p engine`

Expected: all engine tests pass before moving code.

- [ ] **Step 3: Commit only if baseline requires no code edits**

No commit is needed if this task only records baseline status. If a test was already failing, stop and fix the failing behavior in a separate branch before decomposition.

### Task 2: Extract Prompt Parser And Repair Code

**Files:**
- Modify: `crates/engine/src/prompt.rs`
- Create: `crates/engine/src/prompt/parser.rs`
- Create: `crates/engine/src/prompt/repair.rs`
- Create: `crates/engine/src/prompt/mod.rs`
- Modify: `crates/engine/src/lib.rs`

- [ ] **Step 1: Move parser types**

Move `PlayerTurnModelOutput`, `ResponseParser`, `JsonResponseParser`, and `ParseError` into `prompt/parser.rs`. Keep the same public names by re-exporting them from `prompt/mod.rs` and `lib.rs`.

- [ ] **Step 2: Move repair prompt**

Move `repair_prompt` into `prompt/repair.rs`. Re-export it so `pipeline.rs` keeps compiling with its current import.

- [ ] **Step 3: Run formatter and tests**

Run: `cargo fmt --all`

Run: `cargo test -p engine prompt`

Expected: prompt parser and repair tests pass with no behavior changes.

- [ ] **Step 4: Run dependent compile check**

Run: `cargo check -p api -p cli`

Expected: `api` and `cli` compile with existing imports.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src
git commit -m "refactor: split prompt parser code"
```

### Task 3: Extract Prompt Rendering And Stripping

**Files:**
- Modify: `crates/engine/src/prompt/mod.rs`
- Create: `crates/engine/src/prompt/render.rs`
- Create: `crates/engine/src/prompt/strip.rs`

- [ ] **Step 1: Move rendering helpers**

Move `render_context`, `render_narration_context`, `render_facts_section`, `render_gm_only_facts`, `relevant_gm_only_facts`, `tokenize`, `is_noise_token`, `render_section`, `render_single_value_section`, and `join_or_none` into `prompt/render.rs`.

- [ ] **Step 2: Keep helper visibility narrow**

Expose only functions needed by `BasicPromptBuilder` as `pub(super)` or `pub(crate)`. Keep tokenization private to the render module.

- [ ] **Step 3: Move hidden reasoning stripping**

Move `HiddenReasoningStripper` and `BasicHiddenReasoningStripper` into `prompt/strip.rs` and re-export the public names.

- [ ] **Step 4: Run tests**

Run: `cargo test -p engine prompt`

Run: `cargo test -p api --test memory_api_flows in_memory_turn_cycle_applies_delta_and_returns_response`

Expected: prompt behavior and a representative API turn still pass.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src
git commit -m "refactor: split prompt rendering code"
```

### Task 4: Extract Pipeline Types And Events

**Files:**
- Modify: `crates/engine/src/pipeline.rs`
- Create: `crates/engine/src/pipeline/types.rs`
- Create: `crates/engine/src/pipeline/events.rs`
- Create: `crates/engine/src/pipeline/mod.rs`
- Modify: `crates/engine/src/lib.rs`

- [ ] **Step 1: Move data types**

Move `TurnRequestInput`, `TurnResponse`, `DebugTurnResponse`, `LoadedTurnState`, `PreparedTurn`, and `FinalizedTurn` into `pipeline/types.rs`.

- [ ] **Step 2: Move event enum**

Move `PipelineEventKind` and `PipelineEventKind::as_str` into `pipeline/events.rs`.

- [ ] **Step 3: Keep trait and implementation together**

Keep `TurnStateStore`, `DefaultTurnPipeline`, and `TurnPipelineError` in `pipeline/mod.rs` for this pass. Do not split normal/debug/streaming finalization yet.

- [ ] **Step 4: Run tests**

Run: `cargo test -p engine`

Run: `cargo test -p api --test memory_api_flows`

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src
git commit -m "refactor: split pipeline types"
```

### Task 5: Evaluate Validation, Reducer, And Projection Splits

**Files:**
- Read: `crates/engine/src/validation.rs`
- Read: `crates/engine/src/reducer.rs`
- Read: `crates/engine/src/projection.rs`

- [ ] **Step 1: Measure file sizes**

Run: `wc -l crates/engine/src/validation.rs crates/engine/src/reducer.rs crates/engine/src/projection.rs`

Expected: identify files that still exceed comfortable review size.

- [ ] **Step 2: Split only if responsibility is obvious**

If splitting, use these boundaries:

```text
validation/entity_refs.rs       known-id checks and require_reason
validation/secrets.rs           secret leak checks
reducer/entities.rs             per-entity delta application helpers
projection/changed_entities.rs  changed entity detection
```

If a split requires changing behavior or public types, stop and create a focused behavior plan instead.

- [ ] **Step 3: Run full workspace tests**

Run: `cargo test --workspace`

Expected: default workspace tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/engine/src
git commit -m "refactor: narrow engine module responsibilities"
```

## Verification

Run:

```bash
cargo fmt --all
cargo check --workspace
cargo test --workspace
```

If Docker-backed tests are available, also run:

```bash
TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test postgres_api_flows -- --ignored --test-threads=1
```

## Acceptance Criteria

- No behavior changes are introduced by this plan.
- Existing public engine imports continue to compile for `api`, `cli`, and tests.
- Prompt rendering, parsing, repair, and stripping live in separate focused files.
- Pipeline types/events are separated from orchestration code.
- Tests stay green after each extraction commit.

## Risks

- Moving inline tests can accidentally reduce coverage; keep tests close unless a new module owns the behavior.
- Rust module re-export mistakes can break downstream crates even when engine tests pass; run `cargo check --workspace`.
- Splitting too far can make orchestration harder to read. Stop once files are reviewable.

