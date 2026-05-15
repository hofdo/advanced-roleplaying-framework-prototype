# CLI Crate

## Purpose

The `cli` crate is a terminal driver for the roleplaying engine. It compiles to a single binary named `rp` that links the engine library directly and exercises the same pipeline the HTTP API uses — without going through HTTP. The goal is fast inner-loop dogfooding: try a scenario, run a few turns, inspect projected state, swap providers, all from the terminal.

It should stay thin. It should not host its own engine logic, not own its own prompt rules, not duplicate validation. It is wiring and rendering.

## What Lives Here

- `src/main.rs` parses arguments with `clap` and dispatches to the relevant subcommand.
- `src/bootstrap.rs` builds `CliState` (`store`, `provider`, `turn_lock`) from `AppConfig`. Defaults to the in-memory store; `--postgres` opts into the persistent backend.
- `src/commands/scenario.rs` — create / list / get / delete / validate / inspect scenarios, plus sample and template helpers.
- `src/commands/session.rs` — create / list / get sessions, inspect timelines, inspect provider bindings, and export replay fixtures.
- `src/commands/dev.rs` — convenience launchers for local llama.cpp and OpenRouter-backed chat sessions.
- `src/commands/turn.rs` — submit a blocking turn or stream narration tokens live.
- `src/commands/world.rs` — show player-projected state or, with `--admin`, the raw `WorldState`.
- `src/commands/provider.rs` — Postgres-only: register, list, remove, probe, inspect status, and test registered LLM providers.
- `src/samples.rs` — embedded JSON sample catalog for fast onboarding without writing JSON.
- `src/scenario_io.rs` — scenario JSON parsing and domain validation for file imports.
- `scenarios/samples/*.json` — built-in scenario definitions. Add a file here to add a built-in sample.
- `scenarios/templates/scenario.template.json` — copyable authoring skeleton for new scenarios.
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

### Quickstart — chat mode (recommended)

`rp chat` is the interactive REPL. State lives for the lifetime of the process; multi-turn play works without copying UUIDs:

```bash
cargo run -p cli -- chat --sample chosen-beyond-goddess
cargo run -p cli -- chat --sample chosen-beyond-goddess --view quiet
```

Inside the REPL, plain text becomes a turn. Lines starting with `/` are slash-commands:

```
loaded sample scenario abc-123 (session def-456)
Chosen Beyond the Goddess

Setting
A high fantasy isekai world of sword and magic.

Opening
You begin in Guildhall. Guild Examiner is the first visible voice in the scene.

Situation
Immediate concern: Register at the Guild. Complete the registration process.
Pressure: The player's fame spreads stands at 1/6.

type /help for commands, /exit to quit.
rp> /status
scenario: abc-123
session:  def-456
mode:     auto
stream:   on
admin:    off
view:     verbose
rp> The examiner steps forward and asks me to declare my mage rank.
[tokens stream live ...]
---
world_state_version: 1
changed_entities: [...]
rp> /mode dialogue
mode set to dialogue
rp> /world
{ ... projected state ... }
rp> /admin on
admin on
rp!> /world
{ ... raw state including GM-only facts ... }
rp!> /exit
goodbye.
```

The prompt is `rp> ` normally and `rp!> ` while admin is on. `?` instead of `>` means no session is active.

#### Slash commands

| Command | Description |
|---|---|
| `/help` | Show in-REPL command reference |
| `/exit`, `/quit` | Leave the REPL |
| `/status` | Print active scenario / session / mode / stream / admin |
| `/scenario create --sample NAME` | Build and persist a built-in sample |
| `/scenario create --file PATH` | Load a scenario from JSON |
| `/scenario list` | List scenarios |
| `/scenario use <UUID>` | Switch active scenario (clears active session) |
| `/session new [--title TEXT]` | Start a fresh session for the active scenario |
| `/session list` | List sessions |
| `/session use <UUID>` | Switch active session |
| `/session show` | Print the active session record |
| `/world [--admin]` | Show projected (or raw) world state |
| `/mode <action\|dialogue\|direct\|remember\|auto>` | Set turn mode for plain-text turns |
| `/stream <on\|off>` | Toggle live token streaming |
| `/admin <on\|off>` | Toggle admin viewer for turns and `/world` |
| `/view <verbose\|quiet>` | Toggle terminal presentation style |

Plain text (any line not starting with `/`) is submitted as a turn against the active session. Streaming is on by default. `verbose` preserves the current metadata-heavy output. `quiet` renders only the player-facing text plus the persisted opening intro for new sessions.

Line history is persisted to `~/.config/rp/history`. `Ctrl+C` cancels an in-flight turn (releases the session lock immediately). `Ctrl+D` exits cleanly.

### Quickstart — one-shot commands (memory mode)

In memory mode, state lives only for the lifetime of the process. Use it for one-shot smoke tests and prompt iteration. For multi-command flows without entering chat mode, use `--postgres`.

```bash
cargo run -p cli -- scenario create --sample chosen-beyond-goddess
cargo run -p cli -- scenario list
```

Built-in sample names are generated from `crates/cli/scenarios/samples/*.json`:

- `ashfall-murder`
- `chosen-beyond-goddess`
- `glass-senate-crisis`
- `bride-of-the-iron-archduke`

Use `crates/cli/scenarios/templates/scenario.template.json` as a starting point for custom `--file` imports. Both built-in samples and imported files are deserialized as `domain::Scenario` and validated before storage.

### Multi-command flows (Postgres)

```bash
docker compose up -d postgres
export DATABASE_URL=postgres://roleplay:roleplay@localhost:5432/roleplay
cargo run -p cli -- --postgres scenario create --sample chosen-beyond-goddess
cargo run -p cli -- --postgres session create --scenario <SCENARIO_ID> --title smoke
cargo run -p cli -- --postgres turn <SESSION_ID> --input "I greet the examiner." --stream
cargo run -p cli -- --postgres turn <SESSION_ID> --input "I greet the examiner." --view quiet
cargo run -p cli -- --postgres world <SESSION_ID>
cargo run -p cli -- --postgres world <SESSION_ID> --admin
```

`--postgres` can also be enabled by `ROLEPLAY_CLI_POSTGRES=1`.

### Dev launchers

The `dev` command starts the durable backend and opens the interactive REPL with a preset provider config:

```bash
cargo run -p cli -- dev local
cargo run -p cli -- dev open-router
cargo run -p cli -- dev local --destroy
cargo run -p cli -- dev local --scenario <SCENARIO_ID>
cargo run -p cli -- dev open-router --scenario <SCENARIO_ID>
cargo run -p cli -- dev local --view quiet
```

`dev local` starts `docker compose up -d postgres`, launches `scripts/start-llm.sh`, waits for the local llama-server health checks, and then opens chat on `chosen-beyond-goddess` unless `--sample NAME` or `--scenario UUID` is supplied.

`dev open-router` starts the same Postgres backend, configures the OpenRouter provider preset, and then opens chat on `chosen-beyond-goddess` unless `--sample NAME` or `--scenario UUID` is supplied. Set `OPENROUTER_API_KEY` before running it.

Add `--destroy` to either launcher if you want it to run `docker compose down` for the owned Postgres stack on exit instead of just stopping it.

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
cargo run -p cli -- turn <SESSION_ID> --input "describe the room" --view quiet
```

### Subcommand reference

| Command | Description |
|---|---|
| `chat [--session UUID \| --scenario UUID \| --sample NAME] [--mode MODE] [--admin] [--view verbose\|quiet]` | Interactive REPL (see Chat mode section above) |
| `dev local [--sample NAME \| --scenario UUID] [--destroy] [--view verbose\|quiet]` | Start Postgres, launch the local llama.cpp stack, and open chat on the default sample or a specific scenario |
| `dev open-router [--sample NAME \| --scenario UUID] [--destroy] [--view verbose\|quiet]` | Start Postgres, configure OpenRouter, and open chat on the default sample or a specific scenario |
| `scenario create [--file PATH \| --sample NAME]` | Create from validated JSON on disk or a built-in sample |
| `scenario list / get <ID> / delete <ID> / validate [--file PATH \| --sample NAME] / inspect <ID> / samples / template` | Scenario management, validation, inspection, sample listing, and template export |
| `session create --scenario <ID> [--title TEXT]` | Start a session for an existing scenario |
| `session list / get <ID> / timeline <ID> / provider <ID> / export-fixture <ID>` | Enumerate sessions, inspect timeline/provider state, or export replay fixtures |
| `session set-provider <ID> [--provider-id UUID \| --clear]` | Pin a session to a registered provider, or fall back to the default |
| `turn <SESSION_ID> --input TEXT [--mode action\|dialogue\|direct\|remember] [--stream] [--admin] [--view verbose\|quiet]` | Submit a turn; `--stream` renders tokens live, `--admin` enables GM-only visibility |
| `world <SESSION_ID> [--admin]` | Print projected (player-safe) state; `--admin` returns the raw `WorldState` |
| `provider register --file PATH` | Postgres only: persist a `ProviderConfig` |
| `provider list / remove <ID> / models <ID> / status <ID> / test` | Postgres only: enumerate the registry, remove an entry, inspect status, test connectivity, or list models a provider exposes |

### Output

JSON-producing commands print pretty-printed JSON to stdout. In `verbose` view, streaming turns write tokens to stdout as they arrive and finish with a `---` separator followed by `world_state_version`, `changed_entities`, and (when the provider reports them) `usage` and `cost_usd`. In `quiet` view, turns print only the player-facing text. Every newly created session also persists a player-facing opening intro as the first `system_message`; chat and dev commands print that intro immediately when they create the session, and timeline/history commands show it later without changing the `session create` JSON payload.

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
