#!/usr/bin/env python3
"""Exercise real PTY key decoding and paste against a local, synthetic provider.
Uses pyte, already required by bench_startup_visible_ready.py; no real API keys.
"""
import argparse
import json
import os
import pty
import select
import signal
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pyte
from bench_startup_visible_ready import configure_pty, reply_queries

requests = []

class Provider(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        data = json.dumps({"object": "list", "data": [{"id": "anastasia-test", "object": "model"}]}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        data = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))))
        requests.append(data)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        for chunk in [
            {"id": "local-test", "choices": [{"index": 0, "delta": {"role": "assistant", "content": "Local test response."}, "finish_reason": None}]},
            {"id": "local-test", "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]},
        ]:
            self.wfile.write(("data: " + json.dumps(chunk) + "\n\n").encode())
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()


def run(binary, output):
    server = ThreadingHTTPServer(("127.0.0.1", 0), Provider)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    with tempfile.TemporaryDirectory(prefix="anastasia-pty-", dir="/tmp") as directory:
        root = Path(directory)
        home, runtime = root / "home", root / "run"
        home.mkdir(mode=0o700)
        runtime.mkdir(mode=0o700)
        (home / "config.toml").write_text('[features]\nauto_poke = false\ncheck_updates = false\n[display]\nidle_animation = true\n')
        cmd, response = root / "cmd", root / "response"
        env = {**os.environ, "TERM": "xterm-256color", "COLORTERM": "truecolor", "ANASTASIA_CLI_GLYPH_SAFE_MODE": "off", "ANASTASIA_CLI_NO_TELEMETRY": "1",
               "ANASTASIA_CLI_HOME": str(home), "ANASTASIA_CLI_RUNTIME_DIR": str(runtime),
               "ANASTASIA_CLI_DEBUG_CMD_PATH": str(cmd), "ANASTASIA_CLI_DEBUG_RESPONSE_PATH": str(response),
               "ANASTASIA_CLI_OPENAI_COMPAT_API_BASE": f"http://127.0.0.1:{server.server_port}/v1",
               "ANASTASIA_CLI_OPENAI_COMPAT_DEFAULT_MODEL": "anastasia-test", "OPENAI_COMPAT_API_KEY": "local-test-only"}
        env.pop("NO_COLOR", None)
        master, slave = pty.openpty()
        configure_pty(slave, 36, 100)
        screen = pyte.Screen(100, 36)
        stream = pyte.Stream(screen)
        proc = subprocess.Popen([binary, "--provider", "openai-compatible", "--model", "anastasia-test", "--no-update", "--no-selfdev"],
                                stdin=slave, stdout=slave, stderr=slave, env=env, cwd=root, start_new_session=True)
        os.close(slave)
        query_buffer = b""
        def pump(duration=0.1):
            nonlocal query_buffer
            end = time.monotonic() + duration
            while time.monotonic() < end:
                if select.select([master], [], [], 0.02)[0]:
                    try:
                        chunk = os.read(master, 65536)
                    except OSError:
                        raise AssertionError("CLI exited: " + "\n".join(screen.display))
                    stream.feed(chunk.decode("utf-8", "replace"))
                    query_buffer = reply_queries(master, query_buffer + chunk)
        def state():
            response.unlink(missing_ok=True)
            cmd.write_text("state")
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                pump()
                if response.exists():
                    try:
                        return json.loads(response.read_text())
                    except json.JSONDecodeError:
                        continue
            raise AssertionError("No debug response: " + "\n".join(screen.display))
        try:
            initial = state()
            pump(0.5)
            os.write(master, b"\x1b")  # Skip first-run onboarding in the private test home.
            pump(0.2)
            os.write(master, b"\x15")
            pump(0.1)
            opening = "\n".join(screen.display)
            assert "Anastasia" in opening, opening
            art_end = next(i for i, line in enumerate(screen.display) if line.strip() == "Anastasia")
            art_block = screen.display[:art_end + 1]
            configure_pty(master, 24, 20)
            screen.resize(24, 20)
            pump(0.3)
            narrow = "\n".join(screen.display)
            assert screen.display[0].strip() == "Anastasia", narrow
            configure_pty(master, 36, 100)
            screen.resize(36, 100)
            pump(0.3)
            assert screen.display[:art_end + 1] == art_block, "Resizing changed the launch art"
            baseline = len(requests)
            os.write(master, b"first\x1b[13;2usecond\nthird")
            pump(0.2)
            current = state()
            assert current["input"] == "first\nsecond\nthird", current
            paste = "\n\n```rust\r\n    let café = 1;\r\n```\r\n"
            os.write(master, b"\x1b[200~" + paste.encode() + b"\x1b[201~")
            pump(0.2)
            current = state()
            expected = "first\nsecond\nthird" + paste.replace("\r\n", "\n")
            assert current["input"] == expected, current
            assert len(requests) == baseline, "A newline or paste sent a provider request"
            assert current["queued_messages"] == 0, current
            draft = "\n".join(screen.display)
            assert screen.display[0].strip() == "Anastasia", "Multiline input did not collapse the art"
            os.write(master, b"\r")
            deadline = time.monotonic() + 15
            while len(requests) == baseline and time.monotonic() < deadline:
                pump()
            assert len(requests) > baseline, "Plain Enter did not send"
            pump(1)
            assert not any(line.strip() == "Anastasia" for line in screen.display), "Art remained after first prompt"
            deadline = time.monotonic() + 15
            while state()["processing"] and time.monotonic() < deadline:
                pump()
            snapshots = list((home / "sessions").glob("*.json"))
            assert snapshots, "No saved session to resume"
            session_id = json.loads(snapshots[0].read_text())["id"]
            proc.kill()
            proc.wait(timeout=5)
            os.close(master)
            master, slave = pty.openpty()
            configure_pty(slave, 36, 100)
            screen.reset()
            query_buffer = b""
            proc = subprocess.Popen([binary, "--resume", session_id, "--provider", "openai-compatible", "--model", "anastasia-test", "--no-update", "--no-selfdev"], stdin=slave, stdout=slave, stderr=slave, env=env, cwd=root, start_new_session=True)
            os.close(slave)
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                if state()["display_messages"] >= 2:
                    break
            pump(0.3)
            resumed = "\n".join(screen.display)
            assert "Local test response" in resumed, resumed
            assert not any(line.strip() == "Anastasia" for line in screen.display), "Resumed conversation showed launch art"
            # Exercise session management against only private fixture transcripts.
            output.mkdir(parents=True, exist_ok=True)
            source = json.loads(snapshots[0].read_text())
            fixtures = []
            for suffix, title in [("alpha", "PickerAlpha"), ("beta", "PickerBeta")]:
                fixture = {**source, "id": "picker_edit_" + suffix, "short_name": suffix, "title": title,
                           "custom_title": None, "parent_id": None, "last_pid": None, "status": "Closed",
                           "is_debug": False, "is_canary": False}
                path = home / "sessions" / (fixture["id"] + ".json")
                path.write_text(json.dumps(fixture))
                fixtures.append(path)
            def until(predicate):
                deadline = time.monotonic() + 20
                while time.monotonic() < deadline:
                    pump()
                    if predicate():
                        return
                raise AssertionError("Timed out: " + "\n".join(screen.display))
            os.write(master, b"\x15/resume\r")
            until(lambda: "PickerAlpha" in "\n".join(screen.display) and "PickerBeta" in "\n".join(screen.display))
            os.write(master, b"/PickerAlpha\tr\x15cancelled name\x1b")
            pump(0.3)
            assert json.loads(fixtures[0].read_text()).get("custom_title") is None
            os.write(master, b"r\x15\x1b[200~Renamed caf\xc3\xa9\x1b[201~")
            pump(0.2)
            assert json.loads(fixtures[0].read_text()).get("custom_title") is None, "Paste saved a rename"
            assert any(cell.fg == "6ed2ff" for row in screen.buffer.values() for cell in row.values()), "Rename modal lacks cyan accent"
            (output / "session-rename.txt").write_text("\n".join(screen.display))
            os.write(master, b"\r")
            until(lambda: json.loads(fixtures[0].read_text()).get("custom_title") == "Renamed café")
            until(lambda: "Rename session" not in "\n".join(screen.display) and "Saving changes" not in "\n".join(screen.display))
            assert json.loads(fixtures[0].read_text())["messages"] == source["messages"], "Rename changed history"
            os.write(master, b"\x1b")
            pump(0.1)
            os.write(master, b"/picker_edit\t \x1b[B \x1b[3~")
            until(lambda: "Permanently delete 2 session(s)?" in "\n".join(screen.display))
            assert any(cell.bg == "6ed2ff" for row in screen.buffer.values() for cell in row.values()), "Default Cancel is not highlighted"
            (output / "session-delete-confirm.txt").write_text("\n".join(screen.display))
            os.write(master, b"\r")
            pump(0.2)
            assert all(path.exists() for path in fixtures), "Default Cancel deleted sessions"
            os.write(master, b"\x1b[3~\x1b[C")
            pump(0.2)
            assert any(cell.bg == "dc7878" for row in screen.buffer.values() for cell in row.values()), "Delete focus lacks danger highlight"
            os.write(master, b"\r")
            until(lambda: all(not path.exists() for path in fixtures))
            until(lambda: "Saving changes" not in "\n".join(screen.display))
            os.write(master, b"\x1b")
            pump(0.1)
            os.write(master, b"/" + session_id.encode() + b"\t")
            pump(0.3)
            # Debug builds may hide their own live session until the debug filter is enabled.
            if "No sessions" in "\n".join(screen.display):
                os.write(master, b"d")
                pump(0.2)
            os.write(master, b"\x1b[3~")
            until(lambda: "Open sessions cannot be deleted" in "\n".join(screen.display))
            assert snapshots[0].exists(), "Current session was deleted"
            os.write(master, b"\x1b")
            pump(0.1)
            os.write(master, b"r\x15Live rename\r")
            until(lambda: json.loads(snapshots[0].read_text()).get("custom_title") == "Live rename")
            until(lambda: "Saving changes" not in "\n".join(screen.display))
            prior_requests = len(requests)
            os.write(master, b"\x1b")
            pump(0.1)
            os.write(master, b"\x1b")
            pump(0.1)
            os.write(master, b"after live rename\r")
            until(lambda: len(requests) > prior_requests)
            pump(0.5)
            assert "Local test response" in "\n".join(screen.display), "Live rename disrupted the original connection"
            (output / "session-edit-result.json").write_text(json.dumps({"rename": True, "rename_cancel": True, "paste_never_saves": True, "history_preserved": True, "multi_delete": True, "delete_cancel": True, "active_delete_blocked": True, "live_rename": True, "connection_preserved": True}, indent=2))
            (output / "terminal-opening.txt").write_text(opening)
            (output / "terminal-narrow.txt").write_text(narrow)
            (output / "terminal-resumed.txt").write_text(resumed)
            (output / "terminal-draft.txt").write_text(draft)
            (output / "terminal-result.json").write_text(json.dumps({"narrow_banner": True, "stable_art": True, "composer_collapse": True, "first_prompt_hides_art": True, "resume_hides_art": True, "shift_enter": True, "ctrl_j": True, "multiline_paste": True, "enter_sends": True, "provider": "local synthetic HTTP server"}, indent=2))
            print("PASS: stable launch art, narrow fallback, multiline drafts, first-prompt dismissal, resume, session rename/delete, Shift+Enter, Ctrl+J, paste and Enter through the real PTY")
        finally:
            subprocess.run([binary, "server", "stop", "--force"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
            try:
                proc.kill()
                proc.wait(timeout=5)
            except (ProcessLookupError, PermissionError, subprocess.TimeoutExpired):
                pass
            os.close(master)
            server.shutdown()

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("--output", type=Path, default=Path("verification"))
    args = parser.parse_args()
    run(str(Path(args.binary).resolve()), args.output)
