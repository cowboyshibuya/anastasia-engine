# Harness failure-mode review (2026-09-23)

This reviews the externally supplied failure list against Anastasia CLI's current code. "Covered" means a concrete guard exists, not that every provider and crash interleaving has been exercised. The fixes below were made on the current branch without changing the GUI.

## Confirmed gaps fixed here

| Failure mode | Change |
| --- | --- |
| Huge directory listing wedges a worker | `tool/ls.rs` now refuses directories above 4,096 immediate entries before sorting or recursing; the user gets an actionable error. |
| Cut stream is reported as success | The shared MPSC turn loop now rejects EOF without a completion event. Direct Anthropic SSE additionally requires no open tool input or trailing partial SSE data, and returns an error to its retry path. |
| Compaction accepts empty context | Blank text summaries now fail before compacted history advances. A valid native encrypted artifact remains allowed without text. |
| Malformed permission level silently weakens policy | Unknown configured permission levels now fall back to `restricted`, rather than `auto`. |
| Model fallback misreads a dotted version | The existing regression test exposed `Opus 4.8` being cut at the decimal point and incorrectly selecting 4.5. Sentence splitting now preserves version dots. |

## Existing guards found

- Missing tool results after an interrupted call: `agent.rs::repair_missing_tool_outputs` inserts an error result for orphaned calls, but skips calls still in flight. Reload-aborted tools also get explicit interrupted results in `agent/turn_streaming_mpsc.rs`.
- Child death and parent waiting forever: `server/swarm.rs` detects terminal worker states, reclaims assignments, and fails them after a bounded reclaim count. Background tasks orphaned by server death are marked failed at startup in `server.rs`.
- Retry budget of one: direct Anthropic and OpenAI streams use three attempts; Claude CLI uses five. This does not establish that every provider has appropriate retry behavior.
- Blank or oversized compaction payloads: the compaction path has stale-result and encrypted-payload size guards; this review added a blank-summary guard.
- Unknown model mapped to an 8K context: `anastasia-provider-core/src/models.rs` uses a 128K fallback, not 8K. A guessed context can still be wrong for an unknown model.
- Cached-token billing: the TUI cost calculation distinguishes Anthropic split accounting from OpenAI-style subset accounting in `tui/app/misc_ui.rs`. The session metric now uses the existing shared context-token heuristic rather than adding cache reads to OpenAI's inclusive input count again.
- Oversized API frames: the harness API bridge caps frames at 16 MiB and has framing tests.

## Open risks; do not call these solved

1. **Cross-provider stream finality and retry.** The shared MPSC turn loop and Anthropic direct SSE path now reject incomplete streams. Other provider adapters and the alternate turn loop still need fault-injection tests for EOF after partial tool arguments, reconnects, and repeated output.
2. **Durable turn admission and exactly-once effects.** A persisted user message and reload intent support recovery, but this is not a durable, idempotent turn queue. Crash/reconnect, poison-message redelivery, duplicate approvals, stale stop writes, and a worker rebooting before tool registration need a common turn identity and fault-injection tests before making exactly-once claims.
3. **Concurrent lifecycle.** Parent/child lock ordering, out-of-band death while awaiting, two runtimes claiming one endpoint, registration replay, and approval-hook deduplication need deterministic concurrency tests. Existing swarm reclaim and cancellation code cover only parts of this.
4. **Timeout semantics.** Direct Anthropic streams have a chunk idle timeout, but their keepalive pings produce activity events. A separate content-progress deadline is needed to detect a stream that sends only pings. Other upstream clients need a transport-by-transport timeout audit.
5. **Usage, context, and compaction pressure.** File-read double counting, repeated cache reads, undersized context guesses, retained small tool results, oversized hook results, and high compaction counts need measured per-provider traces. Avoid changing accounting heuristics from anecdotes alone.
6. **Large-state and trace pressure.** Session-log replay under locks, wake-payload truncation, synchronous persistence per event, polling frequency, trace volume, and cache eviction cost need bounded-load measurements. The CLI's removed production telemetry delivery does not by itself prove these internal paths cheap.

The supplied reports about dependency-edge installation limits and registration tokens may describe a different extension runtime. No equivalent 64-edge dependency limit or dual registration-token owner was found in this CLI; revisit only if its deployment path acquires those constraints. The tool-schema prose ratio, 41% token overhead, and 48% root status polls are measurements from another harness and cannot be attributed to Anastasia without local measurement.
