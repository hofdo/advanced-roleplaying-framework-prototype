# Docker-Backed CI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add CI coverage for ignored Postgres, API, persistence, behavioral, and optional live-stack suites so durable behavior is exercised outside the default local-only test pass.

**Architecture:** Keep fast unit and memory tests in the default workflow, and create a separate Docker-backed workflow that starts Postgres through CI services or `docker compose`. Keep the live local-LLM smoke test separate from deterministic Postgres suites because it depends on `llama-server`.

**Tech Stack:** Rust, Cargo workspace, Axum, SQLx/Postgres, Clap CLI, serde, tokio tests

---

## Current State

- There is no `.github/workflows/` directory.
- `docker-compose.yml` starts a `postgres:16-alpine` service named `postgres` with health checks and the `roleplay` database.
- `scripts/test-with-local-llm.sh` starts Docker Postgres, starts `llama-server`, runs `cargo test --workspace`, then runs ignored suites.
- Ignored suites include:
  - `crates/api/tests/postgres_api_flows.rs`
  - `crates/api/tests/behavioral_fixtures.rs`
  - `crates/persistence/tests/repository_tests.rs`
  - `crates/api/tests/live_llama_postgres_smoke.rs`
- The ignored Postgres tests cover turn persistence, lock behavior, raw export boundaries, provider persistence, behavioral fixtures, and repository behavior.

## Target Behavior

- Pull requests run a fast default Rust workflow.
- A deterministic Docker-backed workflow runs ignored Postgres suites without requiring a live LLM.
- The live local-LLM smoke test remains manual or scheduled because CI runners do not necessarily provide `llama-server` or model files.
- CI failures show which ignored suite failed.
- The README documents how CI maps to local commands.

## File Structure

- Create: `.github/workflows/rust.yml`
  - Run formatting/checking and default tests.
- Create: `.github/workflows/postgres-integration.yml`
  - Run ignored deterministic Postgres/API/persistence/behavioral suites.
- Modify: `README.md`
  - Document CI workflows and the live local-LLM script boundary.
- Optional modify: `scripts/test-with-local-llm.sh`
  - Keep local script behavior aligned with CI command names if needed.

## Tasks

### Task 1: Add Fast Rust Workflow

**Files:**
- Create: `.github/workflows/rust.yml`

- [ ] **Step 1: Create workflow file**

Create this workflow:

```yaml
name: rust

on:
  pull_request:
  push:
    branches: [main]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Format
        run: cargo fmt --all -- --check
      - name: Check
        run: cargo check --workspace
      - name: Test
        run: cargo test --workspace
```

- [ ] **Step 2: Run local syntax check**

Run: `ruby -e "require 'yaml'; YAML.load_file('.github/workflows/rust.yml'); puts 'ok'"`

Expected: prints `ok`.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/rust.yml
git commit -m "ci: add rust workspace workflow"
```

### Task 2: Add Deterministic Postgres Workflow

**Files:**
- Create: `.github/workflows/postgres-integration.yml`

- [ ] **Step 1: Create workflow using a Postgres service**

Create this workflow:

```yaml
name: postgres integration

on:
  pull_request:
  push:
    branches: [main]
  workflow_dispatch:

env:
  TEST_DATABASE_URL: postgres://roleplay:roleplay@127.0.0.1:5432/roleplay

jobs:
  ignored-postgres:
    runs-on: ubuntu-latest
    services:
      postgres:
        image: postgres:16-alpine
        env:
          POSTGRES_DB: roleplay
          POSTGRES_USER: roleplay
          POSTGRES_PASSWORD: roleplay
        ports:
          - 5432:5432
        options: >-
          --health-cmd "pg_isready -U roleplay -d roleplay"
          --health-interval 5s
          --health-timeout 5s
          --health-retries 20
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: API Postgres flows
        run: cargo test -p api --test postgres_api_flows -- --ignored --test-threads=1
      - name: Behavioral fixtures
        run: cargo test -p api --test behavioral_fixtures -- --ignored --test-threads=1
      - name: Persistence repositories
        run: cargo test -p persistence --test repository_tests -- --ignored --test-threads=1
```

- [ ] **Step 2: Run local syntax check**

Run: `ruby -e "require 'yaml'; YAML.load_file('.github/workflows/postgres-integration.yml'); puts 'ok'"`

Expected: prints `ok`.

- [ ] **Step 3: Run one ignored suite locally**

Run: `docker compose up -d postgres`

Run: `TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p persistence --test repository_tests -- --ignored --test-threads=1`

Expected: ignored repository tests pass.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/postgres-integration.yml
git commit -m "ci: run postgres integration suites"
```

### Task 3: Keep Live LLM Smoke Manual

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Document live smoke boundary**

In the testing section, state that `crates/api/tests/live_llama_postgres_smoke.rs` is not part of deterministic PR CI because it requires a local `llama-server` and a model. Point maintainers to:

```bash
bash scripts/test-with-local-llm.sh
```

and to the direct command:

```bash
TEST_LLM_BASE_URL=http://127.0.0.1:8080/v1 TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test live_llama_postgres_smoke -- --ignored --test-threads=1
```

- [ ] **Step 2: Run docs search**

Run: `rg -n "postgres integration|live local-LLM|live_llama_postgres_smoke|scripts/test-with-local-llm.sh" README.md`

Expected: the README has one CI section and one live smoke section.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: explain postgres CI coverage"
```

## Verification

Run:

```bash
ruby -e "require 'yaml'; YAML.load_file('.github/workflows/rust.yml'); YAML.load_file('.github/workflows/postgres-integration.yml'); puts 'ok'"
cargo test --workspace
docker compose up -d postgres
TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test postgres_api_flows -- --ignored --test-threads=1
TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p api --test behavioral_fixtures -- --ignored --test-threads=1
TEST_DATABASE_URL=postgres://roleplay:roleplay@127.0.0.1:5432/roleplay cargo test -p persistence --test repository_tests -- --ignored --test-threads=1
```

## Acceptance Criteria

- `.github/workflows/rust.yml` exists and runs format, check, and default tests.
- `.github/workflows/postgres-integration.yml` exists and runs deterministic ignored Postgres suites.
- Durable Postgres tests run outside the default local-only suite.
- Live LLM smoke tests are documented as manual or scheduled work, not silently omitted.
- `scripts/test-with-local-llm.sh` remains valid for local full-stack verification.

## Risks

- GitHub-hosted runners may have transient Postgres startup delays; health retries should be generous.
- Running all ignored suites on every pull request increases CI time.
- The live smoke test may need a self-hosted runner if maintainers later want it in CI.

