# Advanced Roleplaying Framework Prototype

Rust backend prototype for an LLM-driven roleplaying engine.

The engine accepts player input, builds role-aware context, calls an LLM provider, validates proposed world-state changes, persists authoritative state, and returns only frontend-safe state to normal player-facing clients.

The project is focused on **backend orchestration and safety boundaries**, not frontend rendering.

## Current Status

The repository currently implements a working backend architecture with:

- scenario and session management
- role-aware prompt construction
- non-streaming and streaming turn flows
- typed `WorldStateDelta` mutations
- deterministic validation and reduction of LLM-proposed changes
- frontend-safe state projection
- PostgreSQL persistence plus in-memory mode
- provider registration and session-scoped provider selection
- hidden-reasoning stripping
- structured-output repair and provider retry support
- admin/debug routes for raw state inspection
- unit, in-memory integration, provider, and Docker-gated PostgreSQL tests

The current priority is **hardening correctness and secrecy boundaries**, not expanding gameplay features.

### Known Limitations

**Non-streaming secrecy boundary**

The streaming narration path now uses a narration-safe prompt context that excludes GM-only facts.

The non-streaming turn path still asks the model to produce both:

- player-visible `player_response`
- structured `world_state_delta`

from a shared context that includes GM-only facts. This is a known architectural limitation: player-visible narration and oracle/delta reasoning are still combined in one non-streaming LLM call. A safer future design is to split non-streaming turns into:

1. narration-safe visible response generation
2. oracle-context delta extraction

**Turn locking is in-memory only**

Session turn locking is currently held in application memory. Multiple API instances will not coordinate locks across processes. PostgreSQL-backed distributed locking is not yet implemented. Run a single API instance when using PostgreSQL storage until this is resolved.

**Context selection is rule-based**

Prompt context is built with compact, deterministic rules. There is no retrieval-based or embedding-based context selection yet. Long sessions with many facts, events, or NPCs may exceed useful context windows before intelligent summarization or retrieval is available.

## Turn Pipeline

Intended engine flow:

```text
player input
-> acquire session turn lock
-> load session, scenario, world state, and recent messages
-> classify scene
-> activate role identity
-> build prompt context
-> call LLM provider
-> parse model output
-> strip hidden reasoning
-> validate proposed delta
-> reduce authoritative world state
-> persist messages, events, delta, and new state version
-> project frontend-safe state
-> return response
```

For streaming turns, visible narration is streamed first and structured delta extraction happens afterward before final state is persisted.

## Architecture

```text
crates/
  api/          Axum HTTP API, routes, application state
  cli/          `rp` CLI for driving the engine end-to-end from the terminal
  domain/       Core domain types and typed delta variants
  engine/       Turn pipeline, prompts, validation, reducers, projection
  persistence/  PostgreSQL repositories and migrations
  providers/    LLM provider abstraction and implementations
  shared/       Shared configuration and common types
```

Each crate also has a local `README.md` with ownership, boundary, and engine-context notes. Start with [`crates/README.md`](crates/README.md) for the workspace map.

### Core Design Rules

- The frontend must not build prompts.
- The frontend must not mutate authoritative world state directly.
- The frontend must not receive GM-only or raw authoritative state on normal routes.
- The LLM may propose typed deltas, but it must not overwrite full world state.
- The backend validates, reduces, persists, and projects state.
- One session should process only one active turn at a time.
- Streaming text must not mutate state before a validated delta is finalized.

## Workspace

The Rust workspace contains:

```text
crates/api
crates/cli
crates/domain
crates/engine
crates/persistence
crates/providers
crates/shared
```

Workspace metadata:

```text
edition: 2024
license: Apache-2.0
```

## LLM Providers

Three provider types are supported, selected via `LLM_PROVIDER_TYPE`:

| `provider_type` | Description |
|---|---|
| `openai_compatible` (default) | Generic OpenAI-compatible HTTP endpoint |
| `llama_cpp` | Local `llama-server` — real `/health` + `/props` probes, control-token filtering |
| `openrouter` | OpenRouter cloud — attribution headers, provider routing, model catalog, usage/cost capture |

### llama.cpp (local)

```bash
bash scripts/start-llm.sh gemma4-uncensored

export LLM_PROVIDER_TYPE=llama_cpp
export LLM_BASE_URL=http://localhost:8080/v1
export LLM_MODEL=your-model-name
cargo run -p api
```

The helper script starts `llama-server` with a HuggingFace-backed model and defaults to port `8080`.
If the model is already cached locally, no `HF_TOKEN` is needed. Set `HF_TOKEN` only when the selected model must be fetched from HuggingFace.
Available model aliases today:

- `gemma4-uncensored`
- `qwen3-uncensored`

If you prefer to keep the older `8081` local convention, override the script port:

```bash
LLAMA_PORT=8081 bash scripts/start-llm.sh gemma4-uncensored
```

### OpenRouter (cloud)

```bash
export LLM_PROVIDER_TYPE=openrouter
export LLM_BASE_URL=https://openrouter.ai/api/v1
export LLM_MODEL=openai/gpt-4o-mini
export LLM_API_KEY=env:OPENROUTER_API_KEY   # resolves from env at startup
export LLM_HTTP_REFERER=https://your-app.example.com
export LLM_X_TITLE=YourAppName
export OPENROUTER_API_KEY=sk-or-...
cargo run -p api
```

Or register via API at runtime:
```bash
curl -X POST http://localhost:8080/providers \
  -H "Content-Type: application/json" \
  -d '{
    "name": "openrouter",
    "provider_type": "openrouter",
    "base_url": "https://openrouter.ai/api/v1",
    "model": "openai/gpt-4o-mini",
    "api_key_secret_ref": "env:OPENROUTER_API_KEY",
    "capabilities": {
      "supports_streaming": true,
      "supports_model_listing": true,
      "supports_usage_reporting": true,
      "supports_cost_reporting": true,
      "http_referer": "https://your-app.example.com",
      "x_title": "YourAppName"
    }
  }'
```

### API key secret references

`api_key_secret_ref` (and `LLM_API_KEY`) accept either a plain string or an env-var reference:

- `sk-or-abc123` — used as-is
- `env:OPENROUTER_API_KEY` — resolved from `$OPENROUTER_API_KEY` at provider construction time; fails loudly if the var is not set

## Prerequisites

- Rust stable
- Docker, when using PostgreSQL locally or running Docker-gated tests
- `llama-server` installed locally, when using `scripts/start-llm.sh`
- An LLM endpoint: local `llama-server`, an OpenAI-compatible server, or an OpenRouter API key

## Local Setup

### Start PostgreSQL

```bash
docker compose up -d postgres
```

The provided compose file starts PostgreSQL 16 with:

```text
database: roleplay
user: roleplay
password: roleplay
port: 5432
```

### Start the local llama.cpp server

```bash
bash scripts/start-llm.sh gemma4-uncensored
```

The script defaults to `http://127.0.0.1:8080/v1`. If the model is already cached locally, no `HF_TOKEN` is needed. If you run the API against it, point `LLM_BASE_URL` there or start the script with `LLAMA_PORT=8081`.

### Run the API with PostgreSQL

```bash
export DATABASE_URL=postgres://roleplay:roleplay@localhost:5432/roleplay
export ROLEPLAY_STORAGE=postgres
export LLM_PROVIDER_TYPE=llama_cpp
export LLM_BASE_URL=http://localhost:8080/v1
export LLM_MODEL=local-model

cargo run -p api
```

### Run in memory mode

Use memory mode for fast local experiments without durable storage:

```bash
ROLEPLAY_STORAGE=memory cargo run -p api
```

### Health check

```bash
curl http://127.0.0.1:8080/health
```

Expected shape:

```json
{
  "status": "ok",
  "active_provider": "local-llama",
  "database": "postgres:ok"
}
```

## CLI (`rp`)

The `rp` binary drives the engine directly — no HTTP layer involved — so you can play test scenarios from the terminal. Defaults to the in-memory store; pass `--postgres` (or set `ROLEPLAY_CLI_POSTGRES=1`) for the durable backend.

```bash
# Interactive chat mode — recommended for actual play
cargo run -p cli -- chat --sample chosen-beyond-goddess
# Inside the REPL: plain text submits a turn, slash-commands manage state.
# /help lists everything.

# One-shot commands
cargo run -p cli -- scenario create --sample chosen-beyond-goddess
cargo run -p cli -- session create --scenario <SCENARIO_ID> --title "Smoke"
cargo run -p cli -- turn <SESSION_ID> --input "I greet the examiner."
cargo run -p cli -- turn <SESSION_ID> --input "I draw my sword." --stream
cargo run -p cli -- world <SESSION_ID>
cargo run -p cli -- world <SESSION_ID> --admin   # includes GM-only facts
```

Built-in sample scenarios are `ashfall-murder`, `bride-of-the-iron-archduke`, `chosen-beyond-goddess`, and `glass-senate-crisis`. Custom scenarios can be copied from `crates/cli/scenarios/templates/scenario.template.json` and loaded with `scenario create --file PATH`; imports are validated before storage.

Subcommands:

| Command | Description |
|---|---|
| `chat [--sample NAME \| --scenario UUID \| --session UUID]` | Interactive REPL: plain text → turn, `/`-prefixed → command |
| `scenario create [--file PATH \| --sample NAME]` | Create from JSON or a built-in sample |
| `scenario list / get / delete` | Standard scenario management |
| `session create --scenario <ID>` | Start a new session |
| `session list / get` | Enumerate / inspect sessions |
| `session set-provider <ID> [--provider-id UUID \| --clear]` | Pin a session to a registered provider |
| `turn <SESSION_ID> --input "..." [--mode action\|dialogue\|direct\|remember] [--stream] [--admin]` | Submit a turn |
| `world <SESSION_ID> [--admin]` | Show projected (or raw) world state |
| `provider register --file PATH` | Postgres only: persist a `ProviderConfig` |
| `provider list / remove / models` | Postgres only: enumerate / clean up registry |

Streaming turns render tokens as they arrive and print a final block with `world_state_version`, `changed_entities`, and (when the provider reports it) usage + cost.

`--admin` on `turn` and `world` swaps in an admin `ViewerContext` so GM-only facts surface — same secrecy semantics as the `/admin/sessions/:id/export/raw` HTTP route.

## API Surface

### Core

| Method | Path | Description |
|---|---|---|
| `GET` | `/health` | Service health |
| `GET` | `/readiness` | Readiness check including provider reachability |

### Scenarios

| Method | Path | Description |
|---|---|---|
| `POST` | `/scenarios` | Create a scenario |
| `GET` | `/scenarios` | List scenarios |
| `GET` | `/scenarios/:id` | Get a scenario |
| `DELETE` | `/scenarios/:id` | Delete a scenario |

### Sessions

| Method | Path | Description |
|---|---|---|
| `POST` | `/sessions` | Create a session from a scenario |
| `GET` | `/sessions/:id` | Get a session |
| `DELETE` | `/sessions/:id` | Delete a session |
| `PATCH` | `/sessions/:id/provider` | Assign a registered provider to the session |
| `GET` | `/sessions/:id/world-state` | Get frontend-safe projected state |
| `GET` | `/sessions/:id/export` | Export frontend-visible session state |
| `GET` | `/sessions/:id/events` | Stream session events |

### Turns

| Method | Path | Description |
|---|---|---|
| `POST` | `/sessions/:id/turn` | Submit a blocking player turn |
| `POST` | `/sessions/:id/turn/stream` | Submit a streaming player turn |

### Providers

| Method | Path | Description |
|---|---|---|
| `POST` | `/providers` | Register a provider configuration |
| `GET` | `/providers` | List registered providers |
| `DELETE` | `/providers/:id` | Remove a provider |
| `GET` | `/providers/:id/models` | List models available from a registered provider |
| `GET` | `/providers/health` | Provider configuration health |
| `GET` | `/providers/readiness` | Live provider readiness |

### Admin / Debug

| Method | Path | Description |
|---|---|---|
| `GET` | `/admin/sessions/:id/export/raw` | Full unfiltered world state |
| `POST` | `/admin/sessions/:id/turn/debug` | Turn response including applied delta |

Admin routes expose internal state and must remain protected or disabled outside local/debug use.

## Domain Model Highlights

### Authoritative State

The engine maintains authoritative state such as:

- facts with visibility levels
- NPC runtime state
- factions and standing
- quests
- clocks
- relationships
- inventory
- recent public events

### Typed Mutations

LLM output is constrained to known change types through `WorldStateDelta`, including:

- facts to add
- NPC changes
- faction changes
- quest changes
- clock changes
- relationship changes
- location changes
- inventory changes
- scene and summary updates

The engine validates every proposed mutation before applying it.

### Secrecy and Projection

Normal player-facing projection removes or filters:

- GM-only facts
- hidden clocks
- hidden NPCs
- raw provider output
- internal debug state

Admin projection can include hidden state for debugging.

Recent hardening work also added stricter checks around direct revelation of GM-only facts:

- all matching leaked secrets are checked
- direct reveals require explicit secret references
- direct reveals require reveal proof
- secrets without reveal conditions cannot be directly exposed
- NPC knowledge additions remain strict when reveal metadata is unavailable

## Testing

### Run the normal test suite

```bash
cargo test --workspace
```

### In-memory API tests

```bash
cargo test -p api --test memory_api_flows
```

### Provider tests

```bash
cargo test -p providers
```

### Docker-gated PostgreSQL API tests

```bash
cargo test -p api --test postgres_api_flows -- --ignored
```

### Docker-gated persistence tests

```bash
cargo test -p persistence --test repository_tests -- --ignored
```

### Live local stack smoke test

This is the opt-in integration path that runs the API against:

- PostgreSQL started through Docker or testcontainers
- a real local `llama-server` started by `scripts/start-llm.sh`

It intentionally checks deterministic surfaces only: API health, provider readiness, provider persistence, and model listing. The main turn-flow integration suite stays mock-provider based because real model output is nondeterministic.

Start the local LLM first:

```bash
bash scripts/start-llm.sh gemma4-uncensored
```

Then run the smoke test:

```bash
TEST_LLM_BASE_URL=http://127.0.0.1:8080/v1 \
TEST_LLM_MODEL=local-model \
cargo test -p api --test live_llama_postgres_smoke -- --ignored --test-threads=1
```

Optional overrides:

- `TEST_LLM_PROVIDER_TYPE` defaults to `llama_cpp`
- `TEST_LLM_API_KEY` is used if your local server requires bearer auth

### One-command local LLM test workflow

This wrapper starts one shared PostgreSQL container through `docker compose`, starts the local `llama-server`, runs the normal workspace suite, then runs the ignored Postgres-backed integration suites and the live local-LLM smoke test, and finally shuts the local LLM and shared Postgres back down again.

The script prints explicit phase markers so it is easy to distinguish:

- the normal workspace test pass, where `#[ignore]` tests are intentionally skipped
- the later ignored integration passes, where those Postgres-backed tests are actually executed

The shared Postgres-backed integration phases run with `--test-threads=1` so each test can reset the shared schema safely before it runs.

```bash
bash scripts/test-with-local-llm.sh gemma4-uncensored
```

Useful overrides:

- `HF_TOKEN=...` when the selected model must be fetched from HuggingFace
- `LLAMA_PORT=8081` to move the local LLM off the default port
- `TEST_LLM_MODEL=...` to label the smoke test provider with a specific model name
- `TEST_LLM_PROVIDER_TYPE=llama_cpp` to keep or override the live smoke provider type
- `TEST_DATABASE_URL=postgres://...` to point the shared test phases at a different Postgres database
- `KEEP_TEST_POSTGRES=1` to leave the compose-managed Postgres container running after the script exits

The wrapper fails fast if `llama-server` is missing, Docker is unavailable, the shared Postgres container never becomes healthy, or the requested LLM port is already occupied.

### Run everything

```bash
cargo test --workspace && \
cargo test -p api --test postgres_api_flows -- --ignored && \
cargo test -p persistence --test repository_tests -- --ignored
```

## Current Engineering Priorities

1. Split the non-streaming visible-response path from oracle/delta reasoning so player-visible generation never receives GM-only facts.
2. Continue strengthening reveal validation from proof-presence checks toward stronger proof/condition matching.
3. Keep prompt construction DRY as narration-safe and oracle contexts evolve.
4. Expand integration coverage around turn locking, provider selection, and secret-handling behavior.
5. Keep the engine focused on safety and correctness before adding larger gameplay systems.

## Not in Scope Yet

Do not treat these as current goals until the core engine is further hardened:

- autonomous multi-agent simulation
- vector memory
- realtime multiplayer
- full rules-system automation
- large frontend surface
- complex long-term memory features
- broad gameplay expansion before the turn pipeline is stable

## License

Apache-2.0
