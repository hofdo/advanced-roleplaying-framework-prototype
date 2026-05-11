#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# start-llm.sh — Start llama-server with a Gemma-4 model from HuggingFace
#
# Usage:
#   bash scripts/start-llm.sh [gemma4-uncensored|qwen3-uncensored]
#
# Environment:
#   HF_TOKEN   — HuggingFace token (required for private/gated repos)
#   LLAMA_PORT — override default port 8080
# ---------------------------------------------------------------------------
set -euo pipefail

MODEL="${1:-gemma4-uncensored}"
PORT="${LLAMA_PORT:-8080}"

case "$MODEL" in
  gemma4-uncensored)
    HF_REPO="HauhauCS/Gemma-4-E4B-Uncensored-HauhauCS-Aggressive"
    HF_QUANT="Q6_K_P"
    ;;
  qwen3-uncensored)
    HF_REPO="HauhauCS/Qwen3.5-9B-Uncensored-HauhauCS-Aggressive"
    HF_QUANT="Q8_0"
    ;;
  *)
    echo "❌ Unknown model: $MODEL"
    echo ""
    echo "Usage: $0 [gemma4-uncensored|qwen3-uncensored]"
    echo ""
    echo "  gemma4-uncensored  HauhauCS/Gemma-4-E4B-Uncensored-HauhauCS-Aggressive (Q6_K_P)"
    echo "  qwen3-uncensored   HauhauCS/Qwen3.5-9B-Uncensored-HauhauCS-Aggressive (Q8_0)"
    exit 1
    ;;
esac

echo "🦙 llama-server starting"
echo "   Model:  $MODEL"
echo "   Repo:   $HF_REPO"
echo "   Quant:  $HF_QUANT"
echo "   Port:   $PORT"
echo "   GPU:    Metal (-ngl 99, Apple M5)"
echo ""

ARGS="-hf ${HF_REPO}:${HF_QUANT} --port ${PORT} -ngl 99 --jinja"

if [ -n "${HF_TOKEN:-}" ]; then
  ARGS="$ARGS --hf-token ${HF_TOKEN}"
fi

exec llama-server $ARGS

