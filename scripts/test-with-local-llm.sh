#!/usr/bin/env bash
set -euo pipefail

MODEL_ALIAS="${1:-gemma4-uncensored}"
LLAMA_PORT="${LLAMA_PORT:-8080}"
TEST_LLM_PROVIDER_TYPE="${TEST_LLM_PROVIDER_TYPE:-llama_cpp}"
TEST_LLM_BASE_URL="${TEST_LLM_BASE_URL:-http://127.0.0.1:${LLAMA_PORT}/v1}"
TEST_LLM_MODEL="${TEST_LLM_MODEL:-local-model}"
TEST_DATABASE_URL="${TEST_DATABASE_URL:-postgres://roleplay:roleplay@127.0.0.1:5432/roleplay}"
KEEP_TEST_POSTGRES="${KEEP_TEST_POSTGRES:-0}"

LLM_PID=""
CURRENT_STEP=""
COMPOSE_POSTGRES_STARTED=0

require_command() {
  local name="$1"
  if ! command -v "$name" >/dev/null 2>&1; then
    echo "Missing required command: $name" >&2
    exit 1
  fi
}

cleanup() {
  local exit_code=$?

  if [ -n "${LLM_PID}" ] && kill -0 "${LLM_PID}" >/dev/null 2>&1; then
    echo "Stopping local llama-server (pid ${LLM_PID})"
    kill "${LLM_PID}" >/dev/null 2>&1 || true
    wait "${LLM_PID}" >/dev/null 2>&1 || true
  fi

  if [ "${COMPOSE_POSTGRES_STARTED}" -eq 1 ] && [ "${KEEP_TEST_POSTGRES}" != "1" ]; then
    echo "Stopping compose postgres"
    docker compose down >/dev/null 2>&1 || true
  fi

  if [ ${exit_code} -ne 0 ]; then
    if [ -n "${CURRENT_STEP}" ]; then
      echo "Test workflow failed during: ${CURRENT_STEP}" >&2
    else
      echo "Test workflow failed." >&2
    fi
  fi

  exit ${exit_code}
}

trap cleanup EXIT INT TERM

require_command bash
require_command cargo
require_command curl
require_command docker
require_command llama-server
require_command lsof

if lsof -iTCP:"${LLAMA_PORT}" -sTCP:LISTEN >/dev/null 2>&1; then
  echo "Port ${LLAMA_PORT} is already in use. Pick another LLAMA_PORT or stop the existing process." >&2
  exit 1
fi

if ! docker info >/dev/null 2>&1; then
  echo "Docker daemon is not available. Start Docker before running this script." >&2
  exit 1
fi

print_phase() {
  local number="$1"
  local total="$2"
  local label="$3"
  echo
  echo "==> Phase ${number}/${total}: ${label}"
}

CURRENT_STEP="docker compose up -d postgres"
print_phase 1 6 "start shared postgres"
docker compose up -d postgres >/dev/null
COMPOSE_POSTGRES_STARTED=1

CURRENT_STEP="waiting for shared postgres"
for _ in $(seq 1 60); do
  if docker compose ps postgres | grep -q "healthy"; then
    break
  fi
  sleep 2
done

if ! docker compose ps postgres | grep -q "healthy"; then
  echo "Compose postgres did not become healthy." >&2
  exit 1
fi

echo "Starting local llama-server on port ${LLAMA_PORT}"
if [ -n "${HF_TOKEN:-}" ]; then
  LLAMA_PORT="${LLAMA_PORT}" HF_TOKEN="${HF_TOKEN}" bash scripts/start-llm.sh "${MODEL_ALIAS}" &
else
  LLAMA_PORT="${LLAMA_PORT}" bash scripts/start-llm.sh "${MODEL_ALIAS}" &
fi
LLM_PID=$!

CURRENT_STEP="waiting for llama health endpoint"
for _ in $(seq 1 180); do
  if curl -fsS "http://127.0.0.1:${LLAMA_PORT}/health" >/dev/null 2>&1; then
    break
  fi
  sleep 2
done

if ! curl -fsS "http://127.0.0.1:${LLAMA_PORT}/health" >/dev/null 2>&1; then
  echo "Local llama-server did not become healthy on port ${LLAMA_PORT}." >&2
  exit 1
fi

CURRENT_STEP="waiting for llama props endpoint"
for _ in $(seq 1 180); do
  if curl -fsS "http://127.0.0.1:${LLAMA_PORT}/props" >/dev/null 2>&1; then
    break
  fi
  sleep 2
done

if ! curl -fsS "http://127.0.0.1:${LLAMA_PORT}/props" >/dev/null 2>&1; then
  echo "Local llama-server did not expose /props on port ${LLAMA_PORT}." >&2
  exit 1
fi

export TEST_LLM_BASE_URL
export TEST_LLM_MODEL
export TEST_LLM_PROVIDER_TYPE
export TEST_DATABASE_URL

echo "Using local LLM base URL: ${TEST_LLM_BASE_URL}"
echo "Using local LLM model: ${TEST_LLM_MODEL}"
echo "Using shared test database: ${TEST_DATABASE_URL}"

CURRENT_STEP="cargo test --workspace"
print_phase 2 6 "workspace tests"
cargo test --workspace

CURRENT_STEP="cargo test -p api --test postgres_api_flows -- --ignored --test-threads=1"
print_phase 3 6 "ignored postgres api flows"
cargo test -p api --test postgres_api_flows -- --ignored --test-threads=1

CURRENT_STEP="cargo test -p api --test behavioral_fixtures -- --ignored --test-threads=1"
print_phase 4 6 "ignored behavioral fixtures"
cargo test -p api --test behavioral_fixtures -- --ignored --test-threads=1

CURRENT_STEP="cargo test -p persistence --test repository_tests -- --ignored --test-threads=1"
print_phase 5 6 "ignored persistence repository tests"
cargo test -p persistence --test repository_tests -- --ignored --test-threads=1

CURRENT_STEP="cargo test -p api --test live_llama_postgres_smoke -- --ignored --test-threads=1"
print_phase 6 6 "live llama postgres smoke"
cargo test -p api --test live_llama_postgres_smoke -- --ignored --test-threads=1

CURRENT_STEP=""
echo "Full test workflow passed."
