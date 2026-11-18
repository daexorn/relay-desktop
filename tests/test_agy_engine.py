import importlib.util
import json
import os
from pathlib import Path
import queue
import tempfile
import time
import unittest
from unittest import mock
import uuid

SPEC = importlib.util.spec_from_file_location("relay_bridge_agy_test", Path(__file__).parents[1] / "bridge/relay_bridge.py")
bridge = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bridge)

FAKE_AGY = """#!/usr/bin/env python3
import json, os, sys

if sys.argv[1:] == ["--version"]:
    print("1.5.10")
    sys.exit(0)

if sys.argv[1:] == ["models"]:
    print(json.dumps({"models": [
        {"id": "gemini-3.1-pro-preview", "name": "Gemini 3.1 Pro", "description": "Deep reasoning"},
        {"id": "gemini-3-flash-preview", "name": "Gemini 3 Flash", "description": "Fast agent"}
    ]}))
    sys.exit(0)

assert sys.argv[1:] == ["acp", "--stdio"], sys.argv
log_file = os.environ.get("FAKE_AGY_LOG")

def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\\n")
    sys.stdout.flush()

def record(obj):
    if log_file:
        with open(log_file, "a", encoding="utf-8") as f:
            f.write(json.dumps(obj) + "\\n")

waiters = {}

for raw in sys.stdin:
    msg = json.loads(raw)
    record(msg)
    if "method" not in msg and "id" in msg:
        waiters[msg["id"]] = msg.get("result") or {}
        continue
    method = msg.get("method")
    rid = msg.get("id")
    params = msg.get("params") or {}
    if method == "initialize":
        emit({"jsonrpc": "2.0", "id": rid, "result": {"protocolVersion": 1, "agentCapabilities": {"loadSession": True}}})
    elif method in ("session/new", "session/resume"):
        sid = params.get("sessionId") or "22222222-3333-4444-8555-666666666666"
        emit({"jsonrpc": "2.0", "id": rid, "result": {
            "sessionId": sid,
            "modes": {"availableModes": [{"id": "default"}, {"id": "auto"}, {"id": "accept-edits"}, {"id": "plan"}]},
            "models": {"availableModels": [{"modelId": "gemini-3.1-pro-preview", "name": "Gemini 3.1 Pro"}]},
            "configOptions": [{"id": "thought_level"}]
        }})
    elif method in ("session/set_model", "session/set_mode", "session/set_config_option"):
        emit({"jsonrpc": "2.0", "id": rid, "result": {}})
    elif method == "session/cancel":
        pass
    elif method == "session/prompt":
        sid = params.get("sessionId")
        text_in = " ".join(b.get("text", "") for b in params.get("prompt", []) if b.get("type") == "text")
        if "need-auth" in text_in:
            emit({"jsonrpc": "2.0", "id": rid, "error": {"code": -32000, "message": "Authentication required"}})
            continue
        if "ask-perm" in text_in:
            emit({"jsonrpc": "2.0", "id": 701, "method": "session/request_permission", "params": {
                "sessionId": sid,
                "toolCall": {"toolCallId": "tc-perm", "title": "run_command", "kind": "execute", "rawInput": {"command": "pytest"}},
                "options": [
                    {"optionId": "allow_once", "kind": "allow_once", "name": "Allow"},
                    {"optionId": "reject_once", "kind": "reject_once", "name": "Reject"}
                ]
            }})
            while 701 not in waiters:
                nxt = json.loads(sys.stdin.readline())
                record(nxt)
                if nxt.get("id") == 701:
                    waiters[701] = nxt.get("result") or {}
            chosen = (waiters.pop(701).get("outcome") or {}).get("optionId", "none")
            reply = f"AGY perm: {chosen}"
        else:
            emit({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
                "sessionUpdate": "tool_call",
                "toolCallId": "tc-1",
                "title": "Read",
                "rawInput": {"file_path": "Cargo.toml"}
            }}})
            emit({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "tc-1",
                "status": "completed",
                "content": [{"type": "content", "content": {"type": "text", "text": "[package] name = relay"}}]
            }}})
            reply = f"AGY reply: {text_in}"
        emit({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": reply[:5]}
        }}})
        emit({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": {
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": reply[5:]}
        }}})
        emit({"jsonrpc": "2.0", "id": rid, "result": {"stopReason": "end_turn"}})
"""


class AgyEngineTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin_dir = self.root / "bin"
        self.bin_dir.mkdir()
        self.agy_bin = self.bin_dir / "agy"
        self.agy_bin.write_text(FAKE_AGY, encoding="utf-8")
        self.agy_bin.chmod(0o700)
        self.rpc_log = self.root / "agy_rpc.jsonl"
        self.project = self.root / "workspace"
        self.project.mkdir()

        self.env_patch = mock.patch.dict(
            os.environ,
            {
                "HOME": str(self.root),
                "FAKE_AGY_LOG": str(self.rpc_log),
                "PATH": f"{self.bin_dir}:{os.environ.get('PATH', '')}",
            },
        )
        self.env_patch.start()
        self.addCleanup(self.env_patch.stop)

        self.events = queue.Queue()
        self.host = bridge.Bridge(
            send=self.events.put,
            transcripts=bridge.Transcripts(self.root / ".claude/projects"),
            binary=None,
        )
        self.addCleanup(self.host.close)

    def test_switch_from_native_codex_keeps_session_and_sends_history_to_agy(self):
        sid = str(uuid.uuid4())
        self.host.codex["has_native_session"] = lambda ident: ident == sid
        self.host.codex["native_cwd"] = lambda ident: str(self.project)
        self.host.codex["native_history"] = lambda ident: [
            {"role": "user", "text": "Проверь сайт"},
            {"role": "assistant", "text": "Редактор ролей проверен, все маршруты доступны."},
        ]
        self.host.dispatch({"op": "prompt", "sid": sid, "cwd": str(self.project), "backend": "agy", "text": "Что было сделано?"})
        result, _ = self.wait_for(lambda e: e.get("type") == "claude" and e["data"].get("type") == "result")
        self.assertFalse(result["data"]["is_error"])
        self.assertEqual(result["sid"], sid)
        calls = [json.loads(line) for line in self.rpc_log.read_text().splitlines()]
        sent = next(m for m in calls if m.get("method") == "session/prompt")["params"]["prompt"][0]["text"]
        self.assertIn("Редактор ролей проверен, все маршруты доступны.", sent)
        self.assertTrue(sent.endswith("Что было сделано?"))
        messages = self.host.transcripts.history(sid)
        self.assertEqual([m["text"] for m in messages if m["role"] == "user"], ["Проверь сайт", "Что было сделано?"])

    def wait_for(self, predicate, timeout=6.0):
        end = time.monotonic() + timeout
        seen = []
        while time.monotonic() < end:
            try:
                ev = self.events.get(timeout=0.2)
                seen.append(ev)
                if predicate(ev):
                    return ev, seen
            except queue.Empty:
                continue
        self.fail(f"Timed out waiting for event; seen={seen}")

    def test_hello_and_agy_catalog_probe(self):
        self.host.hello()
        hello, _ = self.wait_for(lambda e: e.get("type") == "hello")
        self.assertTrue(hello["backends"]["agy"]["installed"])
        self.assertIn("1.5.10", hello["backends"]["agy"]["version"])
        self.host.dispatch({"op": "catalog", "backend": "agy"})
        cat, _ = self.wait_for(lambda e: e.get("type") == "catalog" and e.get("backend") == "agy")
        self.assertEqual([m["value"] for m in cat["models"]], ["gemini-3.1-pro-preview", "gemini-3-flash-preview"])

    def test_agy_acp_prompt_streaming_mcp_and_permissions(self):
        sid = str(uuid.uuid4())
        board_src = (Path(__file__).parents[1] / "bridge/board.py").read_text(encoding="utf-8")
        with mock.patch.dict(os.environ, {"RELAY_BOARD_DIR": str(self.root / "board")}), mock.patch.object(bridge, "BOARD_SOURCE", board_src, create=True):
            host = bridge.Bridge(
                send=self.events.put,
                transcripts=bridge.Transcripts(self.root / ".claude/projects"),
                binary=None,
            )
            self.addCleanup(host.close)
            host.dispatch({
                "op": "prompt",
                "backend": "agy",
                "sid": sid,
                "cwd": str(self.project),
                "text": "Inspect Cargo",
                "model": "gemini-3.1-pro-preview",
                "mode": "acceptEdits",
                "effort": "high",
            })
            res, seen = self.wait_for(lambda e: e.get("type") == "claude" and e["data"].get("type") == "result")
            self.assertFalse(res["data"]["is_error"])
            self.assertEqual(res["data"]["result"], "AGY reply: Inspect Cargo")
            self.assertTrue(any(e.get("type") == "claude" and e["data"].get("type") == "stream_event" for e in seen))

            logged = [json.loads(line) for line in self.rpc_log.read_text(encoding="utf-8").splitlines()]
            snew = next(m for m in logged if m.get("method") == "session/new")
            self.assertTrue(any(s["name"] == "relay_board" for s in snew["params"]["mcpServers"]))
            self.assertIn("Relay supports downloadable", snew["params"]["_meta"]["antigravity"]["systemPrompt"]["append"])
            self.assertTrue(any(m.get("method") == "session/set_model" and m["params"]["modelId"] == "gemini-3.1-pro-preview" for m in logged))
            self.assertTrue(any(m.get("method") == "session/set_mode" and m["params"]["modeId"] == "accept-edits" for m in logged))

            # Permission prompt in manual mode
            host.dispatch({
                "op": "prompt",
                "backend": "agy",
                "sid": sid,
                "cwd": str(self.project),
                "text": "ask-perm",
                "mode": "manual",
            })
            perm, _ = self.wait_for(lambda e: e.get("type") == "permission" and e.get("sid") == sid)
            host.dispatch({"op": "permission", "sid": sid, "id": perm["id"], "allow": False})
            res2, _ = self.wait_for(lambda e: e.get("type") == "claude" and e["data"].get("type") == "result")
            self.assertEqual(res2["data"]["result"], "AGY perm: reject_once")

            # Shared transcript preserved
            host.dispatch({"op": "history", "sid": sid})
            hist, _ = self.wait_for(lambda e: e.get("type") == "history" and e.get("sid") == sid)
            self.assertTrue(any(m.get("text") == "AGY reply: Inspect Cargo" for m in hist["messages"]))

    def test_native_brain_sessions_history_and_delete(self):
        native_sid = str(uuid.uuid4())
        log_dir = self.root / ".gemini/antigravity-cli/brain" / native_sid / ".system_generated/logs"
        log_dir.mkdir(parents=True)
        transcript = log_dir / "transcript.jsonl"
        transcript.write_text(
            "\n".join([
                json.dumps({"step_index": 0, "type": "USER_INPUT", "content": f"<USER_REQUEST>\nRefactor UI\n</USER_REQUEST>\nCommand Working Directory: {self.project}"}),
                json.dumps({"step_index": 1, "type": "PLANNER_RESPONSE", "content": "Checking files.", "tool_calls": [{"name": "read_file", "args": {"Path": "src/main.rs"}}]}),
                json.dumps({"step_index": 2, "type": "GENERIC", "status": "DONE", "content": "fn main() {}"}),
                json.dumps({"step_index": 3, "type": "PLANNER_RESPONSE", "content": "Done refactoring!"}),
            ]) + "\n",
            encoding="utf-8",
        )
        self.host.dispatch({"op": "sessions"})
        sess, _ = self.wait_for(lambda e: e.get("type") == "sessions")
        found = next(s for s in sess["sessions"] if s["id"] == native_sid)
        self.assertEqual(found["backend"], "agy")
        self.assertEqual(found["title"], "Refactor UI")
        self.assertEqual(found["cwd"], str(self.project))

        self.host.dispatch({"op": "history", "sid": native_sid})
        hist, _ = self.wait_for(lambda e: e.get("type") == "history" and e.get("sid") == native_sid)
        roles = [m["role"] for m in hist["messages"]]
        self.assertEqual(roles, ["user", "assistant", "tool", "tool_result", "assistant"])
        self.assertEqual(hist["messages"][0]["text"], "Refactor UI")
        self.assertEqual(hist["messages"][-1]["text"], "Done refactoring!")

        self.host.dispatch({"op": "delete_session", "sid": native_sid})
        self.wait_for(lambda e: e.get("type") == "session_deleted" and e.get("sid") == native_sid)
        self.assertFalse(transcript.exists())


if __name__ == "__main__":
    unittest.main()
