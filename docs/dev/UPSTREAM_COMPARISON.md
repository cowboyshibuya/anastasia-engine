# Comparing Anastasia with Jcode upstream

Use this workflow to review a Jcode release for performance fixes and useful behavior without turning Anastasia back into an upstream mirror.

## Guardrails

- Stay on the current branch and preserve every existing working-tree change.
- Read `UPSTREAM.md` first. Its base commit records Anastasia's fork point; a release review does not change it.
- Never merge, rebase, or cherry-pick a Jcode release wholesale.
- Do not run Jcode installers, update commands, release scripts, or anything that writes to Anastasia's real home.
- Keep Anastasia names, homes, protocols, product behavior, and fork-specific simplifications unless the task explicitly changes them.
- Preserve upstream copyright, license, and historical attribution.

## Review workflow

1. Fetch the requested tag to a namespaced local ref without checking it out or changing the current branch. Verify the tag and release notes from the official Jcode repository.
2. Compare the release with the base revision in `UPSTREAM.md`. Start with release notes and focused commit/file diffs; raw commit counts can be misleading when upstream history contains merges.
3. Trace each candidate through Anastasia before editing. Classify it as:
   - already present or independently solved;
   - safe to port with a small rename-aware patch;
   - useful only after adapting it to Anastasia's architecture;
   - incompatible, service-specific, GUI-only, or outside Anastasia's direction.
4. Prefer root-cause fixes in shared boundaries. Touch only the implementation files required by the fix and the smallest meaningful regression test. Do not copy adjacent upstream refactors for convenience.
5. Keep a short record of the upstream release and commit hashes used. Do not update the base revision unless Anastasia is intentionally rebased.

A typical read-only comparison starts with:

```sh
tag=v0.86.0
ref="refs/tags/jcode-upstream-$tag"
base=$(awk '/^Base commit:/ { print $3 }' UPSTREAM.md)
git status --short
git fetch --no-tags https://github.com/1jehuang/jcode.git "refs/tags/$tag:$ref"
git diff --stat "$base..$ref"
git log --oneline "$base..$ref" -- path/to/relevant/subsystem
```

Replace the tag and path for the review. Do not checkout the fetched ref.

## Performance comparisons

Use equivalent hardware, terminal dimensions, working directory, connection state, and build class. Record the exact binaries and revisions. Run enough samples to report at least median, minimum, maximum, and completed-run count; seven cold launches is the default for startup checks.

For Anastasia, set a private `ANASTASIA_CLI_HOME` and `ANASTASIA_CLI_RUNTIME_DIR`, or use a unique `--socket`. For Jcode, use the equivalent private Jcode directories. Never share a daemon between samples.

Private CLI directories do not contain every macOS side effect. Before running an upstream binary, inspect whether startup creates apps, menu-bar processes, Login Items, or LaunchAgents. Disable those paths or use an isolated macOS user or VM. Never allow an upstream hotkey listener or updater to remain installed.

Measure the user-visible path rather than only process creation. For startup work, capture first visible output, input-ready time, idle redraws, and daemon memory after a fixed delay. State differences in first-screen behavior, enabled features, credentials, binary size, and caches; do not claim causation from an uncontrolled comparison.

## Port and validation

- Translate owned `jcode` identifiers to `anastasia`; retain genuine external identifiers and attribution.
- Intersect the upstream change with existing Anastasia policies instead of replacing them.
- Run the narrow regression test first, then the relevant crate checks and workspace check.
- For TUI or input changes, run the actual terminal check. For startup changes, repeat the isolated before/after measurement and check for idle background activity.
- Install only with `scripts/install-anastasia.sh`.
- Audit the final diff for unrelated upstream files, dependencies, services, configuration, and documentation changes.

Report what was adopted, already present, adapted, and rejected. Include measured results and their limits. A faster upstream number is evidence to investigate, not a reason to import its architecture.
