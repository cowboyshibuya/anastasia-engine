#!/usr/bin/env python3
"""Check selectable questions and enforced planning through a real PTY.
Uses the existing pyte terminal-check dependency and a local synthetic provider.
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
root = None

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
        messages = data.get("messages", [])
        latest = next((m["content"] for m in reversed(messages) if m["role"] == "user"), "")
        if not isinstance(latest, str):
            latest = json.dumps(latest)
        results = {m.get("tool_call_id"): m.get("content", "") for m in messages if m["role"] == "tool"}
        tool = None
        if "run build test" in latest:
            if "allowed-write" not in results:
                tool = ("allowed-write", "write", {"file_path": str(root / "build-marker.txt"), "content": "built", "intent": "Verify build mode"})
        elif "cancel questions" in latest:
            if "cancel-test" not in results:
                tool = ("cancel-test", "request_user_input", {"intent": "Check cancellation", "questions": [{"id": "cancel", "header": "Cancel", "question": "Cancel this question", "options": [{"label": "Continue", "description": "Proceed"}], "multi_select": False}]})
        elif "PTY plan interview" in latest:
            if "question-test" not in results:
                time.sleep(2)  # Leave time to type a composer draft before the panel appears.
                tool = ("question-test", "request_user_input", {"intent": "Clarify the plan", "questions": [
                    {"id": "surfaces", "header": "Surfaces", "question": "Which interfaces?", "options": [{"label": "CLI", "description": "Terminal interface"}, {"label": "GUI", "description": "Desktop interface"}], "multi_select": True},
                    {"id": "details", "header": "Details", "question": "Describe your requirements", "options": [], "multi_select": False}]})
            elif "blocked-write" not in results:
                tool = ("blocked-write", "write", {"file_path": str(root / "forbidden-marker.txt"), "content": "must not exist", "intent": "Try an edit during planning"})
        delta = {"role": "assistant", "content": "```plan\n# PTY verified plan\nGoal: improve the CLI\nApproach: use confirmed answers\nValidation: terminal check\n```"}
        finish = "stop"
        if tool:
            call, name, args = tool
            delta = {"role": "assistant", "tool_calls": [{"index": 0, "id": call, "type": "function", "function": {"name": name, "arguments": json.dumps(args)}}]}
            finish = "tool_calls"
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        for event in [{"id": "local", "choices": [{"index": 0, "delta": delta, "finish_reason": None}]}, {"id": "local", "choices": [{"index": 0, "delta": {}, "finish_reason": finish}]}]:
            self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()


def run(binary, output):
    global root
    requests.clear()
    server = ThreadingHTTPServer(("127.0.0.1", 0), Provider)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    with tempfile.TemporaryDirectory(prefix="ana-q-", dir="/tmp") as directory:
        root = Path(directory)
        home, runtime = root / "home", root / "run"
        home.mkdir(mode=0o700)
        runtime.mkdir(mode=0o700)
        (home / "config.toml").write_text('[features]\nauto_poke = false\ncheck_updates = false\n')
        cmd, response = root / "cmd", root / "response"
        env = {**os.environ, "TERM": "xterm-256color", "COLORTERM": "truecolor", "ANASTASIA_CLI_NO_TELEMETRY": "1",
               "ANASTASIA_CLI_HOME": str(home), "ANASTASIA_CLI_RUNTIME_DIR": str(runtime),
               "ANASTASIA_CLI_DEBUG_CMD_PATH": str(cmd), "ANASTASIA_CLI_DEBUG_RESPONSE_PATH": str(response),
               "ANASTASIA_CLI_OPENAI_COMPAT_API_BASE": f"http://127.0.0.1:{server.server_port}/v1",
               "ANASTASIA_CLI_OPENAI_COMPAT_DEFAULT_MODEL": "anastasia-test", "OPENAI_COMPAT_API_KEY": "local-test-only"}
        master, slave = pty.openpty()
        configure_pty(slave, 36, 100)
        screen = pyte.Screen(100, 36)
        stream = pyte.Stream(screen)
        proc = subprocess.Popen([binary, "--provider", "openai-compatible", "--model", "anastasia-test", "--no-update", "--no-selfdev"], stdin=slave, stdout=slave, stderr=slave, env=env, cwd=root, start_new_session=True)
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
                        pass
            raise AssertionError("No debug state: " + "\n".join(screen.display))
        def wait(predicate):
            end = time.monotonic() + 20
            while time.monotonic() < end:
                current = state()
                if predicate(current):
                    return current
            raise AssertionError("Timed out: " + json.dumps(current) + "\n" + "\n".join(screen.display))
        def snapshot(name):
            output.mkdir(parents=True, exist_ok=True)
            (output / name).write_text("\n".join(screen.display))
        try:
            state()
            pump(0.5)
            os.write(master, b"\x1b")  # Skip first-run onboarding in the private test home.
            pump(0.2)
            os.write(master, b"\x15")
            pump(0.1)
            snapshot("questions-opening.txt")
            os.write(master, b"/plan PTY plan interview\r")
            wait(lambda s: s["planning"] and s["processing"])
            os.write(master, b"saved draft")
            wait(lambda s: s["question_pending"])
            assert state()["input"] == "saved draft", state()
            snapshot("questions-select.txt")
            offered = {t["function"]["name"] for t in requests[0]["tools"]}
            assert "request_user_input" in offered and "write" not in offered and "bash" not in offered, offered
            os.write(master, b" \x1b[B \r")
            pump(0.2)
            configure_pty(master, 24, 60)
            screen.resize(24, 60)
            os.write(master, b"\x1b[200~caf\xc3\xa9\nsecond\x1b[201~")
            os.write(master, b"\x1b[13;2uthird\nfourth")
            pump(0.3)
            assert len(requests) == 1, "Paste/newlines submitted a question"
            os.write(master, b"\r")
            pump(0.2)
            snapshot("questions-review.txt")
            assert len(requests) == 1, "Enter bypassed review"
            os.write(master, b"\r")
            current = wait(lambda s: not s["processing"] and not s["question_pending"])
            assert current["planning"] and current["input"] == "saved draft", current
            assert not (root / "forbidden-marker.txt").exists(), "Plan mode allowed an edit"
            results = {m.get("tool_call_id"): m.get("content", "") for r in requests for m in r.get("messages", []) if m["role"] == "tool"}
            raw_answer = results["question-test"]
            answer = json.loads(raw_answer[raw_answer.index("{"):])["answers"]
            assert answer == {"surfaces": ["CLI", "GUI"], "details": ["café\nsecond\nthird\nfourth"]}, answer
            assert "planning mode" in results["blocked-write"], results
            os.write(master, b"\x15/build\r")
            wait(lambda s: not s["planning"])
            os.write(master, b"run build test\r")
            wait(lambda s: (root / "build-marker.txt").exists() and not s["processing"])
            os.write(master, b"cancel questions\r")
            wait(lambda s: s["question_pending"])
            os.write(master, b"\x1b")
            wait(lambda s: not s["processing"] and not s["question_pending"])
            assert any("cancelled" in str(m.get("content", "")) for r in requests for m in r.get("messages", []) if m.get("tool_call_id") == "cancel-test"), requests
            snapshot("questions-finished.txt")
            (output / "questions-result.json").write_text(json.dumps({"select": True, "multi_select": True, "custom_multiline": True, "paste_never_submits": True, "review": True, "resize": True, "draft_restored": True, "plan_blocks_edits": True, "build_allows_edits": True, "cancel": True}, indent=2))
            print("PASS: selection, custom multiline answers, paste, review, resize, drafts, cancellation and enforced plan/build through a real PTY")
        finally:
            try:
                subprocess.run([binary, "server", "stop", "--force"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
            except subprocess.TimeoutExpired:
                pass
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
