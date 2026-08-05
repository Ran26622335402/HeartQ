#!/usr/bin/env python3
"""Minimal ACP NDJSON client for heartq agent stdio multi-turn tests."""

from __future__ import annotations

import json
import os
import select
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any, Callable, Optional


class AcpClient:
    def __init__(self, debug_file: Path, cwd: str):
        self.debug_file = debug_file
        self.cwd = cwd
        self._id = 0
        self._pending: dict[int, dict[str, Any]] = {}
        self._notifications: list[dict[str, Any]] = []
        self._lock = threading.Lock()
        self._reader_done = threading.Event()
        env = os.environ.copy()
        env["HEARTQ_HOME"] = env.get("HEARTQ_HOME", "/root/.heartq")
        env["GROK_HOME"] = env["HEARTQ_HOME"]
        self.proc = subprocess.Popen(
            [
                "/root/.heartq/bin/heartq",
                "agent",
                "--always-approve",
                "--no-leader",
                "stdio",
                "--debug-file",
                str(debug_file),
            ],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
            cwd=cwd,
            env=env,
        )
        assert self.proc.stdin and self.proc.stdout
        self._thread = threading.Thread(target=self._read_loop, daemon=True)
        self._thread.start()

    def _read_loop(self) -> None:
        assert self.proc.stdout
        for line in self.proc.stdout:
            line = line.strip()
            if not line:
                continue
            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                continue
            with self._lock:
                if "id" in msg and ("result" in msg or "error" in msg):
                    self._pending[int(msg["id"])] = msg
                else:
                    self._notifications.append(msg)
        self._reader_done.set()

    def request(self, method: str, params: Optional[dict] = None, timeout: float = 180.0) -> Any:
        self._id += 1
        req_id = self._id
        payload = {"jsonrpc": "2.0", "id": req_id, "method": method}
        if params is not None:
            payload["params"] = params
        assert self.proc.stdin
        self.proc.stdin.write(json.dumps(payload, separators=(",", ":")) + "\n")
        self.proc.stdin.flush()
        deadline = time.time() + timeout
        while time.time() < deadline:
            with self._lock:
                if req_id in self._pending:
                    msg = self._pending.pop(req_id)
                    if "error" in msg:
                        raise RuntimeError(f"{method} error: {msg['error']}")
                    return msg.get("result")
            if self.proc.poll() is not None:
                raise RuntimeError(f"agent exited early while waiting for {method}")
            time.sleep(0.05)
        raise TimeoutError(f"timeout waiting for {method} id={req_id}")

    def notify(self, method: str, params: Optional[dict] = None) -> None:
        payload: dict[str, Any] = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            payload["params"] = params
        assert self.proc.stdin
        self.proc.stdin.write(json.dumps(payload, separators=(",", ":")) + "\n")
        self.proc.stdin.flush()

    def notifications_text(self) -> str:
        with self._lock:
            return json.dumps(self._notifications, ensure_ascii=False)

    def close(self, wait: float = 90.0) -> int:
        try:
            if self.proc.stdin:
                self.proc.stdin.close()
        except Exception:
            pass
        try:
            return self.proc.wait(timeout=wait)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            return self.proc.wait(timeout=10)


def bootstrap(client: AcpClient) -> str:
    init = client.request(
        "initialize",
        {
            "protocolVersion": 1,
            "clientCapabilities": {
                "fs": {"readTextFile": False, "writeTextFile": False},
                "terminal": False,
            },
            "_meta": {
                "clientType": "dialogue-test",
                "startupHints": {
                    "nonInteractive": True,
                    "skipGitStatus": True,
                    "skipProjectLayout": True,
                },
            },
        },
    )
    auth_methods = init.get("authMethods") or []
    method_id = "xai.api_key"
    if auth_methods:
        method_id = auth_methods[0].get("id", method_id)
    client.request("authenticate", {"methodId": method_id, "_meta": {"headless": True}})
    opened = client.request(
        "session/new",
        {"cwd": client.cwd, "mcpServers": []},
    )
    session_id = opened["sessionId"]
    return session_id


def prompt(client: AcpClient, session_id: str, text: str, timeout: float = 300.0) -> Any:
    return client.request(
        "session/prompt",
        {
            "sessionId": session_id,
            "prompt": [{"type": "text", "text": text}],
            "_meta": {"screenMode": "headless"},
        },
        timeout=timeout,
    )


def main() -> int:
    if len(sys.argv) < 3:
        print("usage: acp_multiturn_client.py <label> <prompt1> [prompt2...]", file=sys.stderr)
        return 2
    label = sys.argv[1]
    prompts = sys.argv[2:]
    out_dir = Path("/workspace/heartq-build/docs/dialogue-tests/results")
    out_dir.mkdir(parents=True, exist_ok=True)
    debug = out_dir / f"{label}.debug"
    log = out_dir / f"{label}.log"
    home = os.environ.get("HEARTQ_HOME", "/root/.heartq")
    cwd = "/workspace/heartq-build"

    client = AcpClient(debug, cwd)
    lines: list[str] = []
    try:
        sid = bootstrap(client)
        lines.append(f"sessionId={sid}")
        for i, text in enumerate(prompts, 1):
            lines.append(f"--- turn {i}: {text}")
            result = prompt(client, sid, text)
            lines.append(f"stopReason={result.get('stopReason') if isinstance(result, dict) else result}")
            # Collect assistant text from notifications if present
            time.sleep(0.3)
        # Explicit terminal close triggers session-end hooks (learning graph, etc.).
        # Mere stdin disconnect detaches without finalizing.
        try:
            close_res = client.request(
                "_x.ai/session/close", {"sessionId": sid}, timeout=120
            )
            lines.append(f"session_close={close_res}")
            # Give the actor a moment to flush LEARNING_GRAPH / dream hooks.
            time.sleep(2)
        except Exception as e:
            lines.append(f"session_close skipped: {e}")
        code = client.close(wait=180)
        lines.append(f"exit_code={code}")
        notes_path = out_dir / f"{label}.notifications.json"
        notes_path.write_text(client.notifications_text(), encoding="utf-8")
        lines.append(f"notifications_file={notes_path}")
        lines.append("notifications_snip=" + client.notifications_text()[:2000])
    except Exception as e:
        lines.append(f"ERROR: {e}")
        try:
            client.close(wait=20)
        except Exception:
            pass
        log.write_text("\n".join(lines) + "\n", encoding="utf-8")
        print("\n".join(lines))
        return 1

    log.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
