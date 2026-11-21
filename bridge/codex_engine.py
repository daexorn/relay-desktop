"""Relay's OpenAI Codex CLI engine (`codex app-server --stdio`). Python stdlib only."""
import collections
from datetime import datetime, timezone
import fcntl
import json
import os
from pathlib import Path
import re
import shutil
import signal
import sqlite3
import subprocess
import threading
import time
import uuid

AGENT_ROLES = {
    "worker": "You are the development agent for this task. Work in the selected host and directory, respect project instructions and concurrent edits, implement the requested change and report files and actual verification results.",
    "planner": "You are the planning agent for this task. Inspect the project, identify dependencies and propose concrete implementation steps. Do not change files unless the user explicitly requests it. Report facts and open questions.",
    "reviewer": "You are the review agent for this task. Inspect relevant changes and run appropriate checks permitted by the user's instructions. Report findings with file references and evidence. Do not claim checks you did not run.",
}


def codex_home():
    raw = os.environ.get("CODEX_HOME")
    return Path(raw).expanduser() if raw else Path.home() / ".codex"


def codex_binary():
    home = Path.home()
    candidates = [
        str(home / ".local/bin/codex"),
        str(home / ".npm-global/bin/codex"),
        shutil.which("codex"),
        "/usr/local/bin/codex",
        "/usr/bin/codex",
        "/usr/sbin/codex",
    ]
    for path in candidates:
        if path and os.path.isfile(path) and os.access(path, os.X_OK):
            return path
    return None


def codex_version(binary=None):
    binary = binary or codex_binary()
    if not binary:
        return None
    try:
        out = subprocess.check_output(
            [binary, "--version"], text=True, timeout=10, stdin=subprocess.DEVNULL, stderr=subprocess.DEVNULL
        ).strip()
        return out or "codex"
    except (OSError, subprocess.SubprocessError):
        return "codex"


def credential_stamp():
    try:
        st = (codex_home() / "auth.json").stat()
        return st.st_dev, st.st_ino, st.st_mtime_ns, st.st_size
    except OSError:
        return None


def clean_error(error):
    text = str(error)
    text = re.sub(r"(?:sk-ant-|sk-proj-|sk-)[A-Za-z0-9_-]{16,}", "[ключ скрыт]", text)
    return text[:1400]


def tombstones_path():
    return Path.home() / ".cache/relay-desktop/deleted-codex-sessions.json"


def load_tombstones():
    try:
        data = json.loads(tombstones_path().read_text(encoding="utf-8"))
        return set(data) if isinstance(data, list) else set()
    except (OSError, ValueError):
        return set()


def add_tombstone(sid):
    path = tombstones_path()
    path.parent.mkdir(parents=True, mode=0o700, exist_ok=True)
    with open(path.with_suffix(".lock"), "a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        items = load_tombstones()
        items.add(sid.lower())
        tmp = path.with_suffix(".tmp")
        tmp.write_text(json.dumps(sorted(items)), encoding="utf-8")
        os.chmod(tmp, 0o600)
        tmp.replace(path)


def models_from_cache():
    cache = codex_home() / "models_cache.json"
    try:
        if not cache.is_file() or cache.stat().st_size > 8 * 1024 * 1024:
            return []
        raw = json.loads(cache.read_text(encoding="utf-8"))
        items = raw.get("models") if isinstance(raw, dict) else raw
        if not isinstance(items, list):
            return []
        out = []
        for m in items:
            if not isinstance(m, dict) or m.get("hidden"):
                continue
            slug = m.get("slug") or m.get("model") or m.get("id")
            if not isinstance(slug, str) or not slug:
                continue
            out.append({
                "value": slug,
                "ultra": any(isinstance(level, dict) and level.get("effort") == "ultra" for level in (m.get("supported_reasoning_levels") or [])),
                "displayName": str(m.get("display_name") or m.get("displayName") or slug),
                "description": str(m.get("description") or "")[:240],
            })
        return out[:200]
    except (OSError, ValueError, TypeError):
        return []


def probe_catalog(binary=None):
    binary = binary or codex_binary()
    cached = models_from_cache()
    if not binary:
        return cached, ""
    ver = codex_version(binary) or ""
    if cached:
        return cached, ver
    result = _probe_app_server(binary, "model/list", {"limit": 100}, timeout=15)
    models = []
    for model in result.get("data") or []:
        if isinstance(model, dict) and not model.get("hidden") and isinstance(model.get("model") or model.get("id"), str):
            value = model.get("model") or model["id"]
            models.append({
                "value": value,
                "ultra": any(isinstance(level, dict) and level.get("reasoningEffort") == "ultra" for level in (model.get("supportedReasoningEfforts") or [])),
                "displayName": str(model.get("displayName") or value),
                "description": str(model.get("description") or "")[:240],
            })
    return models or cached, ver


def probe_rate_limits():
    binary = codex_binary()
    return _probe_app_server(binary, "account/rateLimits/read", {}, timeout=6) if binary else {}


def consume_reset_credit(credit_id):
    binary = codex_binary()
    if not binary:
        return {}
    operation = str(uuid.uuid5(uuid.NAMESPACE_URL, "relay:codex-limit-reset:" + credit_id))
    return _probe_app_server(binary, "account/rateLimitResetCredit/consume",
                             {"creditId": credit_id, "idempotencyKey": operation}, timeout=12)


def _probe_app_server(binary, method, params, timeout):
    try:
        with subprocess.Popen(
            [binary, "app-server", "--stdio"],
            cwd=str(Path.home()),
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            encoding="utf-8",
            errors="replace",
            start_new_session=True,
        ) as proc:
            def stop():
                if proc.poll() is None:
                    try:
                        os.killpg(proc.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass

            def send(request):
                proc.stdin.write(json.dumps({"jsonrpc": "2.0", **request}) + "\n")
                proc.stdin.flush()

            timer = threading.Timer(timeout, stop)
            timer.start()
            try:
                send({"id": 1, "method": "initialize",
                      "params": {"clientInfo": {"name": "relay-desktop", "version": "0.15.0"}}})
                for line in proc.stdout:
                    try:
                        message = json.loads(line)
                    except ValueError:
                        continue
                    if not isinstance(message, dict):
                        continue
                    if message.get("id") == 1:
                        if "error" in message:
                            break
                        send({"method": "initialized"})
                        send({"id": 2, "method": method, "params": params})
                    elif message.get("id") == 2:
                        result = message.get("result")
                        return result if isinstance(result, dict) else {}
            finally:
                timer.cancel()
                stop()
    except OSError:
        pass
    return {}


def native_token_totals(sid):
    """Read the last cumulative token count, including for existing threads."""
    if not _valid_uuid(sid):
        return {}
    home = codex_home()
    rollouts = list((home / "sessions").glob(f"*/*/*/rollout-*-{sid.lower()}.jsonl"))
    rollouts.extend((home / "archived_sessions").glob(f"rollout-*-{sid.lower()}.jsonl"))
    for rollout in rollouts:
        try:
            with rollout.open("rb") as stream:
                size = os.fstat(stream.fileno()).st_size
                start = max(0, size - 16 * 1024 * 1024)
                stream.seek(start)
                if start:
                    stream.readline(size - start)
                for line in reversed(stream.read(size - stream.tell()).splitlines()):
                    if b'"token_count"' not in line:
                        continue
                    try:
                        row = json.loads(line)
                    except (ValueError, UnicodeError):
                        continue
                    payload = row.get("payload") if isinstance(row, dict) else None
                    if not isinstance(payload, dict) or payload.get("type") != "token_count":
                        continue
                    info = payload.get("info")
                    totals = info.get("total_token_usage") if isinstance(info, dict) else None
                    if isinstance(totals, dict):
                        return totals
        except OSError:
            continue
    return {}


def _valid_uuid(val):
    try:
        return isinstance(val, str) and str(uuid.UUID(val)) == val.lower()
    except ValueError:
        return False


def _clean_title(text, mtime):
    text = str(text or "")
    clean = globals().get("relay_title_text")
    if clean is not None:
        text = clean(text)
    else:
        text = " ".join(re.sub(r"<relay-files>.*?</relay-files>", "", text, flags=re.S).split())[:100]
    return text or ("Codex · " + time.strftime("%d.%m %H:%M", time.localtime(mtime)))


def native_sessions(excluded_ids=None):
    excluded = {s.lower() for s in (excluded_ids or set())} | load_tombstones()
    home = codex_home()
    db = home / "state_5.sqlite"
    results = {}
    if db.is_file():
        try:
            con = sqlite3.connect(f"file:{db}?mode=ro", uri=True, timeout=2)
            try:
                subagents = set()
                try:
                    subagents = {str(r[0]).lower() for r in con.execute("SELECT child_thread_id FROM thread_spawn_edges")}
                except sqlite3.Error:
                    pass
                rows = con.execute(
                    "SELECT id, cwd, name, title, first_user_message, preview, updated_at, created_at, archived "
                    "FROM threads ORDER BY updated_at DESC LIMIT 1000"
                ).fetchall()
                for tid, cwd, name, title, first_msg, preview, updated_at, created_at, archived in rows:
                    if archived or not _valid_uuid(tid):
                        continue
                    low = tid.lower()
                    if low in excluded or low in subagents:
                        continue
                    mtime = float(updated_at or created_at or time.time())
                    if mtime > 1e11:
                        mtime /= 1000.0
                    clean = globals().get("relay_title_text") or (lambda text: str(text or "").strip())
                    raw_title = next((value for raw in (name, title, first_msg, preview) if (value := clean(str(raw or "")))), "")
                    resolved_cwd = str(Path(cwd).expanduser()) if cwd else str(Path.home())
                    results[low] = {
                        "id": low,
                        "native_id": low,
                        "title": _clean_title(raw_title, mtime),
                        "cwd": resolved_cwd,
                        "modified": mtime,
                        "messages": 0,
                        "backend": "codex",
                    }
            finally:
                con.close()
        except (sqlite3.Error, OSError):
            pass

    sessions_dir = home / "sessions"
    if sessions_dir.is_dir() and len(results) < 1000:
        for rollout in sessions_dir.glob("*/*/*/rollout-*.jsonl"):
            m = re.search(r"([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})$", rollout.stem)
            if not m:
                continue
            low = m.group(1).lower()
            if low in results or low in excluded:
                continue
            try:
                st = rollout.stat()
                cwd = str(Path.home())
                first_text = ""
                with rollout.open("rb") as f:
                    head = f.read(65536)
                for line in head.splitlines():
                    try:
                        row = json.loads(line)
                    except ValueError:
                        continue
                    payload = row.get("payload") if isinstance(row.get("payload"), dict) else row
                    if isinstance(payload.get("cwd"), str):
                        cwd = payload["cwd"]
                    if not first_text and payload.get("type") in ("message", "user_message") and payload.get("role") in (None, "user"):
                        content = payload.get("content") or payload.get("message") or ""
                        if isinstance(content, list):
                            first_text = " ".join(str(c.get("text") or "") for c in content if isinstance(c, dict))
                        elif isinstance(content, str):
                            first_text = content
                results[low] = {
                    "id": low,
                    "native_id": low,
                    "title": _clean_title(first_text, st.st_mtime),
                    "cwd": cwd,
                    "modified": st.st_mtime,
                    "messages": 0,
                    "backend": "codex",
                }
            except OSError:
                continue

    out = list(results.values())
    out.sort(key=lambda r: r["modified"], reverse=True)
    return out[:1000]


def has_native_session(sid):
    low = str(sid or "").lower()
    if not _valid_uuid(low) or low in load_tombstones():
        return False
    db = codex_home() / "state_5.sqlite"
    if db.is_file():
        try:
            con = sqlite3.connect(f"file:{db}?mode=ro", uri=True, timeout=2)
            try:
                row = con.execute("SELECT 1 FROM threads WHERE lower(id) = ? AND archived = 0 LIMIT 1", (low,)).fetchone()
                if row:
                    return True
            finally:
                con.close()
        except sqlite3.Error:
            pass
    sessions_dir = codex_home() / "sessions"
    if sessions_dir.is_dir():
        for _ in sessions_dir.glob(f"*/*/*/*{low}.jsonl"):
            return True
    return False


def native_cwd(sid):
    low = str(sid or "").lower()
    db = codex_home() / "state_5.sqlite"
    if db.is_file():
        try:
            con = sqlite3.connect(f"file:{db}?mode=ro", uri=True, timeout=2)
            try:
                row = con.execute("SELECT cwd FROM threads WHERE lower(id) = ? LIMIT 1", (low,)).fetchone()
                if row and row[0]:
                    return str(Path(row[0]).expanduser().resolve())
            finally:
                con.close()
        except (sqlite3.Error, OSError):
            pass
    return None


def _item_to_messages(item):
    if not isinstance(item, dict):
        return []
    kind = item.get("type")
    ident = str(item.get("id") or uuid.uuid4().hex)
    if kind == "userMessage":
        texts = []
        images = []
        for part in item.get("content") or []:
            if not isinstance(part, dict):
                continue
            if part.get("type") == "text" and part.get("text"):
                texts.append(str(part["text"]))
            elif part.get("type") in ("image", "localImage"):
                images.append({"media_type": "image/png", "data": ""})
        txt = without_relay_context("\n".join(texts)).strip()
        if not txt and not images:
            return []
        msg = {"role": "user", "text": txt, "uuid": ident, "backend": "codex"}
        if images:
            msg["images"] = images
        return [msg]
    if kind == "agentMessage":
        txt = str(item.get("text") or "").strip()
        return [{"role": "assistant", "text": txt, "uuid": ident, "backend": "codex"}] if txt else []
    if kind == "plan":
        txt = str(item.get("text") or "").strip()
        return [{"role": "assistant", "text": txt, "uuid": ident, "backend": "codex"}] if txt else []
    if kind == "commandExecution":
        cmd = str(item.get("command") or "")
        out = str(item.get("aggregatedOutput") or "")[:24000]
        code = item.get("exitCode")
        failed = item.get("status") == "failed" or (isinstance(code, int) and code != 0)
        if isinstance(code, int) and out and not out.startswith("Exit code:"):
            out = f"Exit code: {code}\n{out}"
        return [
            {"role": "tool", "tool": "Bash", "input": {"command": cmd}, "tool_id": ident, "backend": "codex"},
            {"role": "tool_result", "text": out, "failed": failed, "tool_id": ident, "backend": "codex"},
        ]
    if kind == "fileChange":
        changes = item.get("changes") or []
        paths = [str(c.get("path") or "") for c in changes if isinstance(c, dict) and c.get("path")]
        label = ", ".join(paths[:4]) or "изменение файлов"
        return [
            {"role": "tool", "tool": "Edit", "input": {"file_path": label}, "tool_id": ident, "backend": "codex"},
            {"role": "tool_result", "text": f"Обновлено файлов: {len(paths) or 1}", "failed": item.get("status") == "failed", "tool_id": ident, "backend": "codex"},
        ]
    if kind == "mcpToolCall":
        name = f"mcp__{item.get('server', 'mcp')}__{item.get('tool', 'call')}"
        res = item.get("result")
        err = item.get("error")
        out = json.dumps(err or res or {}, ensure_ascii=False)[:24000] if (err or res) else "Выполнено."
        return [
            {"role": "tool", "tool": name, "input": item.get("arguments") or {}, "tool_id": ident, "backend": "codex"},
            {"role": "tool_result", "text": out, "failed": bool(err) or item.get("status") == "failed", "tool_id": ident, "backend": "codex"},
        ]
    if kind == "webSearch":
        q = str(item.get("query") or "")
        return [
            {"role": "tool", "tool": "WebSearch", "input": {"query": q}, "tool_id": ident, "backend": "codex"},
            {"role": "tool_result", "text": q, "failed": False, "tool_id": ident, "backend": "codex"},
        ]
    return []



HISTORY_KEEP_TOOLS = 1500
HISTORY_FULL_RESULTS = 300


def history_window(messages):
    """Keep 100% of user/assistant dialogue; trim only ancient tool calls/results beyond HISTORY_KEEP_TOOLS."""
    msgs = list(messages)
    tool_indices = [i for i, m in enumerate(msgs) if isinstance(m, dict) and m.get("role") in ("tool", "tool_result")]
    drop_tools = set(tool_indices[:-HISTORY_KEEP_TOOLS]) if len(tool_indices) > HISTORY_KEEP_TOOLS else set()
    kept = [m for i, m in enumerate(msgs) if i not in drop_tools]
    results = [i for i, m in enumerate(kept) if isinstance(m, dict) and m.get("role") == "tool_result"]
    for i in results[:-HISTORY_FULL_RESULTS]:
        text = str(kept[i].get("text") or "")
        if len(text) > 1500:
            kept[i] = {**kept[i], "text": text[:1500] + "\n…"}
    return kept

def native_history(sid):
    low = str(sid or "").lower()
    if not _valid_uuid(low):
        raise ValueError("Некорректная сессия.")
    home = codex_home()
    hist_db = home / "thread_history_1.sqlite"
    messages = []
    if hist_db.is_file():
        try:
            con = sqlite3.connect(f"file:{hist_db}?mode=ro", uri=True, timeout=3)
            try:
                rows = con.execute(
                    "SELECT item_json FROM thread_items WHERE lower(thread_id) = ? ORDER BY rollout_ordinal ASC LIMIT 1200",
                    (low,),
                ).fetchall()
                for (raw,) in rows:
                    try:
                        messages.extend(_item_to_messages(json.loads(raw)))
                    except (ValueError, TypeError):
                        continue
            finally:
                con.close()
        except sqlite3.Error:
            pass
    if messages:
        return history_window(messages)

    sessions_dir = home / "sessions"
    if sessions_dir.is_dir():
        for rollout in sessions_dir.glob(f"*/*/*/*{low}.jsonl"):
            try:
                with rollout.open("rb") as f:
                    size = os.fstat(f.fileno()).st_size
                    if size > 48 * 1024 * 1024:
                        f.seek(size - 48 * 1024 * 1024)
                        f.readline()
                    for line in f:
                        try:
                            row = json.loads(line)
                        except ValueError:
                            continue
                        payload = row.get("payload") if isinstance(row.get("payload"), dict) else row
                        role = payload.get("role")
                        if role in ("user", "assistant"):
                            content = payload.get("content") or ""
                            if isinstance(content, list):
                                text = "\n".join(
                                    str(b.get("text") or "")
                                    for b in content
                                    if isinstance(b, dict) and b.get("type") in ("input_text", "output_text", "text")
                                ).strip()
                            else:
                                text = str(content).strip()
                            if role == "user":
                                text = without_relay_context(text)
                            if text:
                                messages.append({"role": role, "text": text, "uuid": str(row.get("id") or len(messages))})
            except OSError:
                pass
            if messages:
                return history_window(messages)
    raise ValueError("Сессия не найдена. Обнови список разговоров.")


def delete_native_session(sid):
    low = str(sid or "").lower()
    if not _valid_uuid(low):
        return
    add_tombstone(low)
    binary = codex_binary()
    if binary:
        try:
            subprocess.run(
                [binary, "archive", low],
                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=8
            )
        except (OSError, subprocess.SubprocessError):
            pass


def _mode_policies(mode):
    if mode == "auto":
        return "never", "danger-full-access", {"type": "dangerFullAccess"}
    if mode == "plan":
        return "on-request", "read-only", {"type": "readOnly"}
    if mode == "acceptEdits":
        return "on-request", "workspace-write", {"type": "workspaceWrite"}
    return "untrusted", "workspace-write", {"type": "workspaceWrite"}


def _map_effort(effort):
    if effort in ("low", "medium", "high", "xhigh", "ultra"):
        return effort
    if effort == "max":
        return "xhigh"
    return None


class CodexRun:
    backend = "codex"

    def __init__(self, owner, sid, cwd, model, mode, resume, effort="default"):
        binary = codex_binary()
        if not binary:
            raise ValueError("Codex CLI не найден на этом сервере. Установи `codex` или выбери другой агент.")
        self.binary = binary
        self.owner, self.sid, self.cwd = owner, sid, cwd
        self.model, self.mode, self.effort = model, mode, effort
        self.agent_role = getattr(owner, "agent_role", "")
        self.custom_prompt = getattr(owner, "custom_prompt", "")
        self.cluster_context = getattr(owner, "cluster_context", {})
        self.desktop_enabled = getattr(owner, "desktop_requested", False)
        self.desktop_context = getattr(owner, "desktop_context", "")
        self.busy = False
        self.turns = 0
        self.background = []
        self.closed = False
        self.auth_failed = False
        self.auth_stamp = credential_stamp()
        self.last_activity = time.monotonic()
        self.pending = {}
        self.error = collections.deque(maxlen=12)
        self.lock = threading.RLock()
        self.rpc_seq = 0
        self.rpc_waiters = {}
        self.thread_id = None
        self.initialized = False
        self.active_turn_id = None
        self.turn_done = threading.Event()
        self.turn_error = None
        self.stream_started = set()
        self.turn_text = []
        self.turn_blocks = []
        self.inbox = collections.deque()
        self.condition = threading.Condition()

        existing = owner.transcripts.find(sid)
        self.is_pure_native = bool(existing is None and resume and has_native_session(sid))
        self.path = existing or (owner.transcripts.root / re.sub(r"[^a-zA-Z0-9-]", "-", cwd) / (sid + ".jsonl"))
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.parent = None
        self.linked_thread_id = None
        self.prior_handoff = ""
        self._inspect_shared_transcript(resume)

        home = codex_home()
        home.mkdir(parents=True, mode=0o700, exist_ok=True)
        env = dict(os.environ)
        self.mcp_servers, self.system_prompt = self._build_environment()
        self.process = subprocess.Popen(
            [self.binary, "app-server", "--stdio"],
            cwd=self.cwd,
            env=env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            bufsize=1,
            start_new_session=True,
        )
        threading.Thread(target=self._read_stdout, daemon=True).start()
        threading.Thread(target=self._read_stderr, daemon=True).start()
        threading.Thread(target=self._worker, daemon=True).start()

    def _inspect_shared_transcript(self, resume):
        self.shared_rows = primary_chain(self.owner.api_records(self.path)) if self.path.exists() else []
        if self.shared_rows:
            self.parent = self.shared_rows[-1].get("uuid")
            self.linked_thread_id = next((row["codexThreadId"] for row in reversed(self.shared_rows)
                                          if isinstance(row.get("codexThreadId"), str)), None)
        elif resume and has_native_session(self.sid):
            self.linked_thread_id = self.sid

    def _load_relay_context(self, resumed):
        self.prior_handoff = native_handoff(self.shared_rows, self.backend, "codexThreadId", self.thread_id, resumed)
        self.shared_rows = []

    def _append_shared(self, role, content, context_complete=False):
        ident = str(uuid.uuid4())
        message = {"role": role, "content": content}
        if role == "assistant":
            message.update(
                id="msg_codex_" + uuid.uuid4().hex,
                type="message",
                model=self.model,
                stop_reason="end_turn",
                stop_sequence=None,
                usage={"input_tokens": 0, "output_tokens": 0},
            )
        row = {
            "type": role,
            "uuid": ident,
            "parentUuid": self.parent,
            "sessionId": self.sid,
            "cwd": self.cwd,
            "timestamp": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
            "isSidechain": False,
            "userType": "external",
            "relayBackend": "codex",
            "relayModel": self.model,
            "relayMode": self.mode,
            "relayEffort": self.effort,
            "relayActor": getattr(self.owner, "actor", ""),
            "relayAgentName": (self.cluster_context or {}).get("name", ""),
            "message": message,
        }
        if self.thread_id:
            row["codexThreadId"] = self.thread_id
        if context_complete:
            row["relayContextVersion"] = 1
        try:
            with self.path.open("a", encoding="utf-8") as f:
                fcntl.flock(f, fcntl.LOCK_EX)
                f.write(json.dumps(row, ensure_ascii=False) + "\n")
                f.flush()
                os.fsync(f.fileno())
            self.parent = ident
        except OSError:
            pass

    def _build_environment(self):
        # No Relay system prompt: only the user's own instructions and cluster coordination roles are passed.
        prompt = ""
        if self.mode == "plan":
            prompt += " Mode is PLAN: inspect and propose concrete steps without modifying files or running destructive commands."
        if self.agent_role in AGENT_ROLES:
            prompt += (" " if prompt else "") + AGENT_ROLES[self.agent_role]
        servers = {}
        if self.cluster_context.get("role") == "leader" and getattr(self.owner, "cluster", None):
            cfg = self.owner.cluster.config(self.sid, self.cluster_context["run"])
            servers["relay_cluster"] = {
                "command": cfg["command"],
                "args": cfg.get("args", []),
                "env": cfg.get("env", {}),
            }
            prompt += " " + getattr(self.owner, "cluster_instructions", "")
        elif self.cluster_context.get("role") == "worker":
            prompt += (
                " You are a worker directed by the lead agent of this Relay cluster. Complete only the assigned task "
                "and constraints; report concrete changes, artifacts, failures and checks to the lead."
            )
        if getattr(self.owner, "board_file", None) and self.cluster_context.get("role") != "worker":
            actor = getattr(self.owner, "actor", "") or "Relay"
            servers["relay_board"] = {
                "command": os.sys.executable,
                "args": [str(self.owner.board_file), "mcp"],
                "env": {"RELAY_BOARD_ACTOR": f"Codex · {actor}", "RELAY_BOARD_NAME": "sbrp"},
            }
        if self.desktop_enabled and getattr(self.owner, "desktop", None):
            try:
                ctx = json.loads(self.desktop_context or "{}")
            except ValueError:
                ctx = {}
            local = bool(ctx.get("local", True))
            tests = ctx.get("tests", "off") if ctx.get("tests") in ("auto", "ask") else "off"
            tools = (("download_file", "run_local_command") if local else ()) + (("minecraft_test",) if tests != "off" else ())
            if tools:
                config = self.owner.desktop.config(self.sid, tools, phone=True) if ctx.get("phone") else self.owner.desktop.config(self.sid, tools)
                raw_mcp = json.loads(config).get("mcpServers", {})
                for name, spec in raw_mcp.items():
                    servers[name] = {"command": spec["command"], "args": spec.get("args", []), "env": spec.get("env", {})}
        if self.custom_prompt:
            prompt += ("\n\n" if prompt.strip() else "") + self.custom_prompt
        return servers, prompt.strip()[:110000]

    def _emit(self, data):
        self.owner.send({"type": "claude", "sid": self.sid, "data": data})

    def _send_json(self, payload):
        with self.lock:
            if self.closed or self.process.poll() is not None:
                raise ValueError("Codex завершил работу. Отправь сообщение ещё раз.")
            self.process.stdin.write(json.dumps(payload, ensure_ascii=False) + "\n")
            self.process.stdin.flush()

    def _rpc(self, method, params=None, timeout=60):
        with self.lock:
            self.rpc_seq += 1
            req_id = self.rpc_seq
            ev = threading.Event()
            box = {}
            self.rpc_waiters[req_id] = (ev, box)
        try:
            self._send_json({"jsonrpc": "2.0", "id": req_id, "method": method, "params": params or {}})
            if not ev.wait(timeout):
                raise ValueError(f"Codex не ответил на запрос {method}.")
            if "error" in box:
                err = box["error"]
                msg = err.get("message") if isinstance(err, dict) else str(err)
                if "auth" in str(msg).lower() or "unauthorized" in str(msg).lower() or "401" in str(msg):
                    self.auth_failed = True
                raise ValueError(clean_error(msg or "Ошибка Codex."))
            return box.get("result") or {}
        finally:
            with self.lock:
                self.rpc_waiters.pop(req_id, None)

    def _read_stderr(self):
        for line in self.process.stderr:
            self.error.append(line.rstrip()[:800])

    def _read_stdout(self):
        try:
            for line in self.process.stdout:
                try:
                    msg = json.loads(line)
                except ValueError:
                    continue
                if not isinstance(msg, dict):
                    continue
                self.last_activity = time.monotonic()
                if "id" in msg and "method" not in msg:
                    with self.lock:
                        waiter = self.rpc_waiters.get(msg["id"])
                    if waiter:
                        ev, box = waiter
                        if "error" in msg and msg["error"] is not None:
                            box["error"] = msg["error"]
                        else:
                            box["result"] = msg.get("result")
                        ev.set()
                    continue
                method = msg.get("method")
                params = msg.get("params") or {}
                if "id" in msg and method:
                    threading.Thread(target=self._handle_server_request, args=(msg["id"], method, params), daemon=True).start()
                    continue
                self._handle_notification(method, params)
        finally:
            with self.lock:
                for ev, box in self.rpc_waiters.values():
                    box["error"] = {"message": "\n".join(self.error)[-800:] or "Codex завершил работу."}
                    ev.set()
            self.turn_done.set()
            was_busy = self.busy
            self.busy = False
            self.turns = 0
            self.owner.run_state(self)
            if not self.closed:
                self.owner.send({
                    "type": "run_closed",
                    "sid": self.sid,
                    "interrupted": was_busy,
                    "detail": "\n".join(self.error)[-1800:],
                })

    def _handle_server_request(self, rpc_id, method, params):
        req_id = f"codex-perm-{rpc_id}-{uuid.uuid4().hex[:8]}"
        try:
            if method in ("item/commandExecution/requestApproval", "execCommandApproval"):
                if self.mode == "auto":
                    self._send_json({"jsonrpc": "2.0", "id": rpc_id, "result": {"decision": "accept"}})
                    return
                cmd = params.get("command")
                if isinstance(cmd, list):
                    cmd = " ".join(str(x) for x in cmd)
                tool_input = {"command": str(cmd or ""), "cwd": str(params.get("cwd") or self.cwd)}
                allow, _ = self._await_permission(req_id, "Bash", tool_input)
                self._send_json({"jsonrpc": "2.0", "id": rpc_id, "result": {"decision": "accept" if allow else "decline"}})
            elif method in ("item/fileChange/requestApproval", "applyPatchApproval"):
                if self.mode in ("auto", "acceptEdits"):
                    self._send_json({"jsonrpc": "2.0", "id": rpc_id, "result": {"decision": "accept"}})
                    return
                tool_input = {"description": str(params.get("reason") or "Изменение файлов")}
                allow, _ = self._await_permission(req_id, "Edit", tool_input)
                self._send_json({"jsonrpc": "2.0", "id": rpc_id, "result": {"decision": "accept" if allow else "decline"}})
            elif method == "item/tool/requestUserInput":
                questions = []
                for q in params.get("questions") or []:
                    if not isinstance(q, dict):
                        continue
                    opts = [
                        {"label": str(o.get("label") or ""), "description": str(o.get("description") or "")}
                        for o in (q.get("options") or [])
                        if isinstance(o, dict)
                    ]
                    questions.append({
                        "id": str(q.get("id") or ""),
                        "question": str(q.get("question") or ""),
                        "header": str(q.get("header") or "Вопрос"),
                        "options": opts,
                    })
                allow, answers = self._await_permission(req_id, "AskUserQuestion", {"questions": questions})
                formatted = {}
                if allow and isinstance(answers, dict):
                    for q in questions:
                        qid = q["id"]
                        ans = answers.get(q["question"]) or answers.get(qid)
                        if ans:
                            formatted[qid] = {"answers": [str(ans)]}
                self._send_json({"jsonrpc": "2.0", "id": rpc_id, "result": {"answers": formatted}})
            else:
                self._send_json({"jsonrpc": "2.0", "id": rpc_id, "result": {"decision": "accept" if self.mode == "auto" else "decline"}})
        except Exception:
            pass

    def _await_permission(self, req_id, tool_name, tool_input):
        ev = threading.Event()
        entry = {"tool_name": tool_name, "input": tool_input, "event": ev, "allow": False, "answers": None}
        self.pending[req_id] = entry
        self.owner.send({"type": "permission", "sid": self.sid, "id": req_id, "tool": tool_name, "input": tool_input})
        try:
            end = time.monotonic() + 1200
            while not ev.wait(0.2):
                if self.closed or self.process.poll() is not None:
                    return False, None
                if time.monotonic() > end:
                    return False, None
            return bool(entry["allow"]), entry["answers"]
        finally:
            self.pending.pop(req_id, None)
            self.owner.send({"type": "permission_cancelled", "sid": self.sid, "id": req_id})

    def _handle_notification(self, method, params):
        if method == "turn/diff/updated" and params.get("threadId") == self.thread_id:
            diff = params.get("diff")
            if not isinstance(diff, str):
                return
            plus = minus = 0
            in_hunk = False
            for line in diff.splitlines():
                if line.startswith("diff --git "):
                    in_hunk = False
                elif line.startswith("@@ "):
                    in_hunk = True
                elif in_hunk:
                    plus += line.startswith("+")
                    minus += line.startswith("-")
            self.owner.send({"type": "turn_summary", "sid": self.sid, "backend": self.backend,
                             "added": plus, "removed": minus})
        elif method == "thread/tokenUsage/updated" and params.get("threadId") == self.thread_id:
            usage = params.get("tokenUsage") or {}
            last = usage.get("last") or {}
            used, limit = last.get("totalTokens"), usage.get("modelContextWindow")
            if type(used) is int and used >= 0 and type(limit) is int and limit > 0:
                self.owner.send({"type": "turn_summary", "sid": self.sid, "backend": self.backend,
                                 "used": used, "limit": limit})
        elif method == "item/agentMessage/delta":
            item_id = str(params.get("itemId") or "msg")
            delta = str(params.get("delta") or "")
            if not delta:
                return
            idx = len(self.stream_started)
            if item_id not in self.stream_started:
                self.stream_started.add(item_id)
                self._emit({"type": "stream_event", "event": {"type": "content_block_start", "index": idx, "content_block": {"type": "text", "text": ""}}})
            self._emit({"type": "stream_event", "event": {"type": "content_block_delta", "index": max(0, len(self.stream_started) - 1), "delta": {"type": "text_delta", "text": delta}}})
        elif method == "item/started":
            item = params.get("item") or {}
            kind = item.get("type")
            ident = str(item.get("id") or uuid.uuid4().hex)
            if kind == "commandExecution":
                cmd = str(item.get("command") or "")
                block = {"type": "tool_use", "id": ident, "name": "Bash", "input": {"command": cmd}}
                self.turn_blocks.append(("assistant", [block]))
                self._emit({"type": "assistant", "message": {"role": "assistant", "content": [block]}})
            elif kind == "fileChange":
                changes = item.get("changes") or []
                paths = [str(c.get("path") or "") for c in changes if isinstance(c, dict) and c.get("path")]
                block = {"type": "tool_use", "id": ident, "name": "Edit", "input": {"file_path": ", ".join(paths[:4]) or "файлы"}}
                self.turn_blocks.append(("assistant", [block]))
                self._emit({"type": "assistant", "message": {"role": "assistant", "content": [block]}})
            elif kind == "mcpToolCall":
                name = f"mcp__{item.get('server', 'mcp')}__{item.get('tool', 'call')}"
                block = {"type": "tool_use", "id": ident, "name": name, "input": item.get("arguments") or {}}
                self.turn_blocks.append(("assistant", [block]))
                self._emit({"type": "assistant", "message": {"role": "assistant", "content": [block]}})
            elif kind == "webSearch":
                block = {"type": "tool_use", "id": ident, "name": "WebSearch", "input": {"query": str(item.get("query") or "")}}
                self.turn_blocks.append(("assistant", [block]))
                self._emit({"type": "assistant", "message": {"role": "assistant", "content": [block]}})
        elif method == "item/completed":
            item = params.get("item") or {}
            kind = item.get("type")
            ident = str(item.get("id") or uuid.uuid4().hex)
            if kind == "agentMessage":
                txt = str(item.get("text") or "")
                if txt:
                    self.turn_text.append(txt)
                    block = {"type": "text", "text": txt}
                    self.turn_blocks.append(("assistant", [block]))
                    self._emit({"type": "assistant", "message": {"role": "assistant", "content": [block]}})
            elif kind == "commandExecution":
                out = str(item.get("aggregatedOutput") or "")[:24000]
                code = item.get("exitCode")
                failed = item.get("status") == "failed" or (isinstance(code, int) and code != 0)
                if isinstance(code, int):
                    out = f"Exit code: {code}\n{out}" if out else f"Exit code: {code}"
                res = {"type": "tool_result", "tool_use_id": ident, "content": out, "is_error": failed}
                self.turn_blocks.append(("user", [res]))
                self._emit({"type": "user", "message": {"role": "user", "content": [res]}})
            elif kind == "fileChange":
                changes = item.get("changes") or []
                paths = [str(c.get("path") or "") for c in changes if isinstance(c, dict) and c.get("path")]
                failed = item.get("status") == "failed"
                res = {"type": "tool_result", "tool_use_id": ident, "content": "Сохранено: " + (", ".join(paths[:6]) or "файлы"), "is_error": failed}
                self.turn_blocks.append(("user", [res]))
                self._emit({"type": "user", "message": {"role": "user", "content": [res]}})
            elif kind == "mcpToolCall":
                err = item.get("error")
                raw = err or item.get("result") or "Выполнено."
                out = raw if isinstance(raw, str) else json.dumps(raw, ensure_ascii=False)[:24000]
                res = {"type": "tool_result", "tool_use_id": ident, "content": out, "is_error": bool(err) or item.get("status") == "failed"}
                self.turn_blocks.append(("user", [res]))
                self._emit({"type": "user", "message": {"role": "user", "content": [res]}})
            elif kind == "webSearch":
                res = {"type": "tool_result", "tool_use_id": ident, "content": str(item.get("query") or "Поиск завершён"), "is_error": False}
                self.turn_blocks.append(("user", [res]))
                self._emit({"type": "user", "message": {"role": "user", "content": [res]}})
        elif method == "turn/started":
            turn = params.get("turn") or {}
            if isinstance(turn.get("id"), str):
                self.active_turn_id = turn["id"]
                self.owner.send({"type": "turn_summary", "sid": self.sid, "backend": self.backend, "reset_diff": True})
        elif method == "turn/completed":
            turn = params.get("turn") or {}
            status = turn.get("status")
            if status == "failed":
                err = turn.get("error") or {}
                msg = err.get("message") if isinstance(err, dict) else str(err or "Ошибка выполнения Codex.")
                if "auth" in str(msg).lower() or "401" in str(msg):
                    self.auth_failed = True
                self.turn_error = clean_error(msg)
            self.active_turn_id = None
            self.turn_done.set()
        elif method == "error":
            err = params.get("error") or params
            msg = err.get("message") if isinstance(err, dict) else str(err)
            if msg and not params.get("willRetry"):
                self.turn_error = clean_error(msg)

    def _ensure_thread(self):
        if self.thread_id:
            return
        if not getattr(self, "initialized", False):
            try:
                self._rpc("initialize", {"clientInfo": {"name": "relay-desktop", "version": "0.20.0"}}, timeout=30)
            except ValueError as error:
                if "already initialized" not in str(error).lower():
                    raise
            else:
                self._send_json({"jsonrpc": "2.0", "method": "initialized"})
            self.initialized = True
            try:
                res = self._rpc("model/list", {"limit": 100}, timeout=15)
                models = [
                    {
                        "value": m.get("model") or m.get("id"),
                        "displayName": str(m.get("displayName") or m.get("model") or m.get("id")),
                        "ultra": any(isinstance(level, dict) and level.get("reasoningEffort") == "ultra" for level in (m.get("supportedReasoningEfforts") or [])),
                        "description": str(m.get("description") or "")[:240],
                    }
                    for m in (res.get("data") or [])
                    if isinstance(m, dict) and not m.get("hidden") and isinstance(m.get("model") or m.get("id"), str)
                ]
                if models:
                    self.owner.learn_backend_catalog("codex", models, codex_version(self.binary) or "")
            except Exception:
                pass

        approval, sandbox_mode, _ = _mode_policies(self.mode)
        cfg = {"mcp_servers": self.mcp_servers} if self.mcp_servers else None
        chosen_model = None if not self.model or self.model in ("default", "inherit") else self.model

        if self.linked_thread_id and has_native_session(self.linked_thread_id):
            params = {
                "threadId": self.linked_thread_id,
                "cwd": self.cwd,
                "approvalPolicy": approval,
                "sandbox": sandbox_mode,
            }
            if self.system_prompt:
                params["developerInstructions"] = self.system_prompt
            if chosen_model:
                params["model"] = chosen_model
            if cfg:
                params["config"] = cfg
            for delay in (0.15, 0.35, 0.7, 1.4, None):
                try:
                    res = self._rpc("thread/resume", params, timeout=30)
                    break
                except ValueError as error:
                    if "already has an active writer" not in str(error).lower():
                        raise
                    if delay is None:
                        raise ValueError("Этот тред Codex всё ещё открыт в другом процессе. Дождись его завершения и повтори отправку.") from error
                    time.sleep(delay)
            self.thread_id = (res.get("thread") or {}).get("id") or self.linked_thread_id
            self._load_relay_context(resumed=True)
        else:
            params = {
                "cwd": self.cwd,
                "approvalPolicy": approval,
                "sandbox": sandbox_mode,
            }
            if self.system_prompt:
                params["developerInstructions"] = self.system_prompt
            if chosen_model:
                params["model"] = chosen_model
            if cfg:
                params["config"] = cfg
            res = self._rpc("thread/start", params, timeout=30)
            self.thread_id = (res.get("thread") or {}).get("id")
            if not self.thread_id:
                raise ValueError("Codex не создал сессию. Проверь вход в Codex на сервере.")
            self.linked_thread_id = self.thread_id
            self._load_relay_context(resumed=False)

    @staticmethod
    def _build_inputs(text, images):
        items = []
        if text:
            items.append({"type": "text", "text": text})
        for img in images or []:
            if isinstance(img, dict) and img.get("data"):
                mime = img.get("media_type") or "image/png"
                items.append({"type": "image", "url": f"data:{mime};base64,{img['data']}"})
        return items or [{"type": "text", "text": ""}]

    def prompt(self, text, images=None, steer=False):
        if steer and self.busy and self.thread_id and self.active_turn_id:
            try:
                self._append_shared("user", ([{"type": "text", "text": text}] if text else []) + [{"type": "image", "source": i} for i in (images or [])])
                self._rpc("turn/steer", {
                    "threadId": self.thread_id,
                    "expectedTurnId": self.active_turn_id,
                    "input": self._build_inputs(text, images),
                }, timeout=15)
                self.owner.run_state(self)
                return
            except Exception:
                pass
        with self.condition:
            if self.closed:
                raise ValueError("Сессия Codex закрыта. Отправь сообщение ещё раз.")
            if len(self.inbox) >= 12:
                raise ValueError("В очереди Codex уже 12 сообщений.")
            self.inbox.append((text, images or []))
            self.turns += 1
            self.busy = True
            self.last_activity = time.monotonic()
            self.condition.notify()
        self.owner.run_state(self)

    def _worker(self):
        while True:
            with self.condition:
                while not self.closed and not self.inbox:
                    self.condition.wait()
                if self.closed:
                    return
                text, images = self.inbox.popleft()

            self.turn_done.clear()
            self.turn_error = None
            self.stream_started.clear()
            self.turn_text.clear()
            self.turn_blocks.clear()
            error = None
            try:
                self._ensure_thread()
                mirror = not self.is_pure_native
                if mirror:
                    self._append_shared(
                        "user",
                        ([{"type": "text", "text": text}] if text else [])
                        + [{"type": "image", "source": img} for img in images],
                    )
                self._emit({"type": "system", "subtype": "init", "session_id": self.sid, "model": self.model, "relay_backend": "codex"})
                approval, _, sandbox_policy = _mode_policies(self.mode)
                turn_params = {
                    "threadId": self.thread_id,
                    "cwd": self.cwd,
                    "input": self._build_inputs(self.prior_handoff + text, images),
                    "approvalPolicy": approval,
                    "sandboxPolicy": sandbox_policy,
                }
                if self.model and self.model not in ("default", "inherit"):
                    turn_params["model"] = self.model
                mapped_effort = _map_effort(self.effort)
                if mapped_effort:
                    turn_params["effort"] = mapped_effort
                res = self._rpc("turn/start", turn_params, timeout=30)
                turn_obj = res.get("turn") or {}
                if isinstance(turn_obj.get("id"), str):
                    self.active_turn_id = turn_obj["id"]
                while not self.turn_done.wait(0.25):
                    if self.closed or self.process.poll() is not None:
                        raise ValueError("Codex завершил работу во время ответа.")
                if self.turn_error:
                    error = self.turn_error
                if mirror:
                    for i, (role, blocks) in enumerate(self.turn_blocks):
                        self._append_shared(role, blocks, context_complete=not error and i == len(self.turn_blocks) - 1)
                if not error:
                    self.prior_handoff = ""
            except Exception as exc:
                error = clean_error(exc)

            with self.condition:
                self.turns = max(0, self.turns - 1)
                if error:
                    self.inbox.clear()
                    self.turns = 0
                self.busy = self.turns > 0
                if not self.busy:
                    self.pending.clear()
                self.last_activity = time.monotonic()

            final_text = "\n\n".join(self.turn_text).strip()
            self._emit({
                "type": "result",
                "subtype": "error_during_execution" if error else "success",
                "is_error": bool(error),
                "result": error or final_text,
                "errors": [error] if error else [],
                "session_id": self.sid,
                "relay_backend": "codex",
            })
            self.owner.run_state(self)

    def control(self, subtype, **fields):
        if subtype == "set_model":
            self.model = fields.get("model") or "default"
        elif subtype == "set_permission_mode":
            self.mode = fields.get("mode") or "manual"

    def permit(self, request_id, allow, answers):
        entry = self.pending.get(request_id)
        if entry is None:
            raise ValueError("Этот запрос уже закрыт.")
        entry["allow"] = bool(allow)
        entry["answers"] = answers
        entry["event"].set()

    def stop(self):
        if getattr(self.owner, "cluster", None):
            self.owner.cluster.cancel(self.sid)
        if getattr(self.owner, "desktop", None):
            self.owner.desktop.cancel(self.sid)
        with self.condition:
            self.inbox.clear()
        for entry in list(self.pending.values()):
            entry["allow"] = False
            entry["event"].set()
        tid, turn_id = self.thread_id, self.active_turn_id
        if tid and turn_id:
            try:
                self._rpc("turn/interrupt", {"threadId": tid, "turnId": turn_id}, timeout=5)
            except Exception:
                pass

    def close(self):
        self.closed = True
        if getattr(self.owner, "cluster", None):
            self.owner.cluster.cancel(self.sid, revoke=True)
        if getattr(self.owner, "desktop", None):
            self.owner.desktop.cancel(self.sid, revoke=True)
        for entry in list(self.pending.values()):
            entry["allow"] = False
            entry["event"].set()
        with self.condition:
            self.inbox.clear()
            self.condition.notify_all()
        if self.process.poll() is None:
            try:
                self.process.stdin.close()
            except OSError:
                pass
            try:
                os.killpg(self.process.pid, signal.SIGTERM)
                self.process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(self.process.pid, signal.SIGKILL)
                    self.process.wait(timeout=2)
                except (ProcessLookupError, subprocess.TimeoutExpired):
                    pass
            except ProcessLookupError:
                pass
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            if stream is not None:
                try:
                    stream.close()
                except OSError:
                    pass
