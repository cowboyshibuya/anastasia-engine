#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd -- "$SCRIPT_DIR/.." && pwd)
DEFAULT_BINARY_DIR=${ANASTASIA_CLI_HARBOR_BINARY_DIR:-/tmp/anastasia-compat-dist}
DEFAULT_BINARY_PATH=${ANASTASIA_CLI_HARBOR_BINARY:-$DEFAULT_BINARY_DIR/anastasia-linux-x86_64}
DEFAULT_MODEL=${ANASTASIA_CLI_TB_MODEL:-openai/gpt-5.4}
DEFAULT_PATH=${ANASTASIA_CLI_TB_PATH:-/tmp/terminal-bench-2}

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

OPENAI_AUTH=${ANASTASIA_CLI_HARBOR_OPENAI_AUTH:-$HOME/.anastasia-cli/openai-auth.json}
if [[ ! -f "$OPENAI_AUTH" ]]; then
  echo "OpenAI OAuth file not found at $OPENAI_AUTH" >&2
  exit 1
fi

export PYTHONPATH="$REPO_ROOT/scripts${PYTHONPATH:+:$PYTHONPATH}"
export ANASTASIA_CLI_HARBOR_BINARY="$DEFAULT_BINARY_PATH"
export ANASTASIA_CLI_HARBOR_OPENAI_AUTH="$OPENAI_AUTH"
export ANASTASIA_CLI_OPENAI_REASONING_EFFORT=${ANASTASIA_CLI_OPENAI_REASONING_EFFORT:-high}
export ANASTASIA_CLI_OPENAI_SERVICE_TIER=${ANASTASIA_CLI_OPENAI_SERVICE_TIER:-priority}
export ANASTASIA_CLI_NO_TELEMETRY=${ANASTASIA_CLI_NO_TELEMETRY:-1}

HARBOR_BIN=${ANASTASIA_CLI_HARBOR_BIN:-}
if [[ -z "$HARBOR_BIN" ]]; then
  CACHED_HARBOR="$HOME/.cache/uv/archive-v0/qtLT-I4hA5Q9ne5Zq-5cn/bin/harbor"
  if [[ -x "$CACHED_HARBOR" ]]; then
    HARBOR_BIN="$CACHED_HARBOR"
  else
    HARBOR_BIN="uvx --offline harbor"
  fi
fi

cmd=($HARBOR_BIN run)
if [[ $have_task_source -eq 0 ]]; then
  cmd+=(--path "$DEFAULT_PATH")
fi
if [[ $have_agent_import -eq 0 ]]; then
  cmd+=(--agent-import-path anastasia_harbor_agent:AnastasiaHarborAgent)
fi
if [[ $have_model -eq 0 ]]; then
  cmd+=(--model "$DEFAULT_MODEL")
fi
cmd+=("$@")

{
  echo "Running Harbor with anastasia adapter"
  echo "  binary: $ANASTASIA_CLI_HARBOR_BINARY"
  echo "  auth:   $ANASTASIA_CLI_HARBOR_OPENAI_AUTH"
  echo "  model:  ${DEFAULT_MODEL}"
} >&2

exec "${cmd[@]}"
