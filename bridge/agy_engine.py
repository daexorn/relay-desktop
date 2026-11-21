"""Relay's Google Antigravity CLI (`agy acp --stdio`) engine. Python stdlib only."""
import collections
from datetime import datetime, timezone
import fcntl
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import threading
import time
import uuid

AGENT_ROLES = {
    "worker": "You are the development agent for this task. Work in the selected host and directory, respect project instructions and concurrent edits, implement the requested change and report files and actual verification results.",
    "planner": "You are the planning agent for this task. Inspect the project, identify dependencies and propose concrete implementation steps. Do not change files unless the user explicitly requests it. Report facts and open questions.",
    "reviewer": "You are the review agent for this task. Inspect relevant changes and run appropriate checks permitted by the user's instructions. Report findings with file references and evidence. Do not claim checks you did not run.",
}


def agy_data_dir():
    return Path.home() / ".gemini/antigravity-cli"


def agy_binary():
    home = Path.home()
    candidates = [
        str(home / ".local/bin/agy"),
        str(home / ".gemini/antigravity-cli/bin/agy"),
        shutil.which("agy"),
        "/usr/local/bin/agy",
        "/usr/bin/agy",
    ]
    for path in candidates:
        if path and os.path.isfile(path) and os.access(path, os.X_OK):
            return path
    return None


def agy_version(binary=None):
    binary = binary or agy_binary()
    if not binary:
        return None
    try:
        out = subprocess.check_output(
            [binary, "--version"], text=True, timeout=10, stdin=subprocess.DEVNULL, stderr=subprocess.DEVNULL
        ).strip()
        return f"agy {out}" if out and not out.lower().startswith("agy") else (out or "agy")
    except (OSError, subprocess.SubprocessError):
        return "agy"


def credential_stamp():
    for candidate in (
        agy_data_dir() / "settings.json",
        Path.home() / ".gemini/oauth_creds.json",
        Path.home() / ".gemini/config",
    ):
        try:
            st = candidate.stat()
            return st.st_dev, st.st_ino, st.st_mtime_ns, st.st_size
        except OSError:
            continue
    return None


def clean_error(error):
    text = str(error)
    text = re.sub(r"(?:AIza|ya29\.|sk-)[A-Za-z0-9_.-]{16,}", "[ключ скрыт]", text)
    return text[:1400]


def tombstones_path():
    return Path.home() / ".cache/relay-desktop/deleted-agy-sessions.json"


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


def _valid_uuid(val):
    try:
        return isinstance(val, str) and str(uuid.UUID(val)) == val.lower()
    except ValueError:
        return False


def _strip_tags(raw):
    text = str(raw or "")
    m = re.search(r"<USER_REQUEST>\s*(.*?)\s*</USER_REQUEST>", text, flags=re.S)
    if m:
        text = m.group(1)
    text = re.sub(r"<ADDITIONAL_METADATA>.*?</ADDITIONAL_METADATA>", "", text, flags=re.S)
    text = re.sub(r"<SYSTEM_MESSAGE>.*?</SYSTEM_MESSAGE>", "", text, flags=re.S)
    text = re.sub(r"<relay-files>.*?</relay-files>", "", text, flags=re.S)
    return text.strip()


def probe_catalog(binary=None):
    binary = binary or agy_binary()
    if not binary:
        return [], ""
    ver = agy_version(binary) or ""
    try:
        res = subprocess.run(
            [binary, "models"],
            cwd=str(Path.home()),
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=20,
        )
        models = []
        if res.returncode == 0 and res.stdout.strip():
            raw = res.stdout.strip()
            try:
                parsed = json.loads(raw)
                items = parsed.get("models") if isinstance(parsed, dict) else parsed
                if isinstance(items, list):
                    for item in items:
                        if isinstance(item, dict) and isinstance(item.get("id") or item.get("modelId") or item.get("value"), str):
                            mid = item.get("id") or item.get("modelId") or item.get("value")
                            models.append({
                                "value": mid,
                                "displayName": str(item.get("name") or item.get("displayName") or mid),
                                "description": str(item.get("description") or "")[:240],
                            })
            except ValueError:
                for line in raw.splitlines():
                    clean = line.strip()
                    if not clean or clean.lower().startswith(("available", "fetching", "model", "---", "===")):
                        continue
                    parts = re.split(r"\s{2,}|\t+", clean, maxsplit=2)
                    slug = parts[0].lstrip("*-• ").strip()
                    if slug and re.fullmatch(r"[a-zA-Z0-9._:/-]+", slug):
                        disp = parts[1].strip() if len(parts) > 1 else slug
                        desc = parts[2].strip() if len(parts) > 2 else ""
                        models.append({"value": slug, "displayName": disp, "description": desc[:240]})
        return models[:200], ver
    except (OSError, subprocess.SubprocessError):
        return [], ver


def native_sessions(excluded_ids=None):
    excluded = {s.lower() for s in (excluded_ids or set())} | load_tombstones()
    brain = agy_data_dir() / "brain"
    if not brain.is_dir():
        return []
    out = []
    for folder in brain.iterdir():
        if not folder.is_dir() or not _valid_uuid(folder.name):
            continue
        low = folder.name.lower()
        if low in excluded:
            continue
        transcript = folder / ".system_generated/logs/transcript.jsonl"
        if not transcript.is_file():
            continue
        try:
            st = transcript.stat()
            first_user = ""
            cwd = str(Path.home())
            with transcript.open("rb") as f:
                head = f.read(196 * 1024)
            for line in head.splitlines():
                try:
                    row = json.loads(line)
                except ValueError:
                    continue
                if not isinstance(row, dict):
                    continue
                content = str(row.get("content") or "")
                m_ws = re.search(r"Command Working Directory:\s*(\S+)", content)
                if m_ws:
                    cwd = m_ws.group(1).strip()
                if row.get("type") == "USER_INPUT" and not first_user:
                    cleaned = _strip_tags(content)
                    clean = globals().get("relay_title_text")
                    first_user = clean(cleaned) if clean is not None else " ".join(cleaned.split())[:100]
            title = first_user or ("Antigravity · " + time.strftime("%d.%m %H:%M", time.localtime(st.st_mtime)))
            out.append({
                "id": low,
                "native_id": low,
                "title": title,
                "cwd": cwd,
                "modified": st.st_mtime,
                "messages": 0,
                "backend": "agy",
            })
        except OSError:
            continue
    out.sort(key=lambda r: r["modified"], reverse=True)
    return out[:1000]


def has_native_session(sid):
    low = str(sid or "").lower()
    if not _valid_uuid(low) or low in load_tombstones():
        return False
    return (agy_data_dir() / "brain" / low / ".system_generated/logs/transcript.jsonl").is_file()


def native_cwd(sid):
    low = str(sid or "").lower()
    transcript = agy_data_dir() / "brain" / low / ".system_generated/logs/transcript.jsonl"
    if not transcript.is_file():
        return None
    try:
        with transcript.open("rb") as f:
            head = f.read(196 * 1024)
        for line in head.splitlines():
            try:
                row = json.loads(line)
            except ValueError:
                continue
            if isinstance(row, dict):
                m_ws = re.search(r"Command Working Directory:\s*(\S+)", str(row.get("content") or ""))
                if m_ws:
                    return str(Path(m_ws.group(1).strip()).expanduser().resolve())
    except OSError:
        pass
    return None



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
    transcript = agy_data_dir() / "brain" / low / ".system_generated/logs/transcript.jsonl"
    if not transcript.is_file():
        raise ValueError("Сессия не найдена. Обнови список разговоров.")
    messages = []
    pending_tool_ids = collections.deque()
    rows = []
    with transcript.open("rb") as f:
        size = os.fstat(f.fileno()).st_size
        if size > 48 * 1024 * 1024:
            f.seek(size - 48 * 1024 * 1024)
            f.readline()
        for line in f:
            try:
                row = json.loads(line)
            except ValueError:
                continue
            if isinstance(row, dict):
                rows.append(row)
    # The compact transcript cuts long fields; take those steps whole from transcript_full.jsonl.
    wanted = {row.get("step_index") for row in rows
              if row.get("truncated_fields") and row.get("type") in ("USER_INPUT", "PLANNER_RESPONSE", "GENERIC")}
    wanted.discard(None)
    full_path = transcript.with_name("transcript_full.jsonl")
    if wanted and full_path.is_file():
        full = {}
        try:
            with full_path.open("rb") as f:
                for line in f:
                    marker = line.find(b'"step_index":')
                    if marker < 0:
                        continue
                    digits = re.match(rb'"step_index":\s*(\d+)', line[marker:])
                    if not digits or int(digits.group(1)) not in wanted:
                        continue
                    try:
                        row = json.loads(line)
                    except ValueError:
                        continue
                    if isinstance(row, dict):
                        full[row.get("step_index")] = row
        except OSError:
            full = {}
        rows = [full.get(row.get("step_index"), row) if row.get("truncated_fields") else row for row in rows]
    for row in rows:
        if True:
            kind = row.get("type")
            step_idx = row.get("step_index", len(messages))
            if kind == "USER_INPUT":
                pending_tool_ids.clear()
                txt = without_relay_context(_strip_tags(row.get("content", "")))
                if txt:
                    messages.append({"role": "user", "text": txt, "uuid": f"agy-u-{step_idx}", "backend": "agy"})
            elif kind == "PLANNER_RESPONSE":
                pending_tool_ids.clear()
                txt = str(row.get("content") or "").strip()
                if txt:
                    messages.append({"role": "assistant", "text": txt, "uuid": f"agy-a-{step_idx}", "backend": "agy"})
                for idx, tc in enumerate(row.get("tool_calls") or []):
                    if not isinstance(tc, dict):
                        continue
                    tid = f"agy-t-{step_idx}-{idx}"
                    pending_tool_ids.append(tid)
                    messages.append({
                        "role": "tool",
                        "tool": str(tc.get("name") or "Действие"),
                        "input": tc.get("args") if isinstance(tc.get("args"), dict) else {},
                        "tool_id": tid,
                        "backend": "agy",
                    })
            elif kind == "GENERIC" and pending_tool_ids:
                tid = pending_tool_ids.popleft()
                out = str(row.get("content") or "")[:24000]
                messages.append({
                    "role": "tool_result",
                    "text": out,
                    "failed": row.get("status") == "ERROR",
                    "tool_id": tid,
                    "backend": "agy",
                })
    return history_window(messages)


def delete_native_session(sid):
    low = str(sid or "").lower()
    if not _valid_uuid(low):
        return
    add_tombstone(low)
    folder = agy_data_dir() / "brain" / low
    if folder.is_dir() and not folder.is_symlink():
        archive = agy_data_dir() / "brain/.relay-deleted"
        try:
            archive.mkdir(parents=True, mode=0o700, exist_ok=True)
            folder.rename(archive / f"{low}-{uuid.uuid4().hex[:6]}")
        except OSError:
            pass


def _agy_mode_id(mode):
    if mode == "auto":
        return "auto"
    if mode == "acceptEdits":
        return "accept-edits"
    if mode == "plan":
        return "plan"
    return "default"


class AgyRun:
    backend = "agy"

    def __init__(self, owner, sid, cwd, model, mode, resume, effort="default"):
        binary = agy_binary()
        if not binary:
            raise ValueError("Antigravity CLI (`agy`) не найден на этом сервере. Установи `agy` или выбери другой агент.")
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
        self.session_id = None
        self.available_modes = set()
        self.inbox = collections.deque()
        self.condition = threading.Condition()

        self.stream_started = False
        self.current_text = []
        self.turn_blocks = []
        self.seen_tool_calls = set()

        existing = owner.transcripts.find(sid)
        self.is_pure_native = bool(existing is None and resume and has_native_session(sid))
        self.path = existing or (owner.transcripts.root / re.sub(r"[^a-zA-Z0-9-]", "-", cwd) / (sid + ".jsonl"))
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.parent = None
        self.linked_session_id = None
        self.prior_handoff = ""
        self._inspect_shared_transcript(resume)

        self.mcp_servers, self.system_prompt = self._build_environment()
        env = dict(os.environ)
        self.process = subprocess.Popen(
            [self.binary, "acp", "--stdio"],
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
            self.linked_session_id = next((row["agySessionId"] for row in reversed(self.shared_rows)
                                          if isinstance(row.get("agySessionId"), str)), None)
        elif resume and has_native_session(self.sid):
            self.linked_session_id = self.sid

    def _load_relay_context(self, resumed):
        self.prior_handoff = native_handoff(self.shared_rows, self.backend, "agySessionId", self.session_id, resumed)
        self.shared_rows = []

    def _append_shared(self, role, content, context_complete=False, usage=None):
        ident = str(uuid.uuid4())
        message = {"role": role, "content": content}
        if role == "assistant":
            message.update(
                id="msg_agy_" + uuid.uuid4().hex,
                type="message",
                model=self.model,
                stop_reason="end_turn",
                stop_sequence=None,
                usage=usage or {},
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
            "relayBackend": "agy",
            "relayModel": self.model,
            "relayMode": self.mode,
            "relayEffort": self.effort,
            "relayActor": getattr(self.owner, "actor", ""),
            "relayAgentName": (self.cluster_context or {}).get("name", ""),
            "message": message,
        }
        if usage:
            row["relayReportedUsage"] = True
        if self.session_id:
            row["agySessionId"] = self.session_id
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
        if self.agent_role in AGENT_ROLES:
            prompt += (" " if prompt else "") + AGENT_ROLES[self.agent_role]
        mcp_list = []

        def add_mcp(name, command, args, env_dict):
            mcp_list.append({
                "name": name,
                "command": command,
                "args": list(args or []),
                "env": [{"name": str(k), "value": str(v)} for k, v in (env_dict or {}).items()],
            })

        if self.cluster_context.get("role") == "leader" and getattr(self.owner, "cluster", None):
            cfg = self.owner.cluster.config(self.sid, self.cluster_context["run"])
            add_mcp("relay_cluster", cfg["command"], cfg.get("args", []), cfg.get("env", {}))
            prompt += " " + getattr(self.owner, "cluster_instructions", "")
        elif self.cluster_context.get("role") == "worker":
            prompt += (
                " You are a worker directed by the lead agent of this Relay cluster. Complete only the assigned task "
                "and constraints; report concrete changes, artifacts, failures and checks to the lead."
            )
        if getattr(self.owner, "board_file", None) and self.cluster_context.get("role") != "worker":
            actor = getattr(self.owner, "actor", "") or "Relay"
            add_mcp(
                "relay_board",
                os.sys.executable,
                [str(self.owner.board_file), "mcp"],
                {"RELAY_BOARD_ACTOR": f"AGY · {actor}", "RELAY_BOARD_NAME": "sbrp"},
            )
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
                    add_mcp(name, spec["command"], spec.get("args", []), spec.get("env", {}))
        if self.custom_prompt:
            prompt += ("\n\n" if prompt.strip() else "") + self.custom_prompt
        return mcp_list, prompt.strip()[:110000]

    def _emit(self, data):
        self.owner.send({"type": "claude", "sid": self.sid, "data": data})

    def _send_json(self, payload):
        with self.lock:
            if self.closed or self.process.poll() is not None:
                raise ValueError("AGY завершил работу. Отправь сообщение ещё раз.")
            self.process.stdin.write(json.dumps(payload, ensure_ascii=False) + "\n")
            self.process.stdin.flush()

    def _rpc(self, method, params=None, timeout=90):
        with self.lock:
            self.rpc_seq += 1
            req_id = self.rpc_seq
            ev = threading.Event()
            box = {}
            self.rpc_waiters[req_id] = (ev, box)
        try:
            self._send_json({"jsonrpc": "2.0", "id": req_id, "method": method, "params": params or {}})
            if not ev.wait(timeout):
                raise ValueError(f"AGY не ответил на запрос {method}.")
            if "error" in box:
                err = box["error"]
                code = err.get("code") if isinstance(err, dict) else None
                msg = err.get("message") if isinstance(err, dict) else str(err)
                if code == -32000 or "authentication required" in str(msg).lower() or "sign in" in str(msg).lower():
                    self.auth_failed = True
                    raise ValueError(
                        "Antigravity (`agy`) на сервере требует входа. Нажми «Войти в AGY» в карточке сервера или выполни вход."
                    )
                raise ValueError(clean_error(msg or "Ошибка AGY."))
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
                    box["error"] = {"message": "\n".join(self.error)[-800:] or "AGY завершил работу."}
                    ev.set()
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
        req_id = f"agy-perm-{rpc_id}-{uuid.uuid4().hex[:8]}"
        try:
            if method == "session/request_permission":
                options = params.get("options") or []
                allow_opt = next(
                    (o.get("optionId") for o in options if isinstance(o, dict) and o.get("kind") in ("allow_once", "allow_always")),
                    options[0].get("optionId") if options and isinstance(options[0], dict) else "allow_once",
                )
                reject_opt = next(
                    (o.get("optionId") for o in options if isinstance(o, dict) and str(o.get("kind") or "").startswith("reject")),
                    None,
                )
                tc = params.get("toolCall") or {}
                title = str(tc.get("title") or tc.get("kind") or "Действие")
                raw_in = tc.get("rawInput") if isinstance(tc.get("rawInput"), dict) else {"description": title}
                low_title = title.lower()
                is_cluster_tool = (
                    "relay_cluster" in low_title
                    or low_title in ("list_workers", "create_worker", "remove_worker", "dispatch_task", "task_status", "wait_for_task", "cancel_task", "cluster_status")
                )

                if self.cluster_context.get("role") == "worker" and low_title in ("ask_question", "askuserquestion"):
                    outcome = {"outcome": "selected", "optionId": reject_opt} if reject_opt else {"outcome": "cancelled"}
                    self._send_json({"jsonrpc": "2.0", "id": rpc_id, "result": {"outcome": outcome}})
                    return

                auto_allow = (
                    self.mode == "auto"
                    or "relay_board" in low_title
                    or (self.cluster_context.get("role") == "leader" and is_cluster_tool)
                    or (self.desktop_enabled and "relay_desktop" in low_title)
                    or (self.mode == "acceptEdits" and tc.get("kind") in ("edit", "write"))
                )
                if auto_allow:
                    self._send_json({"jsonrpc": "2.0", "id": rpc_id, "result": {"outcome": {"outcome": "selected", "optionId": allow_opt}}})
                    return

                ev = threading.Event()
                entry = {"tool_name": title, "input": raw_in, "event": ev, "allow": False}
                self.pending[req_id] = entry
                self.owner.send({"type": "permission", "sid": self.sid, "id": req_id, "tool": title, "input": raw_in})
                try:
                    end = time.monotonic() + 1200
                    while not ev.wait(0.2):
                        if self.closed or self.process.poll() is not None or time.monotonic() > end:
                            break
                    allowed = bool(entry["allow"])
                finally:
                    self.pending.pop(req_id, None)
                    self.owner.send({"type": "permission_cancelled", "sid": self.sid, "id": req_id})

                if allowed:
                    outcome = {"outcome": "selected", "optionId": allow_opt}
                elif reject_opt:
                    outcome = {"outcome": "selected", "optionId": reject_opt}
                else:
                    outcome = {"outcome": "cancelled"}
                self._send_json({"jsonrpc": "2.0", "id": rpc_id, "result": {"outcome": outcome}})
            else:
                self._send_json({"jsonrpc": "2.0", "id": rpc_id, "error": {"code": -32601, "message": "Unsupported client method"}})
        except Exception:
            pass

    def _flush_text_block(self):
        if self.current_text:
            full = "".join(self.current_text)
            self.current_text.clear()
            self.stream_started = False
            block = {"type": "text", "text": full}
            self.turn_blocks.append(("assistant", [block]))
            self._emit({"type": "assistant", "message": {"role": "assistant", "content": [block]}})
            return full
        return ""

    def _handle_notification(self, method, params):
        if method != "session/update":
            return
        upd = params.get("update") or {}
        kind = upd.get("sessionUpdate")
        if kind == "usage_update" and params.get("sessionId") == self.session_id:
            used, limit = upd.get("used"), upd.get("size")
            if type(used) is int and used >= 0 and type(limit) is int and limit > 0:
                self.owner.send({"type": "turn_summary", "sid": self.sid, "backend": self.backend,
                                 "used": used, "limit": limit})
        elif kind == "agent_message_chunk":
            content = upd.get("content") or {}
            txt = content.get("text") if isinstance(content, dict) else ""
            if isinstance(txt, str) and txt:
                if not self.stream_started:
                    self.stream_started = True
                    self._emit({"type": "stream_event", "event": {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}})
                self.current_text.append(txt)
                self._emit({"type": "stream_event", "event": {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": txt}}})
        elif kind == "tool_call":
            self._flush_text_block()
            tid = str(upd.get("toolCallId") or uuid.uuid4().hex)
            if tid in self.seen_tool_calls:
                return
            self.seen_tool_calls.add(tid)
            title = str(upd.get("title") or upd.get("kind") or "Действие")
            raw_in = upd.get("rawInput") if isinstance(upd.get("rawInput"), dict) else {"description": title}
            block = {"type": "tool_use", "id": tid, "name": title, "input": raw_in}
            self.turn_blocks.append(("assistant", [block]))
            self._emit({"type": "assistant", "message": {"role": "assistant", "content": [block]}})
        elif kind == "tool_call_update":
            status = upd.get("status")
            if status in ("completed", "failed"):
                tid = str(upd.get("toolCallId") or "")
                texts = []
                for c in upd.get("content") or []:
                    if isinstance(c, dict):
                        inner = c.get("content") if isinstance(c.get("content"), dict) else c
                        if isinstance(inner.get("text"), str):
                            texts.append(inner["text"])
                        elif c.get("type") == "diff" and c.get("path"):
                            texts.append(f"Изменён файл: {c['path']}")
                if not texts and upd.get("rawOutput") is not None:
                    raw = upd["rawOutput"]
                    texts.append(raw if isinstance(raw, str) else json.dumps(raw, ensure_ascii=False))
                out = "\n".join(texts)[:24000] or ("Ошибка." if status == "failed" else "Выполнено.")
                res = {"type": "tool_result", "tool_use_id": tid, "content": out, "is_error": status == "failed"}
                self.turn_blocks.append(("user", [res]))
                self._emit({"type": "user", "message": {"role": "user", "content": [res]}})
        elif kind == "available_commands_update":
            cmds = [
                {"name": str(c.get("name") or ""), "description": str(c.get("description") or "")[:240], "hint": ""}
                for c in (upd.get("availableCommands") or [])
                if isinstance(c, dict) and c.get("name")
            ]
            if cmds:
                self.owner.learn_backend_catalog("agy", None, agy_version(self.binary) or "", commands=cmds)

    def _learn_session_state(self, res):
        modes = (res.get("modes") or {}).get("availableModes") or []
        for m in modes:
            if isinstance(m, dict) and isinstance(m.get("id"), str):
                self.available_modes.add(m["id"])
        models_raw = (res.get("models") or {}).get("availableModels") or []
        models = []
        for m in models_raw:
            if isinstance(m, dict) and isinstance(m.get("modelId") or m.get("id"), str):
                mid = m.get("modelId") or m.get("id")
                models.append({
                    "value": mid,
                    "displayName": str(m.get("name") or mid),
                    "description": str(m.get("description") or "")[:240],
                })
        if models:
            self.owner.learn_backend_catalog("agy", models, agy_version(self.binary) or "")

    def _apply_session_settings(self):
        if not self.session_id:
            return
        if self.model and self.model not in ("default", "inherit"):
            try:
                self._rpc("session/set_model", {"sessionId": self.session_id, "modelId": self.model}, timeout=15)
            except Exception:
                pass
        desired_mode = _agy_mode_id(self.mode)
        if not self.available_modes or desired_mode in self.available_modes:
            try:
                self._rpc("session/set_mode", {"sessionId": self.session_id, "modeId": desired_mode}, timeout=15)
            except Exception:
                pass

    def _ensure_session(self):
        if self.session_id:
            return
        self._rpc("initialize", {
            "protocolVersion": 1,
            "clientCapabilities": {"fs": {"readTextFile": False, "writeTextFile": False}, "terminal": False},
            "clientInfo": {"name": "relay-desktop", "version": "0.20.0"},
        }, timeout=30)
        base_params = {"cwd": self.cwd, "mcpServers": self.mcp_servers}
        if self.system_prompt:
            base_params["_meta"] = {"antigravity": {"systemPrompt": {"append": self.system_prompt}}}

        if self.linked_session_id and has_native_session(self.linked_session_id):
            try:
                res = self._rpc("session/resume", {"sessionId": self.linked_session_id, **base_params}, timeout=45)
                self.session_id = res.get("sessionId") or self.linked_session_id
                self._load_relay_context(resumed=True)
                self._learn_session_state(res)
                self._apply_session_settings()
                return
            except Exception:
                pass

        res = self._rpc("session/new", base_params, timeout=45)
        self.session_id = res.get("sessionId")
        if not self.session_id:
            raise ValueError("Antigravity не создал сессию. Проверь вход на сервере.")
        self._load_relay_context(resumed=False)
        self.linked_session_id = self.session_id
        self._learn_session_state(res)
        self._apply_session_settings()

    def prompt(self, text, images=None, steer=False):
        with self.condition:
            if self.closed:
                raise ValueError("Сессия AGY закрыта. Отправь сообщение ещё раз.")
            if len(self.inbox) >= 12:
                raise ValueError("В очереди AGY уже 12 сообщений.")
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

            self.stream_started = False
            self.current_text.clear()
            self.turn_blocks.clear()
            self.seen_tool_calls.clear()
            error = None
            final_text = ""
            try:
                self._ensure_session()
                self._apply_session_settings()
                mirror = not self.is_pure_native
                if mirror:
                    self._append_shared(
                        "user",
                        ([{"type": "text", "text": text}] if text else [])
                        + [{"type": "image", "source": img} for img in images],
                    )
                self._emit({"type": "system", "subtype": "init", "session_id": self.sid, "model": self.model, "relay_backend": "agy"})
                blocks = []
                if self.prior_handoff or text:
                    blocks.append({"type": "text", "text": self.prior_handoff + text})
                for img in images or []:
                    if isinstance(img, dict) and img.get("data"):
                        blocks.append({
                            "type": "image",
                            "mimeType": img.get("media_type") or "image/png",
                            "data": img["data"],
                        })
                response = self._rpc("session/prompt", {"sessionId": self.session_id, "prompt": blocks or [{"type": "text", "text": ""}]}, timeout=1800)
                raw_usage = response.get("usage")
                usage = {}
                if isinstance(raw_usage, dict):
                    usage = {
                        transcript_key: raw_usage[acp_key]
                        for transcript_key, acp_key in (
                            ("input_tokens", "inputTokens"),
                            ("output_tokens", "outputTokens"),
                            ("cache_read_input_tokens", "cachedReadTokens"),
                            ("cache_creation_input_tokens", "cachedWriteTokens"),
                        )
                        if type(raw_usage.get(acp_key)) is int and raw_usage[acp_key] >= 0
                    }
                flushed = self._flush_text_block()
                texts = [b["text"] for role, blks in self.turn_blocks if role == "assistant" for b in blks if b.get("type") == "text"]
                final_text = "\n\n".join(texts).strip() or flushed
                if mirror:
                    for i, (role, blks) in enumerate(self.turn_blocks):
                        last = i == len(self.turn_blocks) - 1
                        self._append_shared(role, blks, context_complete=last and (role == "assistant" or not usage),
                                            usage=usage if last and role == "assistant" else None)
                    if usage and (not self.turn_blocks or self.turn_blocks[-1][0] != "assistant"):
                        self._append_shared("assistant", [], context_complete=True, usage=usage)
                self.prior_handoff = ""
            except Exception as exc:
                self._flush_text_block()
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

            self._emit({
                "type": "result",
                "subtype": "error_during_execution" if error else "success",
                "is_error": bool(error),
                "result": error or final_text,
                "errors": [error] if error else [],
                "session_id": self.sid,
                "relay_backend": "agy",
            })
            self.owner.run_state(self)

    def control(self, subtype, **fields):
        if subtype == "set_model":
            self.model = fields.get("model") or "default"
            if self.session_id and self.model not in ("default", "inherit"):
                threading.Thread(
                    target=lambda: self._safe_rpc("session/set_model", {"sessionId": self.session_id, "modelId": self.model}),
                    daemon=True,
                ).start()
        elif subtype == "set_permission_mode":
            self.mode = fields.get("mode") or "manual"
            if self.session_id:
                mode_id = _agy_mode_id(self.mode)
                threading.Thread(
                    target=lambda: self._safe_rpc("session/set_mode", {"sessionId": self.session_id, "modeId": mode_id}),
                    daemon=True,
                ).start()

    def _safe_rpc(self, method, params):
        try:
            self._rpc(method, params, timeout=10)
        except Exception:
            pass

    def permit(self, request_id, allow, answers):
        entry = self.pending.get(request_id)
        if entry is None:
            raise ValueError("Этот запрос уже закрыт.")
        entry["allow"] = bool(allow)
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
        if self.session_id:
            try:
                self._send_json({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": self.session_id}})
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
                except ProcessLookupError:
                    pass
            except ProcessLookupError:
                pass
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            if stream is not None:
                try:
                    stream.close()
                except OSError:
                    pass
