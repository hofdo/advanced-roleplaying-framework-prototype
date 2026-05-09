# Release Log

---

## [Unreleased] — secrecy-hardening

### Features
- **Narration-safe streaming prompt** — `build_streaming_prompt` now calls `render_narration_context` (GM-only facts omitted). `build_non_streaming_prompt` and `build_delta_extraction_prompt` retain the full oracle context (GM-only facts included) since both paths require delta-quality LLM reasoning. Streaming is the only path where the LLM emits player-visible text without simultaneously being asked for a delta, making it the correct — and only required — hardening target.
- **Semantic secret reveal bypass** — `BasicDeltaValidator` now uses `find_leaked_gm_only_facts` (plural) instead of a blunt boolean. A `PlayerKnown` fact whose text matches one or more GM-only fact texts is allowed through if and only if all three conditions hold for every matched secret: (a) the secret's ID appears in `related_secret_ids`, (b) `reveal_condition_satisfied` is non-empty, (c) the GM-only fact itself declares at least one `reveal_conditions` entry. This prevents both unauthorized leaks and accidental bypass of secrets that were never intended to be revealable. The `FactSource::PlayerCorrection` carve-out is removed — source does not override `GmOnly` visibility.

### Tests
- `streaming_prompt_excludes_gm_only_facts` — proves streaming user message contains no GM-only section or text.
- `non_streaming_prompt_includes_gm_only_facts` — regression: non-streaming prompt retains GM-only oracle context.
- `delta_extraction_prompt_includes_gm_only_facts` — regression: delta extraction prompt retains GM-only oracle context.
- `player_known_fact_revealing_gm_only_text_with_explicit_id_and_proof_passes` — all three bypass conditions met → delta accepted.
- `player_known_fact_direct_leak_without_id_ref_is_rejected` — text match, no ID reference → `SecretLeak`.
- `player_known_fact_direct_leak_with_id_ref_but_no_proof_is_rejected` — ID referenced but `reveal_condition_satisfied` absent → `SecretLeak`.
- `player_known_fact_leaking_two_secrets_referencing_only_one_is_rejected` — text leaks two secrets; only one ID referenced → `SecretLeak`.
- `player_known_fact_revealing_gm_only_with_no_reveal_conditions_on_secret_is_rejected` — secret has empty `reveal_conditions`; cannot be bypassed even with ID + proof → `SecretLeak`.
- `admin_projection_includes_gm_only_facts` — admin `ViewerContext` sees all facts; player context sees only `PlayerKnown`.

---

## [Unreleased] — post-mvp/enhancements

### Features
- **Provider Registry** — `AppState` now holds a live `HashMap<Uuid, Arc<dyn LlmProvider>>` populated from the database at startup. Turn handlers resolve `session.provider_id` against this registry and fall back to the config-file default provider. `POST /providers` and `DELETE /providers` update the registry atomically at runtime.
- **ProviderConfigRepository** — Provider configurations are now persisted in the `provider_configs` database table. Added `POST /providers` (register), `DELETE /providers/:id` (remove), and `GET /providers` (list from DB). In-memory store backed by `Vec<ProviderRecord>` for tests.
- **Debug Turn Endpoint** — New `POST /admin/sessions/:session_id/turn/debug` returns `DebugTurnResponseBody` which includes all normal turn response fields plus `applied_delta: WorldStateDelta` — the raw delta the LLM returned after validation and reduction. Backed by `DefaultTurnPipeline::process_turn_debug()`.
- **TurnMode prompt shaping** — `TurnMode` (Dialogue / Action / Direct / Remember) now actively shapes the LLM prompt. `Direct` overrides the scene to `RulesAdjudication` and prepends an out-of-character GM preamble. `Remember` overrides to `WorldSimulation` and asks the LLM to acknowledge and confirm a fact correction. `Action` and `Dialogue` keep their natural classified scene.

### Tests
- **wiremock provider tests** — Five HTTP-level tests against a mock OpenAI-compatible server: HTTP 500 → `ProviderError::Status`, HTTP 429 → `ProviderError::RateLimit`, retry-on-500-then-200 succeeds, malformed JSON body → `ProviderError::MalformedResponse`, missing `choices` field → `ProviderError::MalformedResponse`. No Docker required.
- **Behavioral integration tests** (Docker-gated) — Unknown NPC entity ID in delta → 422 Unprocessable Entity. Dead NPC attitude change rejected → 422. Empty provider queue → 502 Bad Gateway. Debug turn returns `applied_delta` in response body.
- **Behavioral fixture test** (Docker-gated) — Full pipeline scenario: player floods the guildhall. Asserts faction standing drops by 10, fame clock advances from 1 to 3, a new player-known fact is created, the GM-only `void-mark` secret does not appear in player projection via either `/export` or `/world-state`.

---

## [0.1.0] — hardening/engine

### Features
- **PostgreSQL turn locking** — `PostgresSessionTurnLock` uses a boolean flag (`processing_turn`) with stale recovery (5-minute timeout on `processing_turn_started_at`). Multi-instance safe. `InMemorySessionTurnLock` retained for tests and local dev.
- **PreparedTurn / FinalizedTurn pipeline split** — Extracted `prepare_turn_context()` and `finalize_with_parsed_delta()` from the turn handler. Both the streaming and non-streaming paths converge at `finalize_with_parsed_delta()` — validation, reduction, and projection logic live in exactly one place.
- **Safe export / player projection** — `GET /sessions/:id/export` returns `FrontendVisibleState` via `ViewerContext::player()`, hiding GM-only facts. Added `GET /admin/sessions/:id/export/raw` returning full `WorldState` for admin use.
- **Session-scoped provider selection** — `SessionRecord.provider_id` persisted in DB. `PATCH /sessions/:id/provider` updates the field. Turn handlers resolve provider from session record (registry lookup introduced in post-MVP phase).
- **Provider health vs readiness split** — `LlmProvider` trait now has two methods: `health()` checks config only (no network), `readiness()` makes a lightweight HTTP GET. Exposed as `GET /providers/health` and `GET /providers/readiness`.
- **Provider retry policy** — `OpenAiCompatibleProvider` retries on transport errors, timeouts, HTTP 429 (rate limit), and HTTP 5xx with exponential backoff capped at 2 s. Non-retryable errors (`MalformedResponse`, `StreamingUnsupported`) fail immediately.
- **JSON repair retry on malformed LLM output** — If the LLM returns delta JSON with structural issues, `repair_prompt()` attempts to fix common truncation and trailing-comma problems before a second parse attempt. Parse failure after repair emits an error event and returns 400.
- **NpcChange::KnowledgeAdded fix** — Reducer now correctly creates a new `Fact` entity in `world_state.facts` (with `FactSource::Turn`) and pushes its `EntityKey` to `npc.known_facts`. Previously the string was pushed directly to `notes`, which was a type mismatch and wrong field.
- **Secret reveal validation** — `FactToAdd` accepts `related_secret_ids` and `reveal_condition_satisfied`. If a `PlayerKnown` fact references secrets but `reveal_condition_satisfied` is absent, the delta is rejected with `DeltaValidationError::MissingRevealProof` → 422.
- **NPC status action restrictions** — `Unconscious` and `Dead` NPCs cannot receive `KnowledgeAdded` or `AttitudeChanged` changes. `Dead` NPCs cannot receive `LocationChanged`. `StatusChanged` is always allowed (needed for resurrection/revival flows). Violation → 422.
- **NpcState.visible_to_player projection** — Added `visible_to_player: bool` (defaults `true`) to `NpcState`. `FrontendStateProjector` filters NPCs by this flag rather than by status, giving the scenario explicit control over visibility.
- **FactToAdd secret fields** — Added `related_secret_ids: Vec<EntityKey>` and `reveal_condition_satisfied: Option<String>` to both `FactToAdd` (delta input) and stored `Fact`, with `#[serde(default)]` for backwards compatibility.
- **License alignment** — Workspace `Cargo.toml` license field set to `Apache-2.0` matching the `LICENSE` file.

### Tests
- `export_projection_strips_gm_only_facts` — unit test, no Docker.
- `turn_response_and_export_do_not_leak_raw_provider_output` — Docker-gated integration test proving `raw_provider_output` is never present in turn responses or the export payload.

---

## [0.0.1] — Initial Implementation

- Rust workspace with crates: `api`, `domain`, `engine`, `persistence`, `providers`, `shared`.
- Axum HTTP API: scenarios, sessions, turns (blocking + SSE streaming), world-state, events, export.
- PostgreSQL persistence via `sqlx` with migration runner.
- `DefaultTurnPipeline`: lock → load → classify → context → prompt → LLM → parse → validate → reduce → project → persist.
- `WorldStateDelta` typed mutation system (no generic PATCH on world state).
- `FrontendStateProjector` with `ViewerContext` for safe state projection.
- `OpenAiCompatibleProvider` (OpenAI chat completions format).
- `MockProvider` for in-process testing.
- Docker Compose for local PostgreSQL.
