# Providers Crate

## Purpose

The `providers` crate isolates LLM integration behind a common `LlmProvider` interface. It owns provider-neutral request and response types, streaming event types, provider health/readiness structures, retry and SSE helpers, secret resolution, and concrete provider implementations.

The rest of the engine should not need to know the HTTP details of OpenAI-compatible APIs, llama.cpp, or OpenRouter.

## What Lives Here

- `provider.rs` defines `LlmProvider`, `LlmRequest`, `LlmResponse`, `ProviderStreamEvent`, usage metadata, model listings, capabilities, health/readiness, and provider errors.
- `openai_compatible.rs` implements generic OpenAI-compatible chat completion calls.
- `llama_cpp.rs` implements local `llama-server` integration, health checks, props checks, and control-token filtering.
- `openrouter.rs` implements OpenRouter-specific routing, attribution headers, model listing, usage, cost, and generation metadata.
- `mock.rs` provides deterministic provider behavior for tests.
- `http.rs` contains shared retry, reqwest error mapping, and buffered SSE decoding.
- `secrets.rs` resolves plain and `env:` API key references.
- `tests/` uses WireMock and unit tests to verify provider behavior.

## Why It Exists

Provider APIs differ in small but important ways: streaming formats, model catalogs, readiness checks, usage reporting, cost reporting, authentication, and retry behavior. This crate contains those differences so the engine can operate on one stable abstraction.

The provider boundary is also important for tests. Most engine and API tests should use `MockProvider` instead of making network calls.

## Engine Context

The engine sends an `LlmRequest` with messages, model settings, and JSON-mode preferences. Providers return either a full `LlmResponse` or a `TokenStream` of `ProviderStreamEvent` values.

Streaming providers emit:

- `Token` events for visible text chunks
- `Metadata` events for usage, cost, model, and generation information when supported

The API and engine collect that metadata without storing per-request stream state on shared provider instances.

## Important Boundaries

- Keep provider-specific JSON payloads and endpoint paths in concrete provider files.
- Keep shared HTTP behavior in `http.rs` only when at least two providers need it.
- Do not let provider implementations mutate domain state.
- Do not persist provider config here; persistence stores records and API composition builds providers from them.
- Secret resolution should fail loudly for unresolved `env:` references so invalid providers are not silently accepted.
- Streaming parsers must tolerate real HTTP chunk boundaries; network chunks are not guaranteed to align with SSE lines.

## Useful Commands

```bash
cargo test -p providers
cargo test -p providers --test wiremock_openai_tests
cargo test -p providers --test wiremock_llama_cpp_tests
cargo test -p providers --test wiremock_openrouter_tests
```
