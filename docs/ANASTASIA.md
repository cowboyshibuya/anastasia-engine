# Anastasia CLI

Anastasia is a focused fork of Anastasia. Its Rust runtime, providers, streaming,
tools, MCP, memory and delegation remain the foundation. See `UPSTREAM.md` and
`LICENSE` for the base revision and attribution.

## Build and install

Run `sh scripts/install-anastasia.sh` on macOS. It builds the optimized binary,
installs an immutable copy, and replaces `~/.local/bin/anastasia` plus its short
alias `~/.local/bin/ana`. The previous launchers are saved beside them as
`anastasia.backup-<version>` and `ana.backup-<version>`. Put `~/.local/bin`
first in PATH. To roll back, replace the launchers with those backups.

Run `anastasia` (or `ana`, the same binary), then `/login` to authenticate and
`/model` to select a model. Use `/help` for commands. `anastasia run 'your
prompt'` runs without the TUI.

## Writing and pasting

Enter sends the draft. Shift+Enter inserts a newline; Ctrl+J does the same.
Paste inserts text into the draft, including blank lines, indentation and code
fences. Large pastes remain editable. Pasting never intentionally sends a prompt.

Some terminals send the same byte for Enter and Shift+Enter. Anastasia requests
enhanced keyboard reporting, but cannot recover a modifier the terminal omits.
Use Ctrl+J there, or `/terminal-setup` for explicit terminal configuration help.
Bracketed paste must be supported by the terminal or terminal multiplexer to
distinguish pasted Enter bytes from typed Enter. No timing heuristic can make
unmarked paste perfectly distinguishable from typing.

## Data and import

CLI state lives in `~/.anastasia-cli`; override it with `ANASTASIA_CLI_HOME`.
The GUI's `~/.anastasia` and Anastasia's `~/.anastasia-cli` remain separate.
For isolated runtime checks also set `ANASTASIA_CLI_RUNTIME_DIR` to a private
temporary directory, or supply `--socket` with a unique path.

## Opening screen

The empty welcome screen shows one of eight original ASCII illustrations, chosen
once per launch, with “Anastasia” beneath it. Resizing and new sessions keep the
same selection. Small terminals and growing multiline drafts fall back to plain
“Anastasia”. The art disappears when the conversation starts; resumed conversations
open directly. It never schedules animation ticks.

## Planning and selectable questions

`/plan [goal]` enters persistent planning mode. The agent explores with built-in
read/search tools, asks unresolved questions, and presents a plan card once the
decisions are clear. Answers and plan cards keep the session in planning mode.
Use `/build` to explicitly enable implementation; this switches mode without
automatically starting a turn.

Planning blocks shell, MCP, delegation and mutation tools at the harness boundary,
including indirect tool calls. Providers that execute tools outside the harness
must use an API route for enforced planning.

Shift+Tab cycles **Build → Plan → Auto**; `/build`, `/plan` and `/auto` do the
same by name. Auto mode tells the agent to explore and plan first on non-trivial
tasks, ask when a decision is missing, implement, then verify with the project's
tests or linters. The status line shows `PLAN` or `AUTO`. Modes change between
turns. Ctrl+M opens the model picker in terminals with the kitty keyboard
protocol; elsewhere Ctrl+M is Enter, so use `/model`.

## Permissions

`/permissions restricted|auto|full` sets when tool calls need your approval:

- `restricted` asks before any tool that is not read-only.
- `auto` (default) asks only for destructive shell commands and for edits outside
  the project.
- `full` never asks.

Approvals use the question panel: allow once, allow that tool for the session, or
deny (the agent is told). Without an interactive client (headless runs,
subagents) a call that needs approval is denied. Set the default with
`[tools] permissions = "full"` or `ANASTASIA_CLI_PERMISSIONS`.

## Token usage

Compaction thresholds use a working-context cap, not the model's full window:
`[compaction] max_budget_tokens = 200000` (default; `0` disables the cap,
`ANASTASIA_CLI_COMPACTION_MAX_TOKENS` overrides). Summaries run on the session
model unless `[compaction] summary_model = "claude-sonnet-5"` is set. To send
fewer tool definitions, use `ANASTASIA_CLI_TOOL_PROFILE=minimal` or
`ANASTASIA_CLI_DISABLED_TOOLS`.

Questions also work during normal coding. Up/Down moves between choices, Space
toggles multi-select choices, Enter confirms and advances, and Tab/Shift+Tab
navigates questions. A custom answer accepts text, Shift+Enter or Ctrl+J for
newlines, and bracketed paste without submission. Review the batch and press
Enter to submit. Escape goes back, or cancels from the first question. Composer
drafts are preserved while the panel is active.

Public harness API v1.1 advertises `user_questions` and `planning_mode`. Clients
opt in with `enable_questions` after attaching, handle `question_request` and
`question_closed`, and send `question_response` or `question_cancel` with the
session and request IDs. `set_planning` changes mode; `planning_state` reports it.
Unsupported clients receive a tool error instead of a question they cannot answer.
Pending questions end on cancellation or disconnect and are not replayed.

Preview an import with `anastasia import --from ~/.anastasia-cli --preview`, then remove
`--preview` to copy. The same command accepts the experimental fork's data
directory. Existing destination files are skipped. Imports include validated
session snapshots and display, provider-selection and compaction preferences.
Credentials, hooks, executable launch settings and removed features are excluded.
Authenticate through `/login` afterward.

## Updates

Upstream binary replacement and telemetry are disabled. Update this checkout
deliberately, then rerun the installer. Do not use upstream Jcode install/release
scripts to install Anastasia. No Anastasia release service is configured.

## Managing saved sessions

In `/resume` and `/session`, Space selects rows. Press `r` to rename the single
selected session, or the hovered row when nothing is selected. Enter saves;
Escape cancels. Pasting a name never saves it.

Delete opens a confirmation for all selected sessions, or the hovered row.
Cancel is selected initially; Left/Right selects Delete, then Enter confirms.
Deletion removes the transcript, journal, and backups. Open sessions must be
closed first, and external CLI transcripts cannot be edited here. Renaming an
open Anastasia session uses its daemon and may require waiting for its turn to
finish.

Use `/` to filter; Tab finishes search input while keeping the filter, allowing
rename and delete on the filtered rows.
