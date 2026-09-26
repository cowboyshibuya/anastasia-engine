# Anastasia CLI development

This repository is the single owner of the agent harness and CLI. The desktop
client lives in `cowboyshibuya/anastasia-desktop` and consumes the versioned
`anastasia-harness-api`; do not copy provider drivers, session persistence, or
tool policy into the desktop repository. Add desktop-needed operations to the
curated API with translation and capability tests.

- Work on the current branch. Preserve upstream attribution and the base revision in UPSTREAM.md.
- Keep the Rust runtime and existing performance optimizations; simplify product surfaces instead of replacing the engine.
- CLI data is `~/.anastasia-cli` (`ANASTASIA_CLI_HOME`); do not modify the GUI or experimental Anastasia data.
- Test builds against a private `ANASTASIA_CLI_RUNTIME_DIR` or unique `--socket`; never measure an old shared daemon by accident.
- Install through `scripts/install-anastasia.sh`, which preserves the previous launcher. Do not run upstream release/install scripts for this fork.
- Composer contract: Enter sends; Shift+Enter and Ctrl+J insert newlines; bracketed paste never submits. Keep all connection states consistent.
- Run relevant existing tests and an actual terminal check for input/render changes. Avoid network work or filesystem reads in rendering.

## Comparing Jcode upstream

- Follow `docs/dev/UPSTREAM_COMPARISON.md` for release and performance reviews.
- Treat upstream as evidence, not as a patch queue: do not merge, rebase, or cherry-pick a release wholesale. Port the smallest verified change that fits Anastasia's architecture and product direction.
- Preserve the current branch and dirty working tree. Never reset, clean, rename Anastasia code back to Jcode, change `UPSTREAM.md`'s base revision, or run Jcode install/update commands during a comparison.
- Benchmark both CLIs with private homes, private runtime directories, and unique sockets. On macOS, private CLI directories do not isolate LaunchAgents or app installation; inspect or isolate upstream startup before running its binary.
