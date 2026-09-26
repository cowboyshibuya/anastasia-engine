# Terminal-Bench 2.0 with anastasia

This document describes the cleanest currently-working path for running anastasia on Terminal-Bench 2.0 through Harbor.

## What is in the repo

- `scripts/anastasia_harbor_agent.py`
  - Harbor custom agent adapter for anastasia
- `scripts/run_terminal_bench_harbor.sh`
  - helper that wires Harbor to the adapter and a Linux-compatible anastasia binary
- `scripts/run_terminal_bench_campaign.py`
  - sequential campaign runner that preserves small batches in a stitchable layout
- `scripts/build_linux_compat.sh`
  - builds a Linux anastasia artifact against an older glibc baseline for TB-style containers

## Why the compat binary matters

Many Terminal-Bench task containers use an older glibc than a locally-built host binary. The Harbor adapter should use a Linux binary produced by:

```bash
scripts/build_linux_compat.sh /tmp/anastasia-compat-dist
```

The helper script will build it for you automatically if it is missing.

## Auth and model assumptions

The current adapter is designed for:

- OpenAI OAuth auth file at `~/.anastasia-cli/openai-auth.json`
- `gpt-5.4`
- high reasoning effort
- priority service tier

Those defaults can be overridden with environment variables.

## Sequential campaign mode

If you want to run only a few tasks at a time but keep a coherent artifact set, use the campaign runner.

Example:

```bash
python scripts/run_terminal_bench_campaign.py \
  --campaign-dir ~/tb2-anastasia-campaign \
  --task regex-log \
  --task largest-eigenval \
  --task cancel-async-tasks
```

What it does:

- runs tasks sequentially with `--n-concurrent 1`
- preserves Harbor jobs under `campaign-dir/harbor-jobs/`
- writes a pinned `campaign.json`
- refuses to mix runs if key settings drift
- appends per-task outcomes to `results.jsonl`

This is the recommended path when you want to batch tasks gradually and stitch them together later.

## Quick start

Assuming Terminal-Bench is already available at `/tmp/terminal-bench-2`:

```bash
scripts/run_terminal_bench_harbor.sh \
  --include-task-name regex-log \
  --n-tasks 1 \
  --n-concurrent 1 \
  --jobs-dir /tmp/anastasia-tb2 \
  --job-name regex-log-pilot \
  --yes
```

Or point Harbor directly at the remote dataset:

```bash
scripts/run_terminal_bench_harbor.sh \
  --dataset terminal-bench@2.0 \
  --include-task-name regex-log \
  --n-tasks 1 \
  --n-concurrent 1 \
  --jobs-dir /tmp/anastasia-tb2 \
  --job-name regex-log-pilot \
  --yes
```

## Useful environment variables

- `ANASTASIA_HARBOR_BINARY`
  - path to the Linux-compatible anastasia binary to upload into the task container
- `ANASTASIA_HARBOR_BINARY_DIR`
  - output directory used when auto-building the compat binary
- `ANASTASIA_HARBOR_OPENAI_AUTH`
  - path to the OpenAI OAuth file
- `ANASTASIA_HARBOR_CA_BUNDLE`
  - optional host CA bundle path to upload into the task container
- `ANASTASIA_TB_MODEL`
  - Harbor model string, default `openai/gpt-5.4`
- `ANASTASIA_TB_PATH`
  - default local Terminal-Bench path, default `/tmp/terminal-bench-2`
- `ANASTASIA_OPENAI_REASONING_EFFORT`
  - default `high`
- `ANASTASIA_OPENAI_SERVICE_TIER`
  - default `priority`

## Notes on fairness and state isolation

The adapter gives each trial a fresh in-container anastasia home directory under `/tmp/anastasia-home`, so memories and auth state are isolated per trial container.

## Current validation status

This path has already been validated with real Harbor task runs using:

- `regex-log`
- `largest-eigenval`
- `cancel-async-tasks`

All three passed in-container with verifier reward `1.0` during the initial pilot.
