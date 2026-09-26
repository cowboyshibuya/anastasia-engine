#!/usr/bin/env bash
set -euo pipefail

# Run Terminal-Bench through Harbor with anastasia using Opus 4.8.
# Default route is OpenRouter (anthropic/claude-opus-4.8) since native Claude
# OAuth may be unavailable. Override with ANASTASIA_CLI_TB_MODEL / env vars.

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd -- "$SCRIPT_DIR/.." && pwd)
DEFAULT_BINARY_DIR=${ANASTASIA_CLI_HARBOR_BINARY_DIR:-/tmp/anastasia-compat-dist}
DEFAULT_BINARY_PATH=${ANASTASIA_CLI_HARBOR_BINARY:-$DEFAULT_BINARY_DIR/anastasia-linux-x86_64.bin}
DEFAULT_MODEL=${ANASTASIA_CLI_TB_MODEL:-anthropic-api/claude-opus-4-8}
DEFAULT_PATH=${ANASTASIA_CLI_TB_PATH:-/tmp/terminal-bench-2.1}

have_model=0
have_agent_import=0
have_task_source=0

for arg in "$@"; do
  case "$arg" in
    --model|-m)
      have_model=1
      ;;
    --agent-import-path)
      have_agent_import=1
      ;;
    --path|-p|--dataset|-d|--task|-t)
      have_task_source=1
      ;;
  esac
done

if [[ ! -x "$DEFAULT_BINARY_PATH" ]]; then
  echo "Building Linux-compatible anastasia binary into $DEFAULT_BINARY_DIR" >&2
  "$REPO_ROOT/scripts/build_linux_compat.sh" "$DEFAULT_BINARY_DIR"
fi

# Resolve provider keys from anastasia's env files if not already set.
if [[ -z "${OPENROUTER_API_KEY:-}" ]]; then
  OR_ENV=${ANASTASIA_CLI_HARBOR_OPENROUTER_ENV:-$HOME/.config/anastasia/openrouter.env}
  if [[ -f "$OR_ENV" ]]; then
    export ANASTASIA_CLI_HARBOR_OPENROUTER_ENV="$OR_ENV"
  fi
fi
if [[ -z "${ANTHROPIC_API_KEY:-}" ]]; then
  ANT_ENV=${ANASTASIA_CLI_HARBOR_ANTHROPIC_ENV:-$HOME/.config/anastasia/anthropic.env}
  if [[ -f "$ANT_ENV" ]]; then
    export ANASTASIA_CLI_HARBOR_ANTHROPIC_ENV="$ANT_ENV"
  fi
fi

export PYTHONPATH="$REPO_ROOT/scripts${PYTHONPATH:+:$PYTHONPATH}"
export ANASTASIA_CLI_HARBOR_BINARY="$DEFAULT_BINARY_PATH"
export ANASTASIA_CLI_ANTHROPIC_REASONING_EFFORT=${ANASTASIA_CLI_ANTHROPIC_REASONING_EFFORT:-high}
export ANASTASIA_CLI_NO_TELEMETRY=${ANASTASIA_CLI_NO_TELEMETRY:-1}

HARBOR_BIN=${ANASTASIA_CLI_HARBOR_BIN:-harbor}

cmd=($HARBOR_BIN run)
if [[ $have_task_source -eq 0 ]]; then
  cmd+=(--path "$DEFAULT_PATH")
fi
if [[ $have_agent_import -eq 0 ]]; then
  cmd+=(--agent-import-path anastasia_harbor_claude_agent:AnastasiaClaudeHarborAgent)
fi
if [[ $have_model -eq 0 ]]; then
  cmd+=(--model "$DEFAULT_MODEL")
fi
cmd+=("$@")

{
  echo "Running Harbor with anastasia Opus 4.8 adapter"
  echo "  binary: $ANASTASIA_CLI_HARBOR_BINARY"
  echo "  model:  ${DEFAULT_MODEL}"
} >&2

exec "${cmd[@]}"
