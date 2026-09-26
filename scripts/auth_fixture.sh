#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
command=${1:-help}
if [[ $# -gt 0 ]]; then
  shift
fi

sandbox_name=${ANASTASIA_CLI_ONBOARDING_SANDBOX:-default}
sandbox_root_default="$repo_root/.tmp/onboarding/$sandbox_name"
sandbox_root=${ANASTASIA_CLI_ONBOARDING_DIR:-$sandbox_root_default}
anastasia_home="$sandbox_root/home"
runtime_dir="$sandbox_root/runtime"
fixture_root_default="$repo_root/.tmp/auth-fixtures"
fixture_root=${ANASTASIA_CLI_AUTH_FIXTURE_DIR:-$fixture_root_default}

usage() {
  cat <<EOF
Usage: $(basename "$0") <command> [args...]

Commands:
  list                         List saved local auth fixtures
  path [name]                  Print fixture root, or a specific fixture path
  save <name>                  Save the current sandbox ANASTASIA_CLI_HOME as a fixture
  load <name>                  Replace the sandbox ANASTASIA_CLI_HOME with a saved fixture
  reset-sandbox                Remove only the current sandbox ANASTASIA_CLI_HOME
  delete <name>                Delete a saved fixture
  env <name>                   Print exports for running against a loaded fixture
  run <name> -- <args...>      Load fixture, then run anastasia with args in sandbox
  help                         Show this help

Environment overrides:
  ANASTASIA_CLI_ONBOARDING_SANDBOX     Sandbox name to load into/from (default: default)
  ANASTASIA_CLI_ONBOARDING_DIR         Explicit onboarding sandbox directory
  ANASTASIA_CLI_AUTH_FIXTURE_DIR       Fixture store (default: .tmp/auth-fixtures)

Notes:
  Fixtures are local developer state under .tmp by default. They may contain real
  tokens copied from sandbox logins, so do not move them into tracked paths.
EOF
}

require_name() {
  if [[ $# -lt 1 || -z "${1:-}" ]]; then
    echo "missing fixture name" >&2
    exit 2
  fi
  if [[ "$1" == *"/"* || "$1" == *".."* || "$1" =~ [^A-Za-z0-9._-] ]]; then
    echo "invalid fixture name: $1" >&2
    echo "Use only letters, numbers, dot, underscore, and dash." >&2
    exit 2
  fi
  printf '%s' "$1"
}

fixture_path() {
  local name
  name=$(require_name "$1")
  printf '%s/%s/home' "$fixture_root" "$name"
}

metadata_path() {
  local name
  name=$(require_name "$1")
  printf '%s/%s/metadata.txt' "$fixture_root" "$name"
}

ensure_parent_dirs() {
  mkdir -p "$fixture_root" "$sandbox_root" "$runtime_dir"
  chmod 700 "$fixture_root" "$sandbox_root" 2>/dev/null || true
}

copy_dir_contents() {
  local src=$1
  local dst=$2
  rm -rf "$dst"
  mkdir -p "$dst"
  if [[ -d "$src" ]]; then
    shopt -s dotglob nullglob
    local entries=("$src"/*)
    if [[ ${#entries[@]} -gt 0 ]]; then
      cp -a "${entries[@]}" "$dst"/
    fi
    shopt -u dotglob nullglob
  fi
}

run_anastasia() {
  local binary_path="$repo_root/target/debug/anastasia"
  (
    cd "$repo_root"
    if [[ -x "$binary_path" ]]; then
      env ANASTASIA_CLI_HOME="$anastasia_home" ANASTASIA_CLI_RUNTIME_DIR="$runtime_dir" "$binary_path" "$@"
    else
      env ANASTASIA_CLI_HOME="$anastasia_home" ANASTASIA_CLI_RUNTIME_DIR="$runtime_dir" cargo run --bin anastasia -- "$@"
    fi
  )
}

list_fixtures() {
  ensure_parent_dirs
  if [[ ! -d "$fixture_root" ]]; then
    return 0
  fi
  find "$fixture_root" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort
}

save_fixture() {
  local name=$1
  local dst meta
  dst=$(fixture_path "$name")
  meta=$(metadata_path "$name")
  ensure_parent_dirs
  if [[ ! -d "$anastasia_home" ]]; then
    echo "sandbox ANASTASIA_CLI_HOME does not exist: $anastasia_home" >&2
    exit 1
  fi
  mkdir -p "$(dirname "$dst")"
  copy_dir_contents "$anastasia_home" "$dst"
  chmod -R go-rwx "$(dirname "$dst")" 2>/dev/null || true
  cat > "$meta" <<EOF
name=$name
saved_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
sandbox_name=$sandbox_name
source_anastasia_home=$anastasia_home
warning=May contain real local auth tokens. Do not commit or share.
EOF
  echo "Saved auth fixture '$name' from $anastasia_home"
  echo "Fixture path: $(dirname "$dst")"
}

load_fixture() {
  local name=$1
  local src
  src=$(fixture_path "$name")
  ensure_parent_dirs
  if [[ ! -d "$src" ]]; then
    echo "fixture does not exist: $name" >&2
    echo "Expected: $src" >&2
    exit 1
  fi
  copy_dir_contents "$src" "$anastasia_home"
  chmod -R go-rwx "$anastasia_home" 2>/dev/null || true
  echo "Loaded auth fixture '$name' into sandbox '$sandbox_name'"
  echo "ANASTASIA_CLI_HOME=$anastasia_home"
}

delete_fixture() {
  local name=$1
  local fixture_dir="$fixture_root/$name"
  if [[ ! -d "$fixture_dir" ]]; then
    echo "fixture does not exist: $name" >&2
    exit 1
  fi
  rm -rf "$fixture_dir"
  echo "Deleted auth fixture '$name'"
}

case "$command" in
  list)
    list_fixtures
    ;;
  path)
    ensure_parent_dirs
    if [[ $# -gt 0 ]]; then
      name=$(require_name "$1")
      dirname "$(fixture_path "$name")"
    else
      printf '%s\n' "$fixture_root"
    fi
    ;;
  save)
    name=$(require_name "${1:-}")
    save_fixture "$name"
    ;;
  load)
    name=$(require_name "${1:-}")
    load_fixture "$name"
    ;;
  reset-sandbox)
    rm -rf "$anastasia_home"
    mkdir -p "$anastasia_home" "$runtime_dir"
    echo "Reset sandbox ANASTASIA_CLI_HOME: $anastasia_home"
    ;;
  delete)
    name=$(require_name "${1:-}")
    delete_fixture "$name"
    ;;
  env)
    name=$(require_name "${1:-}")
    load_fixture "$name" >/dev/null
    cat <<EOF
export ANASTASIA_CLI_HOME="$anastasia_home"
export ANASTASIA_CLI_RUNTIME_DIR="$runtime_dir"
EOF
    ;;
  run)
    name=$(require_name "${1:-}")
    shift || true
    if [[ "${1:-}" == "--" ]]; then
      shift
    fi
    load_fixture "$name" >/dev/null
    run_anastasia "$@"
    ;;
  help|-h|--help)
    usage
    ;;
  *)
    echo "Unknown command: $command" >&2
    echo >&2
    usage >&2
    exit 1
    ;;
esac
