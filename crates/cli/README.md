# CLI Crate

## Purpose

The `cli` crate is a terminal driver for the roleplaying engine. It compiles to a single binary named `rp` that links the engine library directly and exercises the same pipeline the HTTP API uses — without going through HTTP. The goal is fast inner-loop dogfooding: try a scenario, run a few turns, inspect projected state, swap providers, all from the terminal.

It should stay thin. It should not host its own engine logic, not own its own prompt rules, not duplicate validation. It is wiring and rendering.

## What Lives Here

- `src/main.rs` parses arguments with `clap` and dispatches to the relevant subcommand.
- `src/bootstrap.rs` builds `CliState` (`store`, `provider`, `turn_lock`) from `AppConfig`. Defaults to the in-memory store; `--postgres` opts into the persistent backend.
- `src/commands/scenario.rs` — create / list / get / delete scenarios.
- `src/commands/session.rs` — create / list / get sessions and assign providers.
- `src/commands/turn.rs` — submit a blocking turn or stream narration tokens live.
- `src/commands/world.rs` — show player-projected state or, with `--admin`, the raw `WorldState`.
- `src/commands/provider.rs` — Postgres-only: register, list, remove, and probe registered LLM providers.
- `src/samples.rs` — built-in sample scenarios for fast onboarding without writing JSON.
- `src/render.rs` — small JSON pretty-printer used by the subcommand handlers.
- `tests/cli_smoke.rs` — in-process integration test driving the full scenario → session → turn → world cycle.

## Why It Exists

The HTTP API is a fine production surface but a heavy testbed. Iterating on prompt content, scenario shape, projection rules, or provider switching through `curl` is slow. The CLI removes the HTTP round trip without forking the engine: everything below `crates/cli/src/commands` ends up calling the same `engine::DefaultTurnPipeline::process_turn` and `engine::stream_turn` that `crates/api` calls.

This also makes the CLI the natural place to expose admin-only views. `--admin` on `turn` and `world` swaps the player `ViewerContext` for one that surfaces GM-only facts, matching the secrecy semantics of the `/admin/sessions/:id/export/raw` HTTP route. Normal invocations stay player-safe.

## Engine Context

```text
                 ┌──────────────────────────────────────┐
                 │              crates/engine           │
                 │  pipeline, stream_turn, projection,  │
                 │  prompt builder, validation, safety  │
                 └─────────────────▲────────────────────┘
                                   │
              ┌────────────────────┴────────────────────┐
              │                                         │
        crates/api                                   crates/cli
        (Axum HTTP)                                  (rp binary)
```

Both binaries are thin wrappers. Engine logic lives once. The CLI never reaches around the engine — it composes the same components, picks the same store implementations, honors the same turn lock, and runs the same projection.

For streaming, the CLI consumes `engine::stream_turn` directly. The same function powers the HTTP `/sessions/:id/turn/stream` SSE handler. Tokens, provider metadata, and the terminal `Final` event are typed engine events; only the rendering differs between the two callers.

## Usage

### Quickstart (memory mode)

In memory mode, state lives only for the lifetime of the process. Use it for one-shot smoke tests and prompt iteration. For multi-command sessions, use `--postgres`.

```bash
cargo run -p cli -- scenario create --sample chosen-beyond-goddess
cargo run -p cli -- scenario list
```

### Multi-command flows (Postgres)

```bash
docker compose up -d postgres
export DATABASE_URL=postgres://roleplay:roleplay@localhost:5432/roleplay
cargo run -p cli -- --postgres scenario create --sample chosen-beyond-goddess
cargo run -p cli -- --postgres session create --scenario <SCENARIO_ID> --title smoke
cargo run -p cli -- --postgres turn <SESSION_ID> --input "I greet the examiner." --stream
cargo run -p cli -- --postgres world <SESSION_ID>
cargo run -p cli -- --postgres world <SESSION_ID> --admin
```

`--postgres` can also be enabled by `ROLEPLAY_CLI_POSTGRES=1`.

### LLM provider

The CLI reuses the same `AppConfig` resolution as the HTTP API. Point it at whichever provider you want:

```bash
# Local llama-server
export LLM_PROVIDER_TYPE=llama_cpp
export LLM_BASE_URL=http://localhost:8081/v1
export LLM_MODEL=local-model
cargo run -p cli -- turn <SESSION_ID> --input "describe the room" --stream

# OpenRouter
export LLM_PROVIDER_TYPE=openrouter
export LLM_BASE_URL=https://openrouter.ai/api/v1
export LLM_MODEL=openai/gpt-4o-mini
export LLM_API_KEY=env:OPENROUTER_API_KEY
export OPENROUTER_API_KEY=sk-or-...
cargo run -p cli -- turn <SESSION_ID> --input "describe the room" --stream
```

### Subcommand reference

| Command | Description |
|---|---|
| `scenario create [--file PATH \| --sample NAME]` | Create from JSON on disk or a built-in sample (`chosen-beyond-goddess`) |
| `scenario list / get <ID> / delete <ID>` | Standard scenario management |
| `session create --scenario <ID> [--title TEXT]` | Start a session for an existing scenario |
| `session list / get <ID>` | Enumerate / inspect sessions |
| `session set-provider <ID> [--provider-id UUID \| --clear]` | Pin a session to a registered provider, or fall back to the default |
| `turn <SESSION_ID> --input TEXT [--mode action\|dialogue\|direct\|remember] [--stream] [--admin]` | Submit a turn; `--stream` renders tokens live, `--admin` enables GM-only visibility |
| `world <SESSION_ID> [--admin]` | Print projected (player-safe) state; `--admin` returns the raw `WorldState` |
| `provider register --file PATH` | Postgres only: persist a `ProviderConfig` |
| `provider list / remove <ID> / models <ID>` | Postgres only: enumerate the registry, remove an entry, or list models a provider exposes |

### Output

JSON-producing commands print pretty-printed JSON to stdout. Streaming turns write tokens to stdout as they arrive and finish with a `---` separator followed by `world_state_version`, `changed_entities`, and (when the provider reports them) `usage` and `cost_usd`. Errors go to stderr with a non-zero exit code; the CLI never panics on a provider or store failure.

Logs follow the `RP_LOG` env var (defaults to `warn`) and are routed to stderr so they don't contaminate stdout JSON.

### Secrecy boundary

The CLI honors the same secrecy contract as the HTTP API. Without `--admin`:

- `world` returns `FrontendVisibleState` (no GM-only facts, no hidden clocks, no raw provider output).
- `turn` runs the full pipeline with `ViewerContext::player()`, so the projected state patch in the response is player-safe.

With `--admin`:

- `world` returns the raw `WorldState` including GM-only facts.
- `turn` projects through an admin `ViewerContext`, matching the `/admin/*` HTTP routes.

There is no other knob for raising visibility. Anything that requires admin context must pass `--admin` explicitly.

## Testing

`crates/cli/tests/cli_smoke.rs` runs the full scenario → session → turn → world cycle in-process against `InMemoryApplicationStore` and `MockProvider`. It also covers the streaming path through `engine::stream_turn`. Run with:

```bash
cargo test -p cli --test cli_smoke
```

These tests do not spawn the binary; they assemble `CliState` directly to keep them fast. End-to-end binary checks live alongside the manual runbook in the workspace `README.md`.
