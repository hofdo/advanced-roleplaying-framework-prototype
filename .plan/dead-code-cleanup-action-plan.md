# Dead Code Cleanup Action Plan

## Summary

This document is the implementation handoff for the current dead/unused code cleanup. It captures the real production issue we found, the test-helper warning noise around it, and the exact order to fix both without changing unrelated behavior.

## What Needs To Change

### 1. Remove the unused `provider_name` request field

`SetProviderRequest` in `crates/api/src/app.rs` currently includes `provider_name`, but the handler only reads `provider_id`. That makes the field dead production API surface.

Fix it by:

- removing `provider_name` from the request type
- keeping `PATCH /sessions/:id/provider` as a `provider_id`-only endpoint
- updating any request examples, docs, and tests so they only use `provider_id`
- preserving the existing `null` behavior for clearing the session provider

### 2. Reduce test helper dead-code warnings structurally

`crates/api/tests/common/mod.rs` is compiled into multiple integration test crates, so helpers that are used in one test binary but not another show up as `dead_code` warnings.

Fix it by:

- splitting the shared helper module into narrower helper modules by concern
- keeping only the helpers each integration test file actually needs
- moving Postgres-specific setup and reset logic into a Postgres helper module
- avoiding broad `#[allow(dead_code)]` unless one or two narrow leftovers remain after the split

## Execution Order

1. Remove `provider_name` and update the API contract to `provider_id` only.
2. Split the integration test helper surface so test binaries stop compiling unused helpers.
3. Re-run the shared Postgres and local LLM test workflow to confirm behavior did not regress.

## Verification

Run these checks after the cleanup:

```bash
cargo check --workspace
cargo test --workspace
bash scripts/test-with-local-llm.sh gemma4-uncensored
```

Success criteria:

- the production `provider_name` dead-code warning is gone
- the remaining test warnings are reduced or clearly justified
- the session-provider API still behaves exactly as before for `provider_id` and `null`
- the shared-Postgres local LLM workflow still passes end to end

## Assumptions

- Name-based session-provider assignment is not required.
- The repo should keep `.plan/` as the place for implementation handoff documents.
- The goal is cleanup, not a broader API redesign.
