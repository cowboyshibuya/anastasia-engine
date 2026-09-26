# Auth Notes: OAuth + API-key Providers

This document explains how authentication works in J-Code.

## Overview

J-Code can detect existing local credentials and can also run built-in OAuth and API-key login flows.

For auth files managed by other tools/CLIs, anastasia asks before reading them. If you
approve a source, anastasia remembers that approval for that external auth file path
for future sessions and still leaves the original file untouched (no move,
rewrite, or permission mutation). Symlinked external auth files are rejected.

Credentials are stored locally:
- J-Code Claude OAuth (if logged in via `anastasia login --provider claude`): `~/.anastasia-cli/auth.json`
- Claude Code CLI: `~/.claude/.credentials.json` (Linux/Windows), or the **macOS login Keychain** item `Claude Code-credentials` (the default on macOS, where the JSON file usually does not exist), or the `CLAUDE_CODE_OAUTH_TOKEN` env var (set by `claude setup-token`)
- OpenCode (optional provider/OAuth import source): `~/.local/share/opencode/auth.json`
- pi (optional provider/OAuth import source): `~/.pi/agent/auth.json`
- J-Code OpenAI/Codex OAuth: `~/.anastasia-cli/openai-auth.json`
- Codex CLI auth source (read in place only after confirmation): `~/.codex/auth.json`
- Gemini native OAuth: `~/.anastasia-cli/gemini_oauth.json`
- Gemini CLI import fallback: `~/.gemini/oauth_creds.json`
- Copilot CLI plaintext fallback: `~/.copilot/config.json`
- Legacy Copilot JSON sources: `~/.config/github-copilot/hosts.json`, `~/.config/github-copilot/apps.json`

Relevant code:
- Claude provider: `src/provider/claude.rs`
- OpenAI login + refresh: `src/auth/oauth.rs`
- OpenAI credentials parsing: `src/auth/codex.rs`
- OpenAI requests: `src/provider/openai.rs`
- Azure OpenAI auth/config: `src/auth/azure.rs`
- Azure OpenAI transport: `src/provider/openrouter.rs`
- Gemini login + refresh: `src/auth/gemini.rs`
- Gemini Code Assist provider: `src/provider/gemini.rs`
- OpenAI-compatible provider metadata/login descriptors: `crates/anastasia-provider-metadata/src/lib.rs`

## Claude (Claude Max)

### Login steps
1. Run `anastasia login --provider claude` (recommended), or `anastasia login` and choose Claude.
   - For headless / SSH use: `anastasia login --provider claude --no-browser`
   - For scriptable remote flows: `anastasia login --provider claude --print-auth-url`, then later complete with `--callback-url` or `--auth-code`
2. Alternative: run `claude` (or `claude setup-token`). anastasia can detect Claude Code's credentials, ask before reading them, and remember that approval for future sessions. This works whether Claude Code stored them in `~/.claude/.credentials.json` (Linux/Windows), the macOS login Keychain (`Claude Code-credentials`), or the `CLAUDE_CODE_OAUTH_TOKEN` env var. On macOS, approving the Keychain source copies the credentials into `~/.anastasia-cli/auth.json` once so later sessions never re-prompt the Keychain.
3. Verify with `anastasia --provider claude run "Say hello from anastasia"`.

Credential discovery order is:
1. `~/.anastasia-cli/auth.json`
2. `~/.claude/.credentials.json`
3. Claude Code native credentials (macOS Keychain `Claude Code-credentials`, or `CLAUDE_CODE_OAUTH_TOKEN` env var) once approved
4. `~/.local/share/opencode/auth.json`
5. `~/.pi/agent/auth.json`

### Direct Anthropic API (default)
`--provider claude` uses the direct Anthropic Messages API by default.
anastasia owns the full runtime path itself: auth, refresh, request shaping, tool
compatibility, and transport.

#### Claude OAuth direct API compatibility
Claude Code OAuth tokens can be used directly against the Messages API, but only
if the request matches the Claude Code "OAuth contract". anastasia applies this
automatically for the default Claude runtime path.

Required behaviors (applied by the Anthropic provider):
- Use the Messages endpoint with `?beta=true`.
- Send `User-Agent: claude-cli/1.0.0`.
- Send `anthropic-beta: oauth-2025-04-20,claude-code-20250219`.
- Prepend the system blocks with the Claude Code identity line as the first
  block:
  - `You are Claude Code, Anthropic's official CLI for Claude.`

Tool name allow-list:
Claude OAuth requests reject certain tool names. anastasia remaps a small set of
builtin tool names on the wire to the Claude-Code builtin names and maps them
back on responses so native tools continue to work. Every other tool is
forwarded under its own name, so the full custom toolset (websearch, webfetch,
browser, codesearch, memory, swarm, multiedit, open, ...) stays available on
OAuth. The remapped names are:
- `bash` → `Bash`
- `read` → `Read`
- `write` → `Write`
- `edit` → `Edit`
- `glob` → `Glob`
- `grep` → `Grep`
- `subagent` → `Agent`
- `schedule` → `ScheduleWakeup`
- `skill_manage` → `Skill`

Notes:
- If the OAuth token expires, refresh via the Claude OAuth refresh endpoint.
- Without the identity line and allow-listed tool names, the API will reject
  OAuth requests even if the token is otherwise valid.

### Deprecated Claude CLI transport
The old Claude CLI shell-out path is deprecated and should only be used for
legacy compatibility.

You can still force it temporarily with:
- `ANASTASIA_USE_CLAUDE_CLI=1`
- or `--provider claude-subprocess` (deprecated hidden compatibility value)

These environment variables control the deprecated Claude Code CLI transport:
- `ANASTASIA_CLAUDE_CLI_PATH` (default: `claude`)
- `ANASTASIA_CLAUDE_CLI_MODEL` (default: `claude-opus-4-5-20251101`)
- `ANASTASIA_CLAUDE_CLI_PERMISSION_MODE` (default: `bypassPermissions`)
- `ANASTASIA_CLAUDE_CLI_PARTIAL` (set to `0` to disable partial streaming)

## OpenAI / Codex OAuth

### Login steps
1. Run `anastasia login --provider openai`.
   - For headless / SSH use: `anastasia login --provider openai --no-browser`
   - For scriptable remote flows: `anastasia login --provider openai --print-auth-url`, then later complete with `--callback-url`
2. Your browser opens to the OpenAI OAuth page unless you use `--no-browser`. The local callback listens on
   `http://localhost:1455/auth/callback` by default.
   If port `1455` is unavailable, anastasia falls back to a manual paste flow where
   you can paste the full callback URL or query string.
3. After login, tokens are saved to `~/.anastasia-cli/openai-auth.json`.

Credential discovery order is:
1. `~/.anastasia-cli/openai-auth.json`
2. `~/.codex/auth.json`
3. trusted OpenCode/pi OAuth in `~/.local/share/opencode/auth.json` / `~/.pi/agent/auth.json`
4. `OPENAI_API_KEY`

If anastasia finds existing credentials in `~/.codex/auth.json`, it asks before
reading them. When approved, it remembers that trust decision for future anastasia
sessions and still does not move, delete, or rewrite the Codex file.

### Request details
J-Code uses the Responses API. If you have a ChatGPT subscription (refresh
token or id_token present), requests go to:
- `https://chatgpt.com/backend-api/codex/responses`
with headers:
- `originator: codex_cli_rs`
- `chatgpt-account-id: <from token>`

Otherwise it uses:
- `https://api.openai.com/v1/responses`

For **API-key** usage (no ChatGPT/Codex OAuth), the Responses API base URL is
overridable so you can target a local or proxied Responses-API endpoint. Set one
of (checked in this order) to an absolute `http(s)://` base that ends in the API
version, e.g. `http://127.0.0.1:8317/v1`:
- `ANASTASIA_OPENAI_API_BASE`
- `OPENAI_BASE_URL`
- `OPENAI_API_BASE`

anastasia appends `/responses` itself, derives the WebSocket and `/compact`
endpoints from the same base, and also points the `/models` catalog probe at it.
The override is ignored in ChatGPT/Codex OAuth mode (that backend is fixed), and
a malformed value is logged and ignored rather than breaking requests.

### Troubleshooting
- Claude 401/auth errors: run `anastasia login --provider claude`.
- 401/403: re-run `anastasia login --provider openai`.
- Callback issues: make sure port 1455 is free and the browser can reach
  `http://localhost:1455/auth/callback`.

## Azure OpenAI

This was added after comparing J-Code to OpenCode/Crush. The meaningful auth gap
was not another browser OAuth flow, but support for **Azure OpenAI** using either:
- **Microsoft Entra ID** credentials (via Azure's `DefaultAzureCredential` chain), or
- **Azure OpenAI API keys**.

### Login/setup steps
1. Run `anastasia login --provider azure`.
2. Enter your Azure OpenAI endpoint, for example:
   - `https://your-resource.openai.azure.com`
3. Enter your Azure deployment/model name.
4. Choose one auth mode:
   - **Entra ID** (recommended)
   - **API key**
5. anastasia saves settings to `~/.config/anastasia/azure-openai.env`.

### Stored configuration
The Azure env file may contain:
- `AZURE_OPENAI_ENDPOINT`
- `AZURE_OPENAI_MODEL`
- `AZURE_OPENAI_USE_ENTRA`
- `AZURE_OPENAI_API_KEY` (only when using key auth)

### Runtime behavior
- anastasia normalizes the endpoint to the newer Azure OpenAI `/openai/v1` base.
- In **Entra ID** mode, anastasia obtains bearer tokens using `azure_identity::DefaultAzureCredential` with scope:
  - `https://cognitiveservices.azure.com/.default`
- In **API key** mode, anastasia sends the credential in the Azure-style `api-key` header.
- The Azure provider currently reuses J-Code's OpenAI-compatible transport layer under the hood.
- Model catalog fetching is disabled for Azure by default, so you should configure a deployment/model explicitly.

### Entra ID credential sources
`DefaultAzureCredential` can resolve credentials from sources like:
- `az login`
- managed identity
- Azure environment credentials

### Troubleshooting
- If Entra ID auth fails locally, try `az login` first.
- Make sure your identity has access to the Azure OpenAI resource.
- If requests fail with deployment/model errors, verify `AZURE_OPENAI_MODEL` matches your deployed model name.
- If you prefer static credentials, re-run `anastasia login --provider azure` and choose API key mode.

## Gemini OAuth

### Login steps
1. Run `anastasia login --provider gemini` or `/login gemini` inside the TUI.
   - For headless / SSH use: `anastasia login --provider gemini --no-browser`
   - For scriptable remote flows: `anastasia login --provider gemini --print-auth-url`, then later complete with `--auth-code`
2. anastasia opens a browser to the Google OAuth flow used for Gemini Code Assist unless you use `--no-browser`.
3. If local callback binding is unavailable, anastasia falls back to a manual paste flow using `https://codeassist.google.com/authcode`.
4. Tokens are saved to `~/.anastasia-cli/gemini_oauth.json`.

### Credential discovery order
1. Native anastasia Gemini tokens: `~/.anastasia-cli/gemini_oauth.json`
2. Gemini CLI OAuth source (read only after approval): `~/.gemini/oauth_creds.json`
3. trusted OpenCode/pi OAuth in `~/.local/share/opencode/auth.json` / `~/.pi/agent/auth.json`

### Runtime notes
- anastasia uses native Google OAuth and talks to the Google Code Assist backend directly.
- Expired tokens are refreshed automatically using the Google refresh token.
- Some school / Workspace accounts may require `GOOGLE_CLOUD_PROJECT` or `GOOGLE_CLOUD_PROJECT_ID` for Code Assist entitlement checks.

### Troubleshooting
- If browser launch fails, use `--no-browser` and the pasted callback/code flow.
- If entitlement or onboarding fails for a Workspace account, set `GOOGLE_CLOUD_PROJECT` and retry.
- If login succeeds but requests fail later, re-run `anastasia login --provider gemini` to refresh the stored session.

### Auth verification
Use the built-in auth verifier to test the full local auth/runtime path after login:

```bash
# Run Gemini login now, then verify token refresh + provider smoke
anastasia --provider gemini auth-test --login

# Verify existing Gemini auth without re-running login
anastasia --provider gemini auth-test

# Check every currently configured supported auth provider
anastasia auth-test --all-configured
```

For model providers, `auth-test` attempts:
1. credential discovery
2. refresh/auth probe
3. a real provider smoke prompt expecting `AUTH_TEST_OK`
4. a tool-enabled smoke prompt using the same tool-attached request path as normal chat

Use `--no-tool-smoke` if you only want the auth/simple-runtime checks.

For Gmail/Google it verifies credential discovery and token refresh, but skips model smoke because it is not a model provider.

## OpenAI-compatible API-key providers

J-Code also ships first-class provider presets for many OpenAI-compatible APIs.
These providers use the same built-in login flow pattern: `anastasia login --provider <name>`.

For arbitrary OpenAI-compatible APIs, especially when an agent is doing setup, prefer the named profile command instead of hand-editing config:

```bash
printf '%s' "$MY_API_KEY" | anastasia provider add my-api \
  --base-url https://llm.example.com/v1 \
  --model my-model-id \
  --api-key-stdin \
  --set-default \
  --json

anastasia --provider-profile my-api auth-test --no-tool-smoke
```

This writes `[providers.my-api]` in `~/.anastasia-cli/config.toml` and stores the key in anastasia's private app config dir, for example `~/.config/anastasia/provider-my-api.env`. For localhost servers, use `--no-api-key`.

Notable presets include:

### Fireworks
- Login: `anastasia login --provider fireworks`
- Stored env file: `~/.config/anastasia/fireworks.env`
- API key env var: `FIREWORKS_API_KEY`
- Base URL: `https://api.fireworks.ai/inference/v1`
- Default model hint: `accounts/fireworks/routers/kimi-k2p5-turbo`
- Docs: <https://docs.fireworks.ai/tools-sdks/openai-compatibility>

### Novita AI
- Login: `anastasia login --provider novita` or `/login novita` in the TUI
- Authentication: pay-as-you-go API key, not a subscription login or browser OAuth
- Stored env file: `~/.config/anastasia/novita.env`
- API key env var: `NOVITA_API_KEY`
- Base URL: `https://api.novita.ai/openai`
- Default model hint: `zai-org/glm-5.3`
- Get a key: <https://novita.ai/settings/key-management>
- Docs: <https://novita.ai/docs/guides/llm-api>

### MiniMax
- Login: `anastasia login --provider minimax`
- Stored env file: `~/.config/anastasia/minimax.env`
- API key env var: `OPENAI_API_KEY`
- Base URL: `https://api.minimax.io/v1`
- Default model hint: `MiniMax-M2.7`
- Docs: <https://platform.minimax.io/docs/guides/text-generation>

These are first-class anastasia provider presets, not just manual custom endpoint examples.
You can still use `openai-compatible` for arbitrary custom providers when there is not a built-in preset.

If anastasia finds matching API keys in trusted OpenCode/pi auth files, it can reuse them for the corresponding provider preset without asking you to paste the key again.

## Experimental CLI Providers

J-Code also supports experimental CLI-backed providers, plus Antigravity with native OAuth login:
- `--provider cursor`
- `--provider copilot`
- `--provider antigravity`

Cursor uses anastasia's native HTTPS transport. Copilot uses GitHub device-flow auth. Antigravity login/auth storage is handled natively by anastasia.

### Cursor
- Login: `anastasia login --provider cursor`
  - saves `CURSOR_API_KEY` to `~/.config/anastasia/cursor.env`
- Runtime:
  - anastasia uses native HTTPS requests
  - if a Cursor API key is configured, anastasia exchanges/uses it directly
- Env vars:
  - `ANASTASIA_CURSOR_MODEL` (default: `composer-1.5`)
  - `CURSOR_API_KEY` (optional; overrides saved key)

### GitHub Copilot
- Login: `anastasia login --provider copilot`
  - Headless / SSH: `anastasia login --provider copilot --no-browser`
  - Scriptable remote flow: `anastasia login --provider copilot --print-auth-url`, then later `anastasia login --provider copilot --complete`
  - anastasia uses GitHub device code flow and can print the verification URL/QR without opening a local browser.
- Credential discovery order:
  1. `COPILOT_GITHUB_TOKEN`
  2. `GH_TOKEN`
  3. `GITHUB_TOKEN`
  4. trusted `~/.copilot/config.json`
  5. trusted legacy `~/.config/github-copilot/hosts.json`
  6. trusted legacy `~/.config/github-copilot/apps.json`
  7. trusted OpenCode/pi OAuth entries
  8. `gh auth token`
- Env vars:
  - `ANASTASIA_COPILOT_CLI_PATH` (optional override for CLI path)
  - `ANASTASIA_COPILOT_MODEL` (default: `claude-sonnet-4`)

### Antigravity
- Login: `anastasia login --provider antigravity` (native Google OAuth flow; does **not** require Antigravity to be installed)
  - Headless / SSH: `anastasia login --provider antigravity --no-browser`
  - Scriptable remote flow: `anastasia login --provider antigravity --print-auth-url`, then later complete with `--callback-url`
- Tokens: `~/.anastasia-cli/antigravity_oauth.json`
- Credential discovery order:
  1. native anastasia tokens at `~/.anastasia-cli/antigravity_oauth.json`
  2. trusted OpenCode/pi OAuth entries when present
- Runtime:
  - anastasia authenticates directly and stores/refreshes Antigravity OAuth tokens itself
  - the provider transport still shells out to the Antigravity CLI for completions if you choose `--provider antigravity`
- Env vars:
  - `ANASTASIA_ANTIGRAVITY_CLIENT_ID` (optional override for OAuth client id)
  - `ANASTASIA_ANTIGRAVITY_CLIENT_SECRET` (optional override for OAuth client secret)
  - `ANASTASIA_ANTIGRAVITY_VERSION` (optional override for Antigravity request fingerprint/version)
  - `ANASTASIA_ANTIGRAVITY_CLI_PATH` (default: `antigravity`, runtime only)
  - `ANASTASIA_ANTIGRAVITY_MODEL` (default: `default`)
  - `ANASTASIA_ANTIGRAVITY_PROMPT_FLAG` (default: `-p`)
  - `ANASTASIA_ANTIGRAVITY_MODEL_FLAG` (default: `--model`)

## Google / Gmail OAuth

### Login steps
1. Run `anastasia login --provider google`.
   - For headless / SSH use: `anastasia login --provider google --no-browser`
   - For scriptable remote flows after credentials are already configured: `anastasia login --provider google --print-auth-url`
2. If Google credentials are not configured yet, anastasia first walks you through saving your client ID/client secret or importing the JSON credentials file.
3. For scriptable Google flows, choose the Gmail scope with `--google-access-tier full|readonly` if you do not want the default full access tier.
4. Complete the printed flow later with `anastasia login --provider google --callback-url '<full callback url or query>'`.

### Notes
- Google/Gmail scriptable auth requires saved OAuth client credentials first.
- The callback URL can come from a remote browser session that fails on the loopback redirect. Copy the final URL from the address bar and paste or pass it back to anastasia.

## Scriptable auth state lifecycle

- anastasia stores temporary scriptable login state in `~/.anastasia-cli/pending-login/*.json`
- pending state expires automatically
- stale pending entries are cleaned up when scriptable login flows start or resume
- Copilot `--print-auth-url` stores the GitHub device code session and `--complete` resumes polling later
# Google OAuth client credentials

Public source builds do not embed Gemini or Antigravity OAuth client IDs or
secrets. To use these login flows, provide `GEMINI_CLIENT_ID` and
`GEMINI_CLIENT_SECRET`, or `ANASTASIA_CLI_ANTIGRAVITY_CLIENT_ID` and
`ANASTASIA_CLI_ANTIGRAVITY_CLIENT_SECRET`, respectively. Missing credentials
produce a local error before opening the browser.
