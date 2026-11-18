import importlib.util
import io
import json
import os
from pathlib import Path
import socket
import tarfile
import tempfile
import threading
import time
import unittest
from unittest import mock
import uuid

SPEC = importlib.util.spec_from_file_location(
    "relay_bridge_priv_inst_test", Path(__file__).parents[1] / "bridge/relay_bridge.py"
)
bridge = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bridge)


def make_transcript(projects_dir: Path, sid: str, secret_title: str, secret_message: str):
    proj = projects_dir / "-tmp-proj"
    proj.mkdir(parents=True, exist_ok=True)
    path = proj / f"{sid}.jsonl"
    rows = [
        {
            "type": "user",
            "uuid": "u1",
            "parentUuid": None,
            "cwd": str(projects_dir),
            "message": {"role": "user", "content": secret_title},
        },
        {
            "type": "assistant",
            "uuid": "a1",
            "parentUuid": "u1",
            "cwd": str(projects_dir),
            "message": {"role": "assistant", "content": [{"type": "text", "text": secret_message}]},
        },
    ]
    path.write_text("\n".join(json.dumps(r, ensure_ascii=False) for r in rows) + "\n", encoding="utf-8")
    return path


class PrivateSessionsAndInstallerTest(unittest.TestCase):
    def test_session_meta_pbkdf2_never_stores_plaintext_and_redacts_titles(self):
        with tempfile.TemporaryDirectory() as tmp:
            meta_file = Path(tmp) / "sessions.json"
            meta = bridge.SessionMeta(meta_file)
            sid = str(uuid.uuid4())
            meta.session(sid, title="Секретный проект Альфа", actor="Alice")
            meta.set_lock(sid, password="SuperSecretPassword!42", actor="Alice")

            raw_disk = meta_file.read_text(encoding="utf-8")
            self.assertNotIn("SuperSecretPassword!42", raw_disk)
            disk_json = json.loads(raw_disk)
            lock_obj = disk_json["sessions"][sid]["lock"]
            self.assertEqual(len(bytes.fromhex(lock_obj["salt"])), 16)
            self.assertTrue(len(lock_obj["verifier"]) >= 64)

            self.assertTrue(meta.is_locked(sid))
            self.assertFalse(meta.verify_lock(sid, "wrong-password"))
            self.assertFalse(meta.verify_lock(sid, ""))
            self.assertTrue(meta.verify_lock(sid, "SuperSecretPassword!42"))

            raw_rows = [{"id": sid, "title": "Авто-заголовок секрета", "cwd": tmp, "modified": 100.0}]
            redacted, _ = meta.apply(raw_rows, unlocked_ids=set())
            self.assertTrue(redacted[0]["locked"])
            self.assertFalse(redacted[0]["unlocked"])
            self.assertEqual(redacted[0]["title"], "Защищённая сессия")
            self.assertEqual(redacted[0]["auto_title"], "")

            opened, _ = meta.apply(raw_rows, unlocked_ids={sid})
            self.assertTrue(opened[0]["locked"])
            self.assertTrue(opened[0]["unlocked"])
            self.assertEqual(opened[0]["title"], "Секретный проект Альфа")

            with self.assertRaises(ValueError):
                meta.set_lock(sid, remove=True, current_password="bad")
            meta.set_lock(sid, remove=True, current_password="SuperSecretPassword!42")
            self.assertFalse(meta.is_locked(sid))

    def test_bridge_blocks_history_rename_prompt_and_delete_when_locked(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            sid = str(uuid.uuid4())
            projects = home / ".claude/projects"
            make_transcript(projects, sid, "Приватный ключ деплоя", "Внутренний ответ секретной сессии")
            events = []
            with mock.patch.dict(os.environ, {"HOME": str(home), "RELAY_BOARD_DIR": str(home / "no-shared")}):
                b = bridge.Bridge(send=events.append, transcripts=bridge.Transcripts(projects))
                try:
                    b.dispatch({"op": "session_lock", "sid": sid, "password": "correct-horse-battery"})
                    # Creator stays unlocked right after locking
                    b.dispatch({"op": "history", "sid": sid})
                    self.assertTrue(any(e.get("type") == "history" and len(e.get("messages", [])) == 2 for e in events))

                    # Explicit relock
                    events.clear()
                    b.dispatch({"op": "session_relock", "sid": sid})
                    self.assertTrue(any(e.get("type") == "session_lock_changed" and e.get("unlocked") is False for e in events))
                    sess_ev = next(e for e in reversed(events) if e.get("type") == "sessions")
                    row = next(s for s in sess_ev["sessions"] if s["id"] == sid)
                    self.assertEqual(row["title"], "Защищённая сессия")
                    self.assertTrue(row["locked"])
                    self.assertFalse(row["unlocked"])

                    # History must NOT leak messages
                    events.clear()
                    b.dispatch({"op": "history", "sid": sid})
                    self.assertFalse(any(e.get("type") == "history" for e in events))
                    self.assertTrue(any(e.get("type") == "session_locked" and e.get("sid") == sid for e in events))
                    self.assertNotIn("Внутренний ответ", json.dumps(events, ensure_ascii=False))

                    # Rename, prompt, delete must all fail
                    for blocked in (
                        {"op": "session_meta", "sid": sid, "title": "Взлом"},
                        {"op": "prompt", "sid": sid, "text": "Привет", "cwd": str(home)},
                        {"op": "delete_session", "sid": sid},
                    ):
                        events.clear()
                        b.dispatch(blocked)
                        self.assertTrue(any(e.get("type") == "error" and "защищена паролем" in e.get("message", "") for e in events))

                    # Wrong password rejected
                    events.clear()
                    b.dispatch({"op": "session_unlock", "sid": sid, "password": "wrong"})
                    self.assertTrue(any(e.get("type") == "session_unlock_result" and e.get("ok") is False for e in events))

                    # Correct password unlocks history and delete
                    events.clear()
                    b.dispatch({"op": "session_unlock", "sid": sid, "password": "correct-horse-battery"})
                    self.assertTrue(any(e.get("type") == "session_unlock_result" and e.get("ok") is True for e in events))
                    b.dispatch({"op": "history", "sid": sid})
                    self.assertTrue(any(e.get("type") == "history" and len(e.get("messages", [])) == 2 for e in events))
                    b.dispatch({"op": "delete_session", "sid": sid})
                    self.assertTrue(any(e.get("type") == "session_deleted" and e.get("sid") == sid for e in events))
                finally:
                    b.close()

    def test_host_multi_client_isolation_for_locked_sessions_and_live_stream(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            sid = str(uuid.uuid4())
            projects = home / ".claude/projects"
            make_transcript(projects, sid, "Финансовый отчёт", "Секретные цифры Q4")
            sock_path = home / "host.sock"

            with mock.patch.dict(os.environ, {"HOME": str(home), "RELAY_BOARD_DIR": str(home / "no-shared")}):
                host = bridge.Host(bridge_factory=lambda send: bridge.Bridge(send=send, transcripts=bridge.Transcripts(projects)))
                server_thread = threading.Thread(target=host.serve, args=(str(sock_path),), daemon=True)
                server_thread.start()
                deadline = time.monotonic() + 5
                while not sock_path.exists() and time.monotonic() < deadline:
                    time.sleep(0.02)

                def connect_client():
                    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                    s.settimeout(2.0)
                    s.connect(str(sock_path))
                    f = s.makefile("rb")
                    return s, f

                def read_until(f, predicate, timeout=2.0):
                    seen = []
                    end = time.monotonic() + timeout
                    while time.monotonic() < end:
                        line = f.readline()
                        if not line:
                            break
                        ev = json.loads(line)
                        seen.append(ev)
                        if predicate(ev):
                            return seen
                    return seen

                sa, fa = connect_client()
                sb, fb = connect_client()
                try:
                    read_until(fa, lambda e: e.get("type") == "hello")
                    read_until(fb, lambda e: e.get("type") == "hello")

                    # Client A locks session
                    sa.sendall((json.dumps({"op": "session_lock", "sid": sid, "password": "alpha-pin"}) + "\n").encode())
                    a_evs = read_until(fa, lambda e: e.get("type") == "sessions")
                    b_evs = read_until(fb, lambda e: e.get("type") == "sessions")

                    a_row = next(s for e in reversed(a_evs) if e.get("type") == "sessions" for s in e["sessions"] if s["id"] == sid)
                    b_row = next(s for e in reversed(b_evs) if e.get("type") == "sessions" for s in e["sessions"] if s["id"] == sid)
                    self.assertEqual(a_row["title"], "Финансовый отчёт")
                    self.assertTrue(a_row["unlocked"])
                    self.assertEqual(b_row["title"], "Защищённая сессия")
                    self.assertFalse(b_row["unlocked"])

                    # Live stream broadcast for locked session reaches A, NOT B!
                    host.route({"type": "claude", "sid": sid, "data": {"type": "assistant", "message": {"content": [{"type": "text", "text": "ТОЛЬКО ДЛЯ A"}]}}})
                    sa.sendall((json.dumps({"op": "ping"}) + "\n").encode())
                    sb.sendall((json.dumps({"op": "ping"}) + "\n").encode())
                    a_stream = read_until(fa, lambda e: e.get("type") == "pong")
                    b_stream = read_until(fb, lambda e: e.get("type") == "pong")
                    self.assertTrue(any(e.get("type") == "claude" for e in a_stream))
                    self.assertFalse(any(e.get("type") == "claude" for e in b_stream))
                    self.assertNotIn("ТОЛЬКО ДЛЯ A", json.dumps(b_stream, ensure_ascii=False))
                finally:
                    fa.close(); fb.close()
                    sa.close(); sb.close()
                    host.retiring = True
                    host.bridge.close()

    def test_agent_installer_installs_claude_codex_and_agy_into_user_bin(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            events = []
            done = threading.Event()

            # Build fake codex musl tar.gz archive in memory
            codex_tar_buf = io.BytesIO()
            with tarfile.open(fileobj=codex_tar_buf, mode="w:gz") as tf:
                payload = b"#!/bin/sh\necho 'codex-cli 0.168.0'\n"
                info = tarfile.TarInfo(name="codex-x86_64-unknown-linux-musl")
                info.size = len(payload)
                info.mode = 0o755
                tf.addfile(info, io.BytesIO(payload))
            codex_tar_bytes = codex_tar_buf.getvalue()

            claude_script = b"#!/bin/sh\nmkdir -p \"$HOME/.local/bin\"\ncat <<'EOF' > \"$HOME/.local/bin/claude\"\n#!/bin/sh\necho '2.1.99 (Claude Code)'\nEOF\nchmod 755 \"$HOME/.local/bin/claude\"\n"
            agy_script = b"#!/bin/sh\nmkdir -p \"$HOME/.local/bin\"\ncat <<'EOF' > \"$HOME/.local/bin/agy\"\n#!/bin/sh\necho '1.6.0'\nEOF\nchmod 755 \"$HOME/.local/bin/agy\"\n"

            def fake_download(url, max_bytes=0, timeout=0):
                if "claude.ai/install.sh" in url:
                    return claude_script
                if "antigravity.google/cli/install.sh" in url:
                    return agy_script
                if "openai/codex/releases" in url:
                    return codex_tar_bytes
                raise ValueError(f"Unexpected URL: {url}")

            def on_send(ev):
                events.append(ev)
                if ev.get("type") == "agent_install" and ev.get("running") is False:
                    finished = [x for x in events if x.get("type") == "agent_install" and x.get("running") is False]
                    if len(finished) >= 3:
                        done.set()

            with mock.patch.dict(os.environ, {"HOME": str(home), "PATH": "/usr/bin:/bin", "RELAY_BOARD_DIR": str(home / "no-shared")}):
                b = bridge.Bridge(send=on_send, transcripts=bridge.Transcripts(home / ".claude/projects"))
                b.installer_mod["download_https"] = fake_download
                b.installer_mod["codex_musl_target"] = lambda: "x86_64-unknown-linux-musl"
                try:
                    # Reject arbitrary / unknown agent identifiers
                    b.dispatch({"op": "agent_install", "agent": "https://evil.example/install.sh"})
                    self.assertTrue(any(e.get("type") == "error" for e in events))
                    events.clear()

                    # Install all 3 official agents
                    for agent in ("claude", "codex", "agy"):
                        b.dispatch({"op": "agent_install", "agent": agent})
                    self.assertTrue(done.wait(timeout=10), f"Timed out waiting for installs: {events}")

                    status = b.installer.status()
                    self.assertTrue(status["claude"]["installed"])
                    self.assertIn("2.1.99", status["claude"]["version"])
                    self.assertTrue(status["codex"]["installed"])
                    self.assertIn("0.168.0", status["codex"]["version"])
                    self.assertTrue(status["agy"]["installed"])
                    self.assertIn("1.6.0", status["agy"]["version"])
                    self.assertEqual(b.binary, str(home / ".local/bin/claude"))
                    self.assertEqual(b.auth.binary, str(home / ".local/bin/claude"))
                finally:
                    b.close()


if __name__ == "__main__":
    unittest.main()
