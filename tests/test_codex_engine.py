import importlib.util
import json
import os
from pathlib import Path
import queue
import sqlite3
import tempfile
import time
import unittest
from unittest import mock
import uuid

SPEC = importlib.util.spec_from_file_location("relay_bridge_codex_test", Path(__file__).parents[1] / "bridge/relay_bridge.py")
bridge = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bridge)

FAKE_CODEX = """#!/usr/bin/env python3
import json, os, sys

if sys.argv[1:] == ["--version"]:
    print("codex-cli 0.167.0")
    sys.exit(0)

assert sys.argv[1:] == ["app-server", "--stdio"], sys.argv
log_file = os.environ.get("FAKE_CODEX_LOG")

def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\\n")
    sys.stdout.flush()

def record(obj):
    if log_file:
        with open(log_file, "a", encoding="utf-8") as f:
            f.write(json.dumps(obj) + "\\n")

waiters = {}
turn_seq = 0

for raw in sys.stdin:
    msg = json.loads(raw)
    record(msg)
    if "method" not in msg and "id" in msg:
        # Response to server-initiated approval/question
        waiters[msg["id"]] = msg.get("result") or {}
        continue
    method = msg.get("method")
    rid = msg.get("id")
    params = msg.get("params") or {}
    if method == "initialize":
        emit({"jsonrpc": "2.0", "id": rid, "result": {"userAgent": "codex-fake"}})
    elif method == "initialized":
        pass
    elif method == "model/list":
        emit({"jsonrpc": "2.0", "id": rid, "result": {"data": [
            {"id": "gpt-5.4", "model": "gpt-5.4", "displayName": "GPT-5.4", "description": "Flagship"},
            {"id": "gpt-5.3-codex", "model": "gpt-5.3-codex", "displayName": "GPT-5.3 Codex", "description": "Coding"}
        ]}})
    elif method == "thread/start":
        emit({"jsonrpc": "2.0", "id": rid, "result": {"thread": {"id": "11111111-2222-4333-8444-555555555555"}}})
    elif method == "thread/resume":
        emit({"jsonrpc": "2.0", "id": rid, "result": {"thread": {"id": params["threadId"]}}})
    elif method == "turn/steer":
        emit({"jsonrpc": "2.0", "id": rid, "result": {"turnId": params["expectedTurnId"]}})
    elif method == "turn/interrupt":
        emit({"jsonrpc": "2.0", "id": rid, "result": {}})
        emit({"jsonrpc": "2.0", "method": "turn/completed", "params": {"turn": {"id": params["turnId"], "status": "interrupted"}}})
    elif method == "turn/start":
        turn_seq += 1
        tid = f"turn-{turn_seq}"
        emit({"jsonrpc": "2.0", "id": rid, "result": {"turn": {"id": tid, "status": "inProgress"}}})
        emit({"jsonrpc": "2.0", "method": "turn/started", "params": {"turn": {"id": tid}}})
        text_in = " ".join(i.get("text", "") for i in params.get("input", []) if i.get("type") == "text")
        if "ask-approval" in text_in:
            emit({"jsonrpc": "2.0", "id": 901, "method": "item/commandExecution/requestApproval", "params": {"command": ["ls", "-la"], "cwd": params.get("cwd")}})
            while 901 not in waiters:
                nxt = json.loads(sys.stdin.readline())
                record(nxt)
                if nxt.get("id") == 901:
                    waiters[901] = nxt.get("result") or {}
            decision = waiters.pop(901).get("decision", "decline")
            ans = f"Decision: {decision}"
        elif "ask-question" in text_in:
            emit({"jsonrpc": "2.0", "id": 902, "method": "item/tool/requestUserInput", "params": {"questions": [{"id": "q1", "header": "Env", "question": "Target OS?", "options": [{"label": "Linux", "description": "x64"}]}]}})
            while 902 not in waiters:
                nxt = json.loads(sys.stdin.readline())
                record(nxt)
                if nxt.get("id") == 902:
                    waiters[902] = nxt.get("result") or {}
            got = waiters.pop(902).get("answers", {}).get("q1", {}).get("answers", [""])[0]
            ans = f"Picked: {got}"
        else:
            emit({"jsonrpc": "2.0", "method": "item/started", "params": {"item": {"id": "cmd-1", "type": "commandExecution", "command": "cargo check"}}})
            emit({"jsonrpc": "2.0", "method": "item/completed", "params": {"item": {"id": "cmd-1", "type": "commandExecution", "command": "cargo check", "aggregatedOutput": "Finished dev", "exitCode": 0, "status": "completed"}}})
            ans = f"Codex reply to: {text_in}"
        emit({"jsonrpc": "2.0", "method": "item/agentMessage/delta", "params": {"itemId": f"msg-{turn_seq}", "delta": ans[:6]}})
        emit({"jsonrpc": "2.0", "method": "item/agentMessage/delta", "params": {"itemId": f"msg-{turn_seq}", "delta": ans[6:]}})
        emit({"jsonrpc": "2.0", "method": "item/completed", "params": {"item": {"id": f"msg-{turn_seq}", "type": "agentMessage", "text": ans}}})
        emit({"jsonrpc": "2.0", "method": "turn/completed", "params": {"turn": {"id": tid, "status": "completed"}}})
"""


class CodexEngineTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.codex_home = self.root / ".codex"
        self.codex_home.mkdir(parents=True)
        self.bin_dir = self.root / "bin"
        self.bin_dir.mkdir()
        self.codex_bin = self.bin_dir / "codex"
        self.codex_bin.write_text(FAKE_CODEX, encoding="utf-8")
        self.codex_bin.chmod(0o700)
        self.rpc_log = self.root / "rpc.jsonl"
        self.project = self.root / "workspace"
        self.project.mkdir()

        self.env_patch = mock.patch.dict(
            os.environ,
            {
                "HOME": str(self.root),
                "CODEX_HOME": str(self.codex_home),
                "FAKE_CODEX_LOG": str(self.rpc_log),
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

    def test_hello_advertises_codex_and_catalog_probe(self):
        self.host.hello()
        hello, _ = self.wait_for(lambda e: e.get("type") == "hello")
        self.assertTrue(hello["backends"]["codex"]["installed"])
        self.assertIn("0.167.0", hello["backends"]["codex"]["version"])
        self.host.dispatch({"op": "catalog", "backend": "codex"})
        cat, _ = self.wait_for(lambda e: e.get("type") == "catalog" and e.get("backend") == "codex")
        self.assertEqual([m["value"] for m in cat["models"]], ["gpt-5.4", "gpt-5.3-codex"])

    def test_codex_turn_streaming_tools_mcp_and_shared_history(self):
        sid = str(uuid.uuid4())
        cluster_src = (Path(__file__).parents[1] / "bridge/cluster_mcp.py").read_text(encoding="utf-8")
        run_id = str(uuid.uuid4())
        with mock.patch.object(bridge, "CLUSTER_SOURCE", cluster_src, create=True):
            self.host.dispatch({
                "op": "prompt",
                "backend": "codex",
                "sid": sid,
                "cwd": str(self.project),
                "text": "Build feature",
                "model": "gpt-5.4",
                "mode": "auto",
                "effort": "high",
                "cluster_context": {"run": run_id, "role": "leader", "name": "Team"},
            })
            res, seen = self.wait_for(lambda e: e.get("type") == "claude" and e["data"].get("type") == "result")
        self.assertFalse(res["data"]["is_error"])
        self.assertEqual(res["data"]["result"], "Codex reply to: Build feature")
        self.assertTrue(any(e.get("type") == "claude" and e["data"].get("type") == "stream_event" for e in seen))

        # Check JSON-RPC sent to codex app-server
        logged = [json.loads(line) for line in self.rpc_log.read_text(encoding="utf-8").splitlines()]
        thread_start = next(m for m in logged if m.get("method") == "thread/start")
        self.assertIn("relay_cluster", thread_start["params"]["config"]["mcp_servers"])
        turn_start = next(m for m in logged if m.get("method") == "turn/start")
        self.assertEqual(turn_start["params"]["model"], "gpt-5.4")
        self.assertEqual(turn_start["params"]["effort"], "high")

        # Shared history preserves conversation and marks backend=codex
        self.host.dispatch({"op": "history", "sid": sid})
        hist, _ = self.wait_for(lambda e: e.get("type") == "history" and e.get("sid") == sid)
        texts = [(m["role"], m.get("text") or m.get("tool")) for m in hist["messages"]]
        self.assertIn(("user", "Build feature"), texts)
        self.assertIn(("tool", "Bash"), texts)
        self.assertIn(("assistant", "Codex reply to: Build feature"), texts)

        self.host.dispatch({"op": "sessions"})
        sess, _ = self.wait_for(lambda e: e.get("type") == "sessions")
        found = next(s for s in sess["sessions"] if s["id"] == sid)
        self.assertEqual(found["backend"], "codex")
        self.assertEqual(found["native_id"], "11111111-2222-4333-8444-555555555555")

    def test_codex_manual_approval_and_user_question(self):
        sid = str(uuid.uuid4())
        self.host.dispatch({
            "op": "prompt",
            "backend": "codex",
            "sid": sid,
            "cwd": str(self.project),
            "text": "ask-approval",
            "mode": "manual",
        })
        perm, _ = self.wait_for(lambda e: e.get("type") == "permission" and e.get("sid") == sid)
        self.assertEqual(perm["tool"], "Bash")
        self.host.dispatch({"op": "permission", "sid": sid, "id": perm["id"], "allow": True})
        res, _ = self.wait_for(lambda e: e.get("type") == "claude" and e["data"].get("type") == "result")
        self.assertEqual(res["data"]["result"], "Decision: accept")

        self.host.dispatch({
            "op": "prompt",
            "backend": "codex",
            "sid": sid,
            "cwd": str(self.project),
            "text": "ask-question",
            "mode": "manual",
        })
        q_perm, _ = self.wait_for(lambda e: e.get("type") == "permission" and e.get("tool") == "AskUserQuestion")
        self.host.dispatch({
            "op": "permission",
            "sid": sid,
            "id": q_perm["id"],
            "allow": True,
            "answers": {"Target OS?": "Linux"},
        })
        res2, _ = self.wait_for(lambda e: e.get("type") == "claude" and e["data"].get("type") == "result")
        self.assertEqual(res2["data"]["result"], "Picked: Linux")

    def test_native_sqlite_sessions_history_and_tombstone_delete(self):
        native_id = str(uuid.uuid4())
        sub_id = str(uuid.uuid4())
        con = sqlite3.connect(self.codex_home / "state_5.sqlite")
        con.execute("CREATE TABLE threads (id TEXT, cwd TEXT, name TEXT, title TEXT, first_user_message TEXT, preview TEXT, updated_at INTEGER, created_at INTEGER, archived INTEGER)")
        con.execute("CREATE TABLE thread_spawn_edges (parent_thread_id TEXT, child_thread_id TEXT)")
        con.execute("INSERT INTO threads VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0)", (native_id, str(self.project), "", "Native Codex Task", "Fix parser", "", int(time.time()), int(time.time())))
        con.execute("INSERT INTO threads VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0)", (sub_id, str(self.project), "", "Subagent", "Sub", "", int(time.time()), int(time.time())))
        con.execute("INSERT INTO thread_spawn_edges VALUES (?, ?)", (native_id, sub_id))
        con.commit()
        con.close()

        hcon = sqlite3.connect(self.codex_home / "thread_history_1.sqlite")
        hcon.execute("CREATE TABLE thread_items (thread_id TEXT, rollout_ordinal INTEGER, item_json TEXT)")
        hcon.execute("INSERT INTO thread_items VALUES (?, 1, ?)", (native_id, json.dumps({"id": "u1", "type": "userMessage", "content": [{"type": "text", "text": "Fix parser"}]})))
        hcon.execute("INSERT INTO thread_items VALUES (?, 2, ?)", (native_id, json.dumps({"id": "a1", "type": "agentMessage", "text": "Parser fixed!"})))
        hcon.commit()
        hcon.close()

        self.host.dispatch({"op": "sessions"})
        sess, _ = self.wait_for(lambda e: e.get("type") == "sessions")
        ids = {s["id"] for s in sess["sessions"]}
        self.assertIn(native_id, ids)
        self.assertNotIn(sub_id, ids)

        self.host.dispatch({"op": "history", "sid": native_id})
        hist, _ = self.wait_for(lambda e: e.get("type") == "history" and e.get("sid") == native_id)
        self.assertEqual([m["text"] for m in hist["messages"]], ["Fix parser", "Parser fixed!"])

        self.host.dispatch({"op": "delete_session", "sid": native_id})
        self.wait_for(lambda e: e.get("type") == "session_deleted" and e.get("sid") == native_id)
        self.host.dispatch({"op": "sessions"})
        sess2, _ = self.wait_for(lambda e: e.get("type") == "sessions")
        self.assertNotIn(native_id, {s["id"] for s in sess2["sessions"]})


if __name__ == "__main__":
    unittest.main()
