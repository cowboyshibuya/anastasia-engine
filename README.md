# Anastasia Engine and CLI

A shared Rust coding-agent harness and terminal client built on the Rust runtime from [Jcode](https://github.com/1jehuang/jcode). The separate [Anastasia Desktop](https://github.com/cowboyshibuya/anastasia-desktop) app uses this repository's versioned harness API rather than running another agent engine.

- Enter sends; Shift+Enter or Ctrl+J inserts a newline.
- Multiline Markdown and pasted code stay editable in the composer.
- Random static terminal artwork with Anastasia beneath it; no animated startup splash.
- Selectable clarification questions; Shift+Tab cycles Build / Plan / Auto modes.
- `/permissions` approval levels: restricted, auto (default), full.
- Providers, tools, MCP, memory, sessions and delegation retained.
- Separate `~/.anastasia-cli` state; no menu-bar launcher, global hotkeys,
  cloud/mobile pairing, ambient scheduling or upstream auto-updates.

## Install on macOS

```sh
sh scripts/install-anastasia.sh
anastasia
```

Use `/login`, `/model` and `/help` inside the CLI.

See [the user guide](docs/ANASTASIA.md) for imports, keyboard compatibility,
updates and rollback. [UPSTREAM.md](UPSTREAM.md) records the exact foundation.
Anastasia’s MIT attribution is preserved in [LICENSE](LICENSE).
