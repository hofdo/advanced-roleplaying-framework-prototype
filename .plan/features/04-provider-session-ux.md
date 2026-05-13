# Provider Session UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Improve CLI provider selection, status, and testing so users can confidently choose default and session-scoped LLM providers.

**Architecture:** Build on the existing provider registry in `persistence`, `api::AppState::resolve_provider`, and CLI `provider`/`session set-provider` commands. Add status/test commands that use the same provider construction path as API routes.

**Tech Stack:** Rust, Cargo workspace, Axum, SQLx/Postgres, Clap CLI, serde, tokio tests

---

## Current State

- API has `/providers`, `/providers/test`, `/providers/health`, `/providers/readiness`, `/providers/:id/models`, and `/sessions/:id/provider`.
- CLI `provider` supports `register`, `list`, `remove`, and `models`, but only in Postgres mode.
- CLI `session set-provider` can assign or clear a provider override.
- `api::state::provider_from_record` and `build_provider_registry` are covered by `provider_dispatch_tests.rs`.
- `crates/api/tests/memory_api_flows.rs` covers provider registration, deletion, models, session provider assignment, missing provider errors, and registry lock behavior.
- `chat` and one-shot `turn` currently use `chat.cli.provider`, not session-scoped provider resolution from the store.

## Target Behavior

- `rp provider status` shows default provider readiness and registered providers in Postgres mode.
- `rp provider test [--provider-id UUID]` runs provider health/readiness against default or registered provider.
- `rp session provider <SESSION_ID>` shows the effective provider for a session.
- CLI one-shot turns and chat turns respect session-scoped provider overrides.
- Error messages clearly distinguish missing provider config, unreachable provider backend, and storage mode limitations.

## File Structure

- Modify: `crates/cli/src/commands/provider.rs`
  - Add `status` and `test` commands.
- Modify: `crates/cli/src/commands/session.rs`
  - Add `provider` inspect command or extend `get` output with effective provider details.
- Modify: `crates/cli/src/commands/turn.rs`
  - Resolve session provider before building pipeline.
- Modify: `crates/cli/src/commands/chat.rs`
  - Resolve session provider for each turn or when session changes.
- Modify: `crates/cli/src/bootstrap.rs`
  - Add helper for provider resolution if CLI state owns enough context.
- Modify: `crates/cli/tests/cli_smoke.rs`
  - Add focused smoke tests using mock providers and in-memory limitations.
- Modify: `crates/api/tests/provider_dispatch_tests.rs`
  - Add coverage only if shared provider resolution code moves.

## Tasks

### Task 1: Add CLI Provider Status

**Files:**
- Modify: `crates/cli/src/commands/provider.rs`
- Modify: `crates/cli/tests/cli_smoke.rs`

- [ ] **Step 1: Write failing CLI test**

Add a test for memory mode:

```bash
cargo run -p cli -- provider status
```

Expected stdout includes default provider name and storage status. The command should not require Postgres when it only reports the configured default provider.

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p cli --test cli_smoke provider_status`

Expected: fails because `provider status` is missing or because provider commands reject memory mode globally.

- [ ] **Step 3: Implement command**

Add `Status` to `provider::Cmd`. For memory mode, print:

```text
storage: memory
default: <config.provider.default.name>
readiness: <configured>/<reachable>
registered providers: unavailable in memory mode
```

For Postgres mode, list registered provider IDs, names, provider types, models, and default flags.

- [ ] **Step 4: Run passing test**

Run: `cargo test -p cli --test cli_smoke provider_status`

Expected: status command passes in memory mode.

- [ ] **Step 5: Commit**

```bash
git add crates/cli/src/commands/provider.rs crates/cli/tests/cli_smoke.rs
git commit -m "feat(cli): show provider status"
```

### Task 2: Add CLI Provider Test Command

**Files:**
- Modify: `crates/cli/src/commands/provider.rs`
- Modify: `crates/cli/tests/cli_smoke.rs`

- [ ] **Step 1: Write failing tests**

Add tests:

```rust
provider_test_default_reports_health()
provider_test_registered_requires_postgres_for_provider_id()
```

The default test should use the configured mock provider and assert stdout includes `ok`.

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p cli --test cli_smoke provider_test`

Expected: fails because command is missing.

- [ ] **Step 3: Implement command**

Add:

```rust
Test {
    #[arg(long)]
    provider_id: Option<Uuid>,
}
```

If no provider ID is supplied, call `state.provider.health().await` and `state.provider.readiness().await`. If a provider ID is supplied, load `ProviderRecord` from `state.store.list_providers()`, construct it with `api::provider_from_record` or a shared CLI helper, then call health/readiness.

- [ ] **Step 4: Run tests**

Run: `cargo test -p cli --test cli_smoke provider_test`

Expected: tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/cli/src/commands/provider.rs crates/cli/tests/cli_smoke.rs
git commit -m "feat(cli): test configured providers"
```

### Task 3: Respect Session Provider In CLI Turns

**Files:**
- Modify: `crates/cli/src/bootstrap.rs`
- Modify: `crates/cli/src/commands/turn.rs`
- Modify: `crates/cli/src/commands/chat.rs`
- Modify: `crates/cli/tests/cli_smoke.rs`

- [ ] **Step 1: Write failing one-shot turn test**

Create two mock providers: default returns `default provider used`, registered provider returns `session provider used`. Create a session with `provider_id`, run `rp turn`, and assert response text uses the session provider.

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p cli --test cli_smoke session_provider_turn`

Expected: fails because CLI turn uses the default provider.

- [ ] **Step 3: Add provider resolution helper**

In `bootstrap.rs`, add:

```rust
pub async fn resolve_session_provider(&self, session_id: SessionId) -> Result<Arc<dyn LlmProvider>>
```

It should load the session, inspect `provider_id`, and return default provider when no override exists. In Postgres mode, load provider records and construct the selected provider. In memory tests, use registered providers if the store supports them.

- [ ] **Step 4: Update one-shot turn**

In `turn.rs`, call `state.resolve_session_provider(args.session_id).await?` before building the pipeline.

- [ ] **Step 5: Update chat turn**

In `chat.rs`, call provider resolution in `handle_turn` using the active session ID. If resolution fails, return an error before provider invocation.

- [ ] **Step 6: Run tests**

Run: `cargo test -p cli --test cli_smoke session_provider_turn`

Run: `cargo test -p api --test memory_api_flows turn_with_missing_session_provider_returns_409`

Expected: CLI respects session provider; API behavior remains unchanged.

- [ ] **Step 7: Commit**

```bash
git add crates/cli/src/bootstrap.rs crates/cli/src/commands/turn.rs crates/cli/src/commands/chat.rs crates/cli/tests/cli_smoke.rs
git commit -m "feat(cli): honor session provider overrides"
```

### Task 4: Add Session Provider Inspect Command

**Files:**
- Modify: `crates/cli/src/commands/session.rs`
- Modify: `crates/cli/tests/cli_smoke.rs`

- [ ] **Step 1: Write failing test**

Add a test for:

```bash
cargo run -p cli -- session provider <SESSION_ID>
```

Assert it prints session ID, provider mode (`default` or `override`), and provider ID when set.

- [ ] **Step 2: Run expected failing command**

Run: `cargo test -p cli --test cli_smoke session_provider_command`

Expected: fails because command is missing.

- [ ] **Step 3: Implement command**

Add:

```rust
Provider {
    session_id: Uuid,
}
```

Load session, print `provider: default` when `provider_id` is `None`, otherwise print `provider: <UUID>`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p cli --test cli_smoke session_provider_command`

Expected: test passes.

- [ ] **Step 5: Commit**

```bash
git add crates/cli/src/commands/session.rs crates/cli/tests/cli_smoke.rs
git commit -m "feat(cli): inspect session provider"
```

## Verification

Run:

```bash
cargo test -p cli --test cli_smoke provider_status provider_test session_provider
cargo test -p cli
cargo test -p api --test memory_api_flows
cargo test -p api --test provider_dispatch_tests
```

Optional with Docker:

```bash
TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test postgres_api_flows -- --ignored --test-threads=1
```

## Acceptance Criteria

- CLI provider status works in memory and Postgres modes.
- CLI provider test can check the default provider and a registered provider.
- CLI one-shot turns and chat turns use session-scoped provider overrides.
- Users can inspect a session's effective provider.
- API provider behavior remains covered and unchanged.

## Risks

- Constructing registered providers in CLI may duplicate API code; prefer sharing existing provider construction helpers.
- Provider health calls can be slow or network-bound; honor existing provider timeouts.
- Memory-mode tests may need a test-only way to seed provider registry data.

