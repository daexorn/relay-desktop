"""Relay's server helper. Python stdlib only.

The app passes this file to python3 over an authenticated SSH channel. A new app
also passes the whole source as RELAY_SOURCE: the helper then starts (or joins) a
host process of the same version that owns the Claude processes, listening on a
user-only Unix socket in ~/.cache/relay-desktop/host. Work then continues when the
app disconnects, and every app that connects sees the same running sessions. The
host exits after it has been idle without apps for a while. Existing Claude
transcripts are read; explicit session deletion moves only that session out of the
active history. Project files and Claude settings are never changed.
"""
import collections
import base64
import fcntl
import json
import os
from pathlib import Path
import re
import shutil
import socket
import signal
import subprocess
import sys
import threading
import time
import uuid
import datetime
import math
import hashlib
import stat
import urllib.request
import urllib.error

MAX_REQUEST = 26 * 1024 * 1024
MAX_HISTORY = 16 * 1024 * 1024
WRITE_LOCK = threading.Lock()

AGENT_ROLES = {
    "worker": "You are the development agent for this task. Work in the selected host and directory, respect project instructions and concurrent edits, implement the requested change and report files and actual verification results.",
    "planner": "You are the planning agent for this task. Inspect the project, identify dependencies and propose concrete implementation steps. Do not change files unless the user explicitly requests it. Report facts and open questions.",
    "reviewer": "You are the review agent for this task. Inspect relevant changes and run appropriate checks permitted by the user's instructions. Report findings with file references and evidence. Do not claim checks you did not run.",
}

USAGE_LABELS = {"five_hour": "5 часов", "seven_day": "Неделя", "seven_day_opus": "Неделя · Opus", "seven_day_sonnet": "Неделя · Sonnet", "overage": "Дополнительно"}


def usage_number(value, maximum):
    return float(value) if isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value) and 0 <= value <= maximum else None


def usage_time(value):
    number = usage_number(value, 253402300799)
    if number is not None:
        return int(number)
    if isinstance(value, str):
        try:
            date = datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
            if date.tzinfo is None:
                date = date.replace(tzinfo=datetime.timezone.utc)
            return int(date.timestamp())
        except (ValueError, OverflowError):
            pass
    return None


class UsageNoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class UsageState:
    """Whitelist-only account usage, never credentials. No login/refresh actions.

    Stream fields follow the official Agent SDK RateLimitInfo contract. The
    fixed OAuth endpoint is a best-effort adapter used by the first-party CLI;
    it is not a public versioned API. Failure preserves observed stream data.
    """
    def __init__(self):
        self.lock = threading.Lock()
        self.windows = {}
        self.status = None
        self.status_until = None
        self.source = None
        self.updated_at = 0
        self.message = "Claude пока не передал данные о лимитах."
        self.inflight = False
        self.last_attempt = None

    def snapshot(self):
        now = time.time()
        with self.lock:
            windows = [dict(window) for window in self.windows.values() if window["resets_at"] is None or window["resets_at"] > now]
            status = self.status if self.status_until is None or self.status_until > now else None
            return {"type": "usage", "available": bool(windows or status), "windows": windows, "status": status,
                    "source": self.source, "message": self.message, "updated_at": self.updated_at}

    def observe(self, data):
        if data.get("type") != "rate_limit_event":
            return False
        info = data.get("rate_limit_info", {})
        if not isinstance(info, dict):
            return False
        status = info.get("status")
        kind = info.get("rateLimitType", info.get("rate_limit_type"))
        reset = usage_time(info.get("resetsAt", info.get("resets_at")))
        fraction = usage_number(info.get("utilization"), 1)
        with self.lock:
            if status in ("allowed", "allowed_warning", "rejected"):
                self.status, self.status_until = status, reset
            if isinstance(kind, str) and kind in USAGE_LABELS:
                self.windows[kind] = {"id": kind, "label": USAGE_LABELS[kind], "utilization": fraction * 100 if fraction is not None else None, "resets_at": reset}
            self.source = "claude_stream"
            self.updated_at = int(time.time())
            self.message = "Последние данные Claude Code."
        return True

    def refresh_async(self, send):
        now = time.monotonic()
        with self.lock:
            if self.inflight or self.last_attempt is not None and now - self.last_attempt < 60:
                return
            self.inflight, self.last_attempt = True, now
        def fetch():
            try:
                self.fetch_oauth()
            finally:
                with self.lock:
                    self.inflight = False
                send(self.snapshot())
        threading.Thread(target=fetch, daemon=True).start()

    def fetch_oauth(self):
        try:
            config = Path(os.environ.get("CLAUDE_CONFIG_DIR", str(Path.home() / ".claude"))).expanduser()
            path = config / ".credentials.json"
            if not path.is_file() or path.stat().st_size > 512 * 1024:
                return
            oauth = json.loads(path.read_text()).get("claudeAiOauth", {})
            token = oauth.get("accessToken")
            if not isinstance(token, str) or not token or len(token) > 8192 or "\r" in token or "\n" in token:
                return
            expires = usage_number(oauth.get("expiresAt"), 253402300799000)
            if expires is None or expires <= time.time() * 1000 or oauth.get("subscriptionType") not in ("pro", "max", "team", "enterprise"):
                return
            scopes = oauth.get("scopes", [])
            if not isinstance(scopes, list) or "user:profile" not in scopes:
                return
            request = urllib.request.Request("https://api.anthropic.com/api/oauth/usage", headers={"Authorization": "Bearer " + token, "anthropic-beta": "oauth-2025-04-20", "User-Agent": "Relay-desktop", "Accept": "application/json"})
            opener = urllib.request.build_opener(UsageNoRedirect())
            with opener.open(request, timeout=6) as response:
                body = response.read(262145)
            if len(body) > 262144:
                return
            data = json.loads(body)
            windows = {}
            for kind, label in USAGE_LABELS.items():
                window = data.get(kind)
                if isinstance(window, dict):
                    percentage = usage_number(window.get("utilization"), 100)
                    reset = usage_time(window.get("resets_at"))
                    if percentage is not None or reset is not None:
                        windows[kind] = {"id": kind, "label": label, "utilization": percentage, "resets_at": reset}
            if windows:
                with self.lock:
                    self.windows.update(windows)
                    self.source = "claude_oauth"
                    self.updated_at = int(time.time())
                    self.message = "Данные подписки Claude."
        except Exception:
            # Neither HTTP response bodies nor exception text can reach the GUI.
            with self.lock:
                if not self.windows:
                    self.message = "Не удалось получить лимиты подписки. Данные обновятся после ответа Claude."


def emit(event):
    with WRITE_LOCK:
        sys.stdout.write(json.dumps(event, ensure_ascii=False, separators=(",", ":")) + "\n")
        sys.stdout.flush()


def valid_id(value):
    if not isinstance(value, str) or str(uuid.UUID(value)) != value.lower():
        raise ValueError("Некорректная сессия.")
    return value.lower()


def directory(value):
    p = Path(str(value or "~")).expanduser().resolve()
    if not p.is_dir():
        raise ValueError("Этой папки больше нет или к ней нет доступа.")
    return p


def plain(content):
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(b["text"] for b in content if isinstance(b, dict) and b.get("type") == "text" and isinstance(b.get("text"),str))
    return ""


def records(path, limit=MAX_HISTORY):
    """Bound memory and tolerate partially written lines while Claude is running."""
    with path.open("rb") as f:
        size = os.fstat(f.fileno()).st_size
        if size > limit:
            f.seek(size - limit)
            f.readline()
        for line in f:
            try:
                row = json.loads(line)
                if isinstance(row, dict):
                    yield row
            except (ValueError, UnicodeError):
                continue


def visible_text(text):
    # Claude's own terminal/status traffic is not a user conversation.
    if not text.strip() or text.lstrip().startswith(("<local-command", "<command-name>", "<system-reminder>", "<task-notification>")):
        return ""
    if re.fullmatch(r"(?:claude\s+)?(?:--resume|—resume|/resume|-r|--continue|-c)(?:\s+[0-9a-fA-F-]{36})?\s*", text.strip()):
        return ""
    match = re.search(r"<relay-files>\s*(.*?)\s*</relay-files>", text, flags=re.S)
    if match:
        try:
            names = [str(item.get("name", "Файл")) for item in json.loads(match.group(1))]
            text = text[:match.start()].rstrip() + "\n\nФайлы: " + ", ".join(names)
        except (ValueError, TypeError):
            pass
    return text


class Transcripts:
    def __init__(self, root=None):
        config = Path(os.environ.get("CLAUDE_CONFIG_DIR", str(Path.home() / ".claude"))).expanduser()
        self.root = Path(root) if root else config / "projects"
        self.cache = {}

    def files(self):
        # Only primary conversations. Subagent transcripts stay inside their parent.
        return [p for p in self.root.glob("*/*.jsonl") if re.fullmatch(r"[0-9a-fA-F-]{36}", p.stem)]

    def find(self, session):
        session=valid_id(session)
        return next((p for p in self.files() if p.stem.lower()==session),None)

    def path(self, session):
        found = self.find(session)
        if found is None:
            raise ValueError("Сессия не найдена. Обнови список разговоров.")
        return found

    def metadata(self, path):
        stat = path.stat()
        key = (stat.st_mtime_ns, stat.st_size)
        cached = self.cache.get(str(path))
        if cached and cached[0] == key:
            return cached[1]
        rows = []
        with path.open("rb") as f:
            chunks = [f.read(256 * 1024)]
            if stat.st_size > 256 * 1024:
                f.seek(max(0, stat.st_size - 128 * 1024))
                f.readline()
                chunks.append(f.read(128 * 1024))
        for chunk in chunks:
            for line in chunk.splitlines():
                try:
                    row = json.loads(line)
                    if isinstance(row, dict):
                        rows.append(row)
                except (ValueError, UnicodeError):
                    pass
        cwd = next((r["cwd"] for r in reversed(rows) if isinstance(r.get("cwd"), str)), str(Path.home()))
        cwd = str(Path(cwd).expanduser().resolve())
        title = next((r.get("customTitle") or r.get("custom_title") for r in reversed(rows) if r.get("type") == "custom-title"), None)
        if not title:
            title = next((visible_text(plain((r.get("message") if isinstance(r.get("message"),dict) else {}).get("content"))) for r in rows
                          if r.get("type") == "user" and not r.get("isSidechain") and visible_text(plain((r.get("message") if isinstance(r.get("message"),dict) else {}).get("content")))), "Новый разговор")
        title = visible_text(str(title)) if title else ""
        if not title or title == "Новый разговор":
            title = next((visible_text(plain((r.get("message") if isinstance(r.get("message"),dict) else {}).get("content"))) for r in records(path, limit=8*1024*1024)
                          if r.get("type") == "user" and not r.get("isSidechain") and visible_text(plain((r.get("message") if isinstance(r.get("message"),dict) else {}).get("content")))), "Разговор от " + time.strftime("%d.%m", time.localtime(stat.st_mtime)))
        title = " ".join(str(title).split())[:100]
        result = {"id": path.stem, "title": title, "cwd": cwd, "modified": stat.st_mtime, "messages": 0}
        self.cache[str(path)] = (key, result)
        return result

    def sessions(self):
        result = []
        for path in self.files():
            try:
                result.append(self.metadata(path))
            except (OSError, ValueError, TypeError, AttributeError, UnicodeError):
                continue
        result.sort(key=lambda r: r["modified"], reverse=True)
        return result[:2000]

    def delete(self, session):
        """Remove one session from Claude's index, retaining a private recovery copy."""
        session = valid_id(session)
        path = self.find(session)
        if path is None:
            return
        if not stat.S_ISREG(path.lstat().st_mode) or not path.parent.resolve().is_relative_to(self.root.resolve()):
            raise ValueError("Некорректный файл сессии.")
        archive = path.parent / '.relay-deleted' / uuid.uuid4().hex
        archive.mkdir(parents=True, mode=0o700)
        moved = []
        try:
            paths = [path]
            children = path.with_suffix('')
            if children.is_dir() and not children.is_symlink():
                paths.append(children)
            for source in paths:
                target = archive / source.name
                source.rename(target)
                moved.append((source, target))
        except OSError:
            for source, target in reversed(moved):
                target.rename(source)
            raise
        self.cache.pop(str(path), None)

    def history(self, session):
        path = self.path(session)
        # Follow the last primary branch, excluding abandoned assistant retries.
        nodes = collections.OrderedDict()
        parents = {}
        for row in records(path):
            if row.get("uuid") and not row.get("isSidechain"):
                parents[row["uuid"]] = row.get("parentUuid")
            if row.get("type") in ("user", "assistant") and not row.get("isSidechain"):
                nodes[row.get("uuid") or str(len(nodes))] = row
        chain = []
        if nodes:
            key = next(reversed(nodes))
            seen = set()
            while key in parents and key not in seen:
                seen.add(key)
                if key in nodes:
                    chain.append(nodes[key])
                key = parents[key]
            chain.reverse()
        if not chain and nodes:
            chain = list(nodes.values())
        result = []
        image_budget = MAX_HISTORY_IMAGES
        for row in reversed(chain):
            # Images are sent newest first, so a long history keeps its latest pictures.
            content = (row.get("message") or {}).get("content") if isinstance(row.get("message"), dict) else None
            if isinstance(content, list):
                for b in content:
                    if isinstance(b, dict) and b.get("type") == "image":
                        data = (b.get("source") or {}).get("data") or ""
                        if 0 < len(data) <= image_budget:
                            image_budget -= len(data)
                            b["_relay_keep"] = True
        command = None
        for row in chain:
            # Claude's own bookkeeping (image paths, interrupt markers) is not conversation.
            if row.get("isMeta"):
                continue
            message = row.get("message") or {}
            if not isinstance(message,dict):continue
            role = message.get("role", row.get("type"))
            content = message.get("content", "")
            # Slash commands (/model, /context…) become one action: the command and what it printed.
            raw = plain(content).strip() if role == "user" else ""
            if raw.startswith("<command-name>"):
                name = re.search(r"<command-name>(.*?)</command-name>", raw, re.S)
                args = re.search(r"<command-args>(.*?)</command-args>", raw, re.S)
                line = ((name.group(1).strip() if name else "") + " " + (args.group(1).strip() if args else "")).strip()
                if line:
                    command = {"role": "tool", "tool": "SlashCommand", "input": {"command": line}, "tool_id": "cmd-" + str(row.get("uuid", ""))}
                    result.append(command)
                continue
            if raw.startswith(("<local-command-stdout>", "<local-command-stderr>")):
                output = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", re.sub(r"</?local-command-std(?:out|err)>", "", raw)).strip()
                if command is not None:
                    result.append({"role": "tool_result", "text": output[:24000], "failed": raw.startswith("<local-command-stderr>"), "tool_id": command["tool_id"]})
                    command = None
                continue
            text = visible_text(plain(content)) if role == "user" else plain(content)
            if role == "user" and INTERRUPTED.fullmatch(text.strip()):
                continue
            if role == "assistant" and text.strip() == "No response requested.":
                continue
            images = []
            if role == "user" and isinstance(content, list):
                for b in content:
                    if isinstance(b, dict) and b.get("type") == "image":
                        source = b.get("source") or {}
                        images.append({"media_type": source.get("media_type", "image/png"),
                                       "data": source.get("data", "") if b.pop("_relay_keep", False) else ""})
            if text or images:
                entry = {"role": role, "text": text, "uuid": row.get("uuid", "")}
                if images:
                    entry["images"] = images
                result.append(entry)
            if isinstance(content, list):
                for b in content:
                    if not isinstance(b, dict):
                        continue
                    if b.get("type") == "tool_use":
                        result.append({"role": "tool", "tool": b.get("name", "Действие"), "input": b.get("input", {}), "tool_id": b.get("id", "")})
                    elif b.get("type") == "tool_result":
                        output = plain(b.get("content", ""))
                        result.append({"role": "tool_result", "text": output[:24000], "failed": bool(b.get("is_error")), "tool_id": b.get("tool_use_id", "")})
        return result[-600:]


def load_board():
    """The board module arrives with the bridge; agents get it as a file to run as MCP or CLI."""
    source = globals().get("BOARD_SOURCE")
    if not source:
        return None, None
    namespace = {"__name__": "relay_board"}
    exec(source, namespace)
    # Next to the data, so terminal agents (`relay-board`) run the same version as Relay.
    def version(text):
        match = re.search(r"^BOARD_VERSION = (\d+)", text, re.M)
        return int(match.group(1)) if match else 0
    for path in (namespace["root"]() / "board.py", Path.home() / ".cache/relay-desktop/board.py"):
        try:
            path.parent.mkdir(parents=True, exist_ok=True)
            existing = path.read_text(encoding="utf-8") if path.exists() else ""
            if version(existing) < namespace.get("BOARD_VERSION", 0) or not existing:
                path.write_text(source, encoding="utf-8")
                path.chmod(0o664)
            return namespace, path
        except OSError:
            continue
    return namespace, None


INTERRUPTED = re.compile(r"\[Request interrupted by user(?: for tool use)?\]")
# Total base64 of pictures sent with one history; older ones become placeholders.
MAX_HISTORY_IMAGES = 12 * 1024 * 1024


class SessionMeta:
    """Session names, pins and folders shared by everyone who connects to this server.
    Stored next to the team board; Claude's own transcripts are never rewritten."""

    def __init__(self, path=None):
        if path:
            self.path = Path(path)
        else:
            shared = Path(os.environ.get("RELAY_BOARD_DIR", "/opt/relay-board"))
            self.path = shared / "sessions.json"
            if not (shared.is_dir() and os.access(shared, os.W_OK)):
                self.path = Path.home() / ".config/relay-desktop/sessions.json"

    def read(self):
        try:
            data = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            data = {}
        if not isinstance(data, dict):
            data = {}
        data.setdefault("folders", [])
        data.setdefault("sessions", {})
        return data

    def update(self, change):
        self.path.parent.mkdir(parents=True, exist_ok=True)
        with open(self.path.with_suffix(".lock"), "a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            data = self.read()
            change(data)
            temp = self.path.with_suffix(".tmp")
            temp.write_text(json.dumps(data, ensure_ascii=False, indent=1), encoding="utf-8")
            os.chmod(temp, 0o664)
            temp.replace(self.path)

    def session(self, sid, title=None, pinned=None, folder=None, actor=""):
        def change(data):
            folders = {f["id"] for f in data["folders"]}
            meta = data["sessions"].setdefault(sid, {})
            if title is not None:
                clean = " ".join(str(title).split())[:120]
                if clean:
                    meta["title"] = clean
                else:
                    meta.pop("title", None)
            if pinned is not None:
                if pinned:
                    meta["pinned"] = time.time()
                else:
                    meta.pop("pinned", None)
            if folder is not None:
                if folder and folder not in folders:
                    raise ValueError("Такой папки нет. Обнови список сессий.")
                if folder:
                    meta["folder"] = folder
                else:
                    meta.pop("folder", None)
            meta["changed_by"] = str(actor)[:80]
            if set(meta) <= {"changed_by"}:
                data["sessions"].pop(sid, None)
        self.update(change)

    def folder(self, ident=None, name=None, delete=False, order=None):
        created = []
        def change(data):
            folders = data["folders"]
            if not ident:
                clean = " ".join(str(name or "").split())[:60]
                if not clean:
                    raise ValueError("Назови папку.")
                if any(f["name"].lower() == clean.lower() for f in folders):
                    raise ValueError(f"Папка «{clean}» уже есть.")
                new = "f-" + uuid.uuid4().hex[:8]
                folders.append({"id": new, "name": clean, "order": len(folders)})
                created.append(new)
                return
            target = next((f for f in folders if f["id"] == ident), None)
            if target is None:
                raise ValueError("Такой папки нет. Обнови список сессий.")
            if delete:
                folders.remove(target)
                for meta in data["sessions"].values():
                    if meta.get("folder") == ident:
                        meta.pop("folder", None)
                return
            if name is not None:
                clean = " ".join(str(name).split())[:60]
                if not clean:
                    raise ValueError("Назови папку.")
                target["name"] = clean
            if order is not None:
                folders.remove(target)
                folders.insert(max(0, min(int(order), len(folders))), target)
                for i, f in enumerate(folders):
                    f["order"] = i
        self.update(change)
        return created[0] if created else ident

    def apply(self, sessions):
        """Adds shared names, pins and folders to the transcript list."""
        data = self.read()
        folders = sorted(data["folders"], key=lambda f: f.get("order", 0))
        known = {f["id"] for f in folders}
        rows = []
        for row in sessions:
            meta = data["sessions"].get(row["id"], {})
            row = dict(row)
            if meta.get("title"):
                row["auto_title"] = row["title"]
                row["title"] = meta["title"]
            row["pinned"] = float(meta.get("pinned") or 0)
            row["folder"] = meta.get("folder") if meta.get("folder") in known else ""
            rows.append(row)
        return rows, folders

    def forget(self, sid):
        self.update(lambda data: data["sessions"].pop(sid, None))


def claude_binary():
    candidates = [shutil.which("claude"), str(Path.home() / ".local/bin/claude"), "/usr/local/bin/claude", "/usr/bin/claude"]
    for path in candidates:
        if path and os.path.isfile(path) and os.access(path, os.X_OK):
            return path
    raise ValueError("Claude Code не найден у этого пользователя. Установи и авторизуй его на сервере.")


def validate_effort(effort):
    if not isinstance(effort, str) or effort not in ("default", "low", "medium", "high", "xhigh", "max"):
        raise ValueError("Неизвестный уровень усилий.")
    return effort


def command(binary, sid, resume, model, mode, effort="default"):
    validate_effort(effort)
    if mode not in ("manual", "auto", "acceptEdits", "plan"):
        raise ValueError("Неизвестный режим работы.")
    if not isinstance(model, str) or len(model) > 160 or any(ord(c) < 32 for c in model):
        raise ValueError("Некорректное название модели.")
    args = [binary, "-p", "--output-format", "stream-json", "--input-format", "stream-json", "--verbose",
            "--include-partial-messages", "--permission-prompt-tool", "stdio", "--permission-mode", mode]
    args.append(("--resume=" if resume else "--session-id=") + valid_id(sid))
    if model and model != "default":
        args += ["--model", model]
    if effort != "default":
        args += ["--effort", effort]
    return args


class Run:
    def __init__(self, owner, sid, cwd, model, mode, resume, effort="default"):
        self.owner, self.sid, self.cwd = owner, sid, cwd
        self.model, self.mode, self.effort = model, mode, effort
        self.agent_role = getattr(owner, "agent_role", "")
        self.custom_prompt = getattr(owner, "custom_prompt", "")
        self.cluster_context = getattr(owner, "cluster_context", {})
        self.desktop_enabled = False
        self.desktop_context = getattr(owner,"desktop_context", "")
        self.busy = False
        self.turns = 0  # Prompts sent and not yet answered: Claude Code answers queued ones in order.
        self.background = []  # Commands Claude left running; it carries on when they finish.
        self.closed = False
        self.ready = threading.Event()
        self.pending = {}
        self.error = collections.deque(maxlen=8)
        self.lock = threading.Lock()
        self.init_id = "relay-init-" + uuid.uuid4().hex
        env = dict(os.environ)
        env.pop("CLAUDECODE", None)  # The transport is not a nested terminal conversation.
        env["CLAUDE_CODE_ENTRYPOINT"] = "relay-desktop"
        args = command(owner.binary, sid, resume, model, mode, effort)
        system_prompt="Relay supports downloadable server files in chat. When the user requests a built mod or other generated artifact, include a Markdown download link [filename](absolute_server_file_path) for the real existing file. Verify it exists before offering it. A bare filename or a statement that the build succeeded is not file delivery. Never claim the file was delivered to the PC unless the transfer tool actually succeeded. Relay chat renders LaTeX math: write formulas as $...$ inline and $$...$$ on their own line, not as Unicode approximations or code blocks."
        system_prompt += " " + AGENT_ROLES.get(self.agent_role, "")
        servers = {}
        if self.cluster_context.get("role") == "leader":
            servers["relay_cluster"] = owner.cluster.config(sid, self.cluster_context["run"])
            system_prompt += " " + owner.cluster_instructions
        elif self.cluster_context.get("role") == "worker":
            system_prompt += " You are a worker directed by the lead Claude session of this Relay cluster. Complete only the assigned task and constraints; report concrete changes, artifacts, failures and checks to the lead. If blocked, return the exact blocker and question to the lead rather than asking the user directly. Do not invent progress or delegate tasks to unconfigured agents. Other workers may be editing the same project: preserve concurrent changes. Unless the leader requests another format, finish with a concise handoff: summary, changed files/commits, artifacts, checks actually run or skipped, and unresolved issues."
        if getattr(owner, "board_file", None) and self.cluster_context.get("role")!="worker":
            actor = getattr(owner, "actor", "") or "Relay"
            servers["relay_board"] = {"type": "stdio", "command": sys.executable, "args": [str(owner.board_file), "mcp"],
                                      "env": {"RELAY_BOARD_ACTOR": f"Claude · {actor}", "RELAY_BOARD_NAME": "sbrp"}}
            system_prompt += " The team keeps a shared task board (relay_board tools; the same board is shown in Relay and available in a terminal as `relay-board` or `python3 /opt/relay-board/board.py`). Before substantial work, check board_overview and the relevant tasks; claim a task when you start it, record progress with comments, finish with board_done (or board_move to review when a person must check) and a summary of versions, files and how it was verified. Create tasks for follow-ups and bugs you find instead of leaving them only in chat. Do not invent progress."
        if getattr(owner,"desktop_requested",False):
            self.desktop_enabled=True
            try:context=json.loads(self.desktop_context or "{}")
            except ValueError:context={}
            local=bool(context.get("local",True))
            tests=context.get("tests","off") if context.get("tests") in ("auto","ask") else "off"
            tools=(("download_file","run_local_command") if local else ())+(("minecraft_test",) if tests!="off" else ())
            servers.update(json.loads(owner.desktop.config(sid,tools))["mcpServers"])
            if local:
                system_prompt += " Relay desktop tools operate on the user's computer, not this server. Use relay_desktop.download_file for delivery to the computer, and relay_desktop.run_local_command for local commands. Relay applies the local permission mode: Auto runs commands and downloads without per-action confirmation; other modes ask the user. This is full access with the local user permissions; the working directory is not a sandbox. Invoke the tools directly rather than asking for approval in chat. Never claim local execution or delivery before the tool succeeds."
            if tests!="off":
                system_prompt += " In-game tests: relay_desktop.minecraft_test runs a hidden Minecraft with given mod jars on the user's computer and returns screenshots and logs saved on this server. Call it when a change can only be confirmed by playing (GUI, rendering, input, client behaviour). The user's Relay setting is '" + ("run automatically" if tests=="auto" else "ask before each run") + "'; Relay enforces it, so call the tool directly rather than asking in chat. Look at the returned screenshots yourself before reporting. If the test was skipped or failed, say what remains unverified."
            system_prompt += " Local computer context: " + self.desktop_context
            env["MCP_TOOL_TIMEOUT"]="2000000"
        if servers:
            args += ["--mcp-config", json.dumps({"mcpServers": servers})]
        if self.custom_prompt:
            system_prompt += "\n\nUser's custom instructions for this cluster/session:\n" + self.custom_prompt
        if len(system_prompt.encode("utf-8"))>120000:raise ValueError("Промпт сессии слишком длинный. Сократи общие инструкции или вставки.")
        args += ["--append-system-prompt",system_prompt]
        self.process = subprocess.Popen(args, cwd=cwd, env=env,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            encoding="utf-8", errors="replace", bufsize=1, start_new_session=True)
        threading.Thread(target=self.read, daemon=True).start()
        threading.Thread(target=self.read_errors, daemon=True).start()
        self.write({"type": "control_request", "request_id": self.init_id, "request": {"subtype": "initialize", "hooks": None}})

    def write(self, obj):
        with self.lock:
            if self.closed or self.process.poll() is not None:
                raise ValueError("Claude завершил работу. Отправь сообщение ещё раз, чтобы продолжить сессию.")
            self.process.stdin.write(json.dumps(obj, ensure_ascii=False) + "\n")
            self.process.stdin.flush()

    def read_errors(self):
        for line in self.process.stderr:
            self.error.append(line.rstrip()[:1000])

    def read(self):
        try:
            for line in self.process.stdout:
                try:
                    data = json.loads(line)
                except ValueError:
                    continue
                kind = data.get("type")
                if kind == "rate_limit_event":
                    if self.owner.usage.observe(data):
                        self.owner.send(self.owner.usage.snapshot())
                    continue
                if kind == "control_response":
                    response = data.get("response", {})
                    if response.get("request_id") == self.init_id:
                        if response.get("subtype") == "error":
                            self.error.append(response.get("error", "Не удалось запустить Claude."))
                        else:
                            self.owner.learn_catalog(response.get("response") or {})
                        self.ready.set()
                    continue
                if kind == "control_request":
                    req = data.get("request", {})
                    request_id = data.get("request_id", "")
                    if req.get("subtype") == "can_use_tool":
                        if self.cluster_context.get("role")=="worker" and req.get("tool_name")=="AskUserQuestion":
                            self.write({"type":"control_response","response":{"subtype":"success","request_id":request_id,"response":{"behavior":"deny","message":"Report the blocker and your question to the lead Claude session in your final handoff. The lead will provide the next assignment or ask the user if needed."}}})
                            continue
                        if (self.desktop_enabled and req.get("tool_name") in ("mcp__relay_desktop__download_file","mcp__relay_desktop__run_local_command","mcp__relay_desktop__minecraft_test")) or str(req.get("tool_name","")).startswith("mcp__relay_board__") or (self.mode=="auto" and self.cluster_context.get("role")=="leader" and req.get("tool_name") in tuple("mcp__relay_cluster__"+t for t in ("list_workers","dispatch_task","task_status","wait_for_task","cancel_task","cluster_status"))):
                            # Actual authorization is enforced by the desktop UI
                            # or its locally enabled automatic policy, not here.
                            self.write({"type":"control_response","response":{"subtype":"success","request_id":request_id,"response":{"behavior":"allow","updatedInput":req.get("input",{})}}})
                            continue
                        self.pending[request_id] = req
                        self.owner.send({"type": "permission", "sid": self.sid, "id": request_id,
                                         "tool": req.get("tool_name", "Действие"), "input": req.get("input", {})})
                    else:
                        self.write({"type": "control_response", "response": {"subtype": "error", "request_id": request_id,
                            "error": "This host does not support this control request"}})
                    continue
                if kind == "control_cancel_request":
                    request_id = data.get("request_id", "")
                    self.pending.pop(request_id, None)
                    self.owner.send({"type": "permission_cancelled", "sid": self.sid, "id": request_id})
                    continue
                if kind == "system" and data.get("subtype") == "task_notification" and self.turns == 0 and not self.closed:
                    # A background command finished: Claude Code is about to answer it.
                    self.turns = 1
                if kind == "system" and data.get("subtype") == "background_tasks_changed":
                    self.background = [str(t.get("description") or t.get("task_type") or "задача")[:200]
                                       for t in data.get("tasks") or [] if isinstance(t, dict)]
                    self.busy = self.turns > 0 or bool(self.background)
                    self.owner.run_state(self)
                if kind in ("assistant", "stream_event") and self.turns == 0 and not self.closed:
                    # Claude carries on by itself, e.g. when a background command finishes.
                    self.turns = 1
                    self.busy = True
                    self.owner.run_state(self)
                self.owner.send({"type": "claude", "sid": self.sid, "data": data})
                if kind == "result":
                    self.turns = max(0, self.turns - 1)
                    self.busy = self.turns > 0 or bool(self.background)
                    if not self.busy:
                        self.pending.clear()
                    self.owner.run_state(self)
        finally:
            self.ready.set()
            was_busy = self.busy
            self.busy = False
            self.turns = 0
            self.background = []
            self.owner.run_state(self)
            if not self.closed:
                self.owner.send({"type": "run_closed", "sid": self.sid, "interrupted": was_busy,
                                 "detail": "\n".join(self.error)[-1800:]})

    def prompt(self, text, images=None, steer=False):
        if not self.ready.wait(75):
            self.close()
            raise ValueError("Claude не ответил при запуске. Проверь авторизацию на сервере и повтори подключение.")
        if self.process.poll() is not None:
            raise ValueError("\n".join(self.error) or "Claude завершился при запуске.")
        content = ([{"type": "text", "text": text}] if text else []) + [{"type": "image", "source": image} for image in (images or [])]
        message = {"type": "user", "session_id": self.sid, "message": {"role": "user", "content": content}, "parent_tool_use_id": None}
        if steer:
            # Mid-answer: Claude Code takes it in at the next step of the same turn, like
            # typing into the terminal while it works. It joins that turn, so no extra result.
            message["priority"] = "next"
            self.write(message)
            self.owner.run_state(self)
            return
        self.write(message)
        self.turns += 1
        self.busy = True
        self.owner.run_state(self)

    def control(self, subtype, **fields):
        """Model and permission mode change at once, even in the middle of an answer."""
        self.write({"type": "control_request", "request_id": "relay-" + subtype + "-" + uuid.uuid4().hex,
                    "request": {"subtype": subtype, **fields}})

    def permit(self, request_id, allow, answers):
        req = self.pending.pop(request_id, None)
        if req is None:
            raise ValueError("Этот запрос уже закрыт.")
        updated = dict(req.get("input", {}))
        if answers and req.get("tool_name") == "AskUserQuestion":
            updated["answers"] = answers
        response = {"behavior": "allow", "updatedInput": updated} if allow else {
            "behavior": "deny", "message": "The user declined this action in Relay."}
        self.write({"type": "control_response", "response": {"subtype": "success", "request_id": request_id, "response": response}})

    def stop(self):
        if getattr(self.owner,"cluster",None):self.owner.cluster.cancel(self.sid)
        if getattr(self.owner,"desktop",None):self.owner.desktop.cancel(self.sid)
        if not self.busy:
            return
        self.write({"type": "control_request", "request_id": "relay-stop-" + uuid.uuid4().hex,
                    "request": {"subtype": "interrupt"}})
        def fallback():
            time.sleep(5)
            if self.busy:
                self.close()
                self.owner.send({"type": "run_closed", "sid": self.sid, "interrupted": True, "detail": ""})
        threading.Thread(target=fallback, daemon=True).start()

    def close(self):
        if getattr(self.owner,"cluster",None):self.owner.cluster.cancel(self.sid,revoke=True)
        if getattr(self.owner,"desktop",None):self.owner.desktop.cancel(self.sid)
        self.closed = True
        if self.process.poll() is None:
            try:
                os.killpg(self.process.pid, signal.SIGTERM)
                self.process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(self.process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass


class FileTransfers:
    """Downloads explicitly initiated by the desktop UI, independent of PC control."""
    def __init__(self,send):
        self.send=send;self.lock=threading.RLock();self.items={}

    def info(self,ident,path,cwd):
        source=None
        try:
            ident=valid_id(ident)
            if not isinstance(path,str) or not path.strip() or len(path)>4096 or "\x00" in path:
                raise ValueError("Некорректный путь файла.")
            requested=Path(path).expanduser()
            if not requested.is_absolute():
                base=directory(cwd);requested=base/requested
                if not requested.exists() and len(Path(path).parts)==1:
                    matches={p.resolve() for parent in ("build/libs","target","dist","download") if (p:=base/parent/path).is_file()}
                    if len(matches)>1:raise ValueError("Найдено несколько файлов с таким именем. Нужен полный путь к готовому файлу.")
                    if matches:requested=matches.pop()
            requested=requested.resolve()
            descriptor=os.open(requested,os.O_RDONLY | os.O_NONBLOCK | getattr(os,"O_NOFOLLOW",0))
            source=os.fdopen(descriptor,"rb");metadata=os.fstat(source.fileno())
            if not stat.S_ISREG(metadata.st_mode):raise ValueError("Скачивать можно только обычные файлы.")
            if metadata.st_size>512*1024*1024:raise ValueError("Размер файла — до 512 МБ.")
            with self.lock:
                if len(self.items)>=8 or ident in self.items:raise ValueError("Уже скачивается несколько файлов. Дождись завершения.")
                self.items[ident]={"file":source,"cancel":threading.Event(),"active":False}
                source=None
            self.send({"type":"artifact_info","id":ident,"path":str(requested),"name":requested.name,"size":metadata.st_size})
        except FileNotFoundError:
            self.send({"type":"artifact_info","id":ident,"error":"Файл на сервере не найден. Claude должен указать путь к реально собранному файлу."})
        except (ValueError,OSError,TypeError) as error:
            self.send({"type":"artifact_info","id":ident,"error":str(error)[:500]})
        finally:
            if source is not None:source.close()

    def stream(self,ident):
        with self.lock:
            item=self.items.get(ident)
            if not item or item["active"]:return
            item["active"]=True
        try:
            digest=hashlib.sha256();size=0;sequence=0
            while not item["cancel"].is_set():
                body=item["file"].read(128*1024)
                if not body:
                    self.send({"type":"desktop_end","id":ident,"size":size,"sha256":digest.hexdigest()});return
                size+=len(body)
                if size>512*1024*1024:raise ValueError("Размер файла превысил 512 МБ.")
                digest.update(body)
                self.send({"type":"desktop_chunk","id":ident,"sequence":sequence,"data":base64.b64encode(body).decode()});sequence+=1
        except (OSError,ValueError) as error:
            try:self.send({"type":"desktop_end","id":ident,"error":str(error)[:500]})
            except OSError:pass
        finally:
            with self.lock:self.items.pop(ident,None)
            item["file"].close()

    def cancel(self,ident):
        with self.lock:
            item=self.items.get(ident)
            if item:
                item["cancel"].set()
                if not item["active"]:self.items.pop(ident,None);item["file"].close()

    def close(self):
        with self.lock:
            for ident in list(self.items):self.cancel(ident)


class Bridge:
    def __init__(self, send=emit, transcripts=None, binary=None):
        self.send = send
        self.transcripts = transcripts or Transcripts()
        self.binary = binary or claude_binary()
        self.runs = {}
        self.run_lock = threading.RLock()
        self.usage = UsageState()
        self.files = FileTransfers(self.send)
        source = globals().get('ATTACHMENTS_SOURCE')
        if not source:
            source = Path(__file__).with_name('attachments.py').read_text(encoding='utf-8')
        namespace = {'__name__': 'relay_attachments'}
        exec(source, namespace)
        self.uploads = namespace['UploadStore']()
        self.desktop = None
        self.cluster = None
        self.cluster_instructions = ""
        self.cluster_context = {}
        self.custom_prompt = ""
        self.desktop_requested = False
        self.desktop_context = ""
        self.actor = ""
        self.board, self.board_file = load_board()
        self.meta = SessionMeta()
        self.catalog = None
        self.catalog_probe = False

    def learn_catalog(self, info):
        """Models and slash commands exactly as this server's Claude Code reports them."""
        models = [{k: m.get(k) for k in ("value", "displayName", "description", "resolvedModel", "supportedEffortLevels") if k in m}
                  for m in info.get("models") or [] if isinstance(m, dict) and isinstance(m.get("value"), str)]
        commands = [{"name": c.get("name"), "description": str(c.get("description") or "")[:300], "hint": str(c.get("argumentHint") or "")[:120]}
                    for c in info.get("commands") or [] if isinstance(c, dict) and isinstance(c.get("name"), str) and not c["name"].startswith("__")]
        if not models and not commands:
            return
        self.catalog = {"type": "catalog", "models": models, "commands": commands,
                        "version": str(info.get("claude_code_version") or "")}
        self.send(self.catalog)

    def probe_catalog(self):
        """Start Claude Code once without a prompt just to read its models and commands."""
        if self.catalog or self.catalog_probe:
            return
        self.catalog_probe = True
        def run():
            try:
                env = dict(os.environ); env.pop("CLAUDECODE", None)
                process = subprocess.Popen([self.binary, "-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose"],
                    cwd=str(Path.home()), env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                    text=True, encoding="utf-8", errors="replace", start_new_session=True)
                timer = threading.Timer(40, process.kill); timer.start()
                try:
                    process.stdin.write(json.dumps({"type": "control_request", "request_id": "relay-catalog", "request": {"subtype": "initialize", "hooks": None}}) + "\n")
                    process.stdin.flush()
                    for line in process.stdout:
                        try: data = json.loads(line)
                        except ValueError: continue
                        if data.get("type") == "control_response":
                            self.learn_catalog((data.get("response") or {}).get("response") or {})
                            break
                finally:
                    timer.cancel()
                    process.kill()
                    process.wait()
            except (OSError, ValueError):
                pass
            finally:
                self.catalog_probe = False
        threading.Thread(target=run, daemon=True).start()

    def claude_version(self):
        try:
            return subprocess.check_output([self.binary, "--version"], text=True, timeout=15, stdin=subprocess.DEVNULL).strip()
        except (OSError, subprocess.SubprocessError):
            return "Claude Code"

    def update_claude(self, manual=False):
        """`claude update` on this server; the next started session uses the new version."""
        if getattr(self, "updating", False):
            if manual:
                self.send({"type": "claude_update", "ok": False, "running": True, "output": "Обновление уже идёт."})
            return
        self.updating = True
        try:
            before = self.claude_version()
            if manual:
                self.send({"type": "claude_update", "running": True, "version": before})
            env = dict(os.environ); env.pop("CLAUDECODE", None)
            try:
                result = subprocess.run([self.binary, "update"], cwd=str(Path.home()), env=env, stdin=subprocess.DEVNULL,
                                        capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=600)
                ok, output = result.returncode == 0, (result.stdout + result.stderr).strip()
            except (OSError, subprocess.SubprocessError) as error:
                ok, output = False, str(error)
            try:
                self.binary = claude_binary()  # An update can move the binary.
            except ValueError:
                pass
            after = self.claude_version()
            self.catalog = None  # New models and commands come with a new version.
            event = {"type": "claude_update", "ok": ok, "running": False, "before": before, "version": after,
                     "changed": before != after, "output": output[-3000:]}
            if manual or before != after:
                self.send(event)
            if before != after:
                self.probe_catalog()
        finally:
            self.updating = False

    def quick_command(self, sid, text):
        """`/model x` and `/mode x` switch at once, whichever app sent them, and answer right away."""
        match = re.fullmatch(r"\s*/(model|mode)(?:\s+(\S+))?\s*", text)
        if not match:
            return False
        kind, value = match.groups()
        run = self.runs.get(sid)
        models = (self.catalog or {}).get("models") or []
        if kind == "model":
            if not value:
                reply = "Модели на этом сервере: " + ", ".join(f"`{m['value']}` ({m.get('displayName') or m['value']})" for m in models) if models else "Напиши `/model <название>`, например `/model opus`."
            else:
                wanted = value.lower()
                found = next((m for m in models if wanted in (m["value"].lower(), str(m.get("displayName", "")).lower())), None) or \
                    next((m for m in models if m["value"].lower().startswith(wanted) or str(m.get("displayName", "")).lower().startswith(wanted)), None)
                if models and not found:
                    reply = f"Нет модели «{value}». Есть: " + ", ".join(m["value"] for m in models)
                else:
                    chosen = found["value"] if found else value
                    if run and not run.closed:
                        run.control("set_model", model=None if chosen == "default" else chosen); run.model = chosen
                    self.send({"type": "model_changed", "sid": sid, "model": chosen})
                    reply = "Модель: " + ((found or {}).get("displayName") or chosen) + (" — со следующего ответа." if run and run.busy else ".")
        else:
            modes = {"auto": "auto", "авто": "auto", "plan": "plan", "план": "plan", "edits": "acceptEdits", "acceptedits": "acceptEdits", "manual": "manual", "ручной": "manual"}
            mode = modes.get((value or "").lower())
            if not mode:
                reply = "Режимы: `/mode auto`, `/mode manual`, `/mode edits`, `/mode plan`."
            else:
                if run and not run.closed:
                    run.control("set_permission_mode", mode=mode); run.mode = mode
                self.send({"type": "mode_changed", "sid": sid, "mode": mode})
                reply = "Режим: " + {"auto": "авто", "plan": "план", "acceptEdits": "правки файлов", "manual": "с подтверждением"}[mode] + "."
        # The app has marked the chat busy for this message: answer it like Claude would.
        if not (run and run.busy):
            self.send({"type": "claude", "sid": sid, "data": {"type": "assistant", "message": {"content": [{"type": "text", "text": reply}]}}})
            self.send({"type": "claude", "sid": sid, "data": {"type": "result", "is_error": False, "result": reply}})
        else:
            self.send({"type": "notice", "sid": sid, "message": reply})
        return True

    def run_state(self, run):
        self.send({"type": "run_state", "sid": run.sid, "busy": bool(run.busy), "queued": max(0, run.turns - 1),
                   "background": list(getattr(run, "background", []))})

    def running(self):
        own = {sid for sid, run in list(self.runs.items()) if run.busy and not run.closed}
        return own | set(getattr(self, "peer_running", {}))

    def send_sessions(self):
        sessions = self.transcripts.sessions()
        try:
            sessions, folders = self.meta.apply(sessions)
        except OSError:
            folders = []
        busy = self.running()
        for s in sessions:
            s["running"] = s.get("id") in busy
        self.send({"type": "sessions", "sessions": sessions, "folders": folders})

    def hello(self):
        version = self.claude_version()
        self.send({"type": "hello", "home": str(Path.home()), "hostname": os.uname().nodename,
                   "version": version, "protocol": 4, "host": bool(globals().get("RELAY_HOSTED"))})
        if self.catalog:
            self.send(self.catalog)
        else:
            self.probe_catalog()

    def dispatch(self, req):
        op = req.get("op")
        claimed = False
        try:
            if op == "sessions":
                self.send_sessions()
            elif op == "session_meta":
                sid = valid_id(req.get("sid"))
                self.meta.session(sid, title=req.get("title"), pinned=req.get("pinned"),
                                  folder=req.get("folder"), actor=req.get("actor") or "")
                self.send_sessions()
            elif op == "session_folder":
                created = self.meta.folder(req.get("id"), name=req.get("name"), delete=bool(req.get("delete")),
                                           order=req.get("order"))
                if req.get("sid") and created:
                    self.meta.session(valid_id(req.get("sid")), folder=created, actor=req.get("actor") or "")
                self.send_sessions()
            elif op == "delete_session":
                sid = valid_id(req.get('sid'))
                with self.run_lock:
                    run = self.runs.get(sid)
                    if run and run.busy:
                        raise ValueError('Сначала останови ответ в этой сессии.')
                    if run:
                        run.close()
                        self.runs.pop(sid, None)
                    self.transcripts.delete(sid)
                try:
                    self.meta.forget(sid)
                except OSError:
                    pass
                self.send({'type': 'session_deleted', 'sid': sid})
            elif op == "file_info":
                self.files.info(req.get("id"),req.get("path"),req.get("cwd"))
            elif op == "file_stream":
                self.files.stream(req.get("id"))
            elif op == "file_cancel":
                self.files.cancel(req.get("id"))
            elif op == "desktop_result":
                if self.desktop:self.desktop.result(req.get("id"),req.get("result",{}))
            elif op == "desktop_download":
                if self.desktop:self.desktop.download(req.get("id"),req.get("index"))
            elif op == "desktop_upload":
                if self.desktop:self.desktop.upload(req.get("id"),req.get("name"),req.get("sequence"),req.get("data"))
            elif op == "desktop_upload_end":
                if self.desktop:self.desktop.upload_end(req.get("id"),req.get("name"))
            elif op == "usage":
                self.send(self.usage.snapshot())
                self.usage.refresh_async(self.send)
            elif op == "history":
                sid = valid_id(req.get("sid"))
                self.send({"type": "history", "sid": sid, "messages": self.transcripts.history(sid)})
            elif op == "folders":
                path = directory(req.get("path"))
                children = []
                for p in path.iterdir():
                    try:
                        if p.is_dir() and (req.get("hidden") or not p.name.startswith(".")):
                            children.append({"name": p.name, "path": str(p)})
                    except OSError:
                        pass
                children.sort(key=lambda p: p["name"].casefold())
                self.send({"type": "folders", "path": str(path), "parent": str(path.parent), "folders": children[:1000], "request": req.get("request", 0)})
            elif op == "prompt":
                sid = valid_id(req.get("sid"))
                text = req.get("text")
                images = req.get("images", [])
                files = req.get("files", [])
                if not isinstance(text, str) or (not text.strip() and not images and not files) or len(text.encode("utf-8")) > 1024 * 1024:
                    raise ValueError("Сообщение пустое или слишком длинное.")
                if not images and not files and self.quick_command(sid, text):
                    return
                validate_images(images)
                if not isinstance(files, list) or any(not isinstance(f, dict) or f.get('type') not in ('file', 'uploaded_file') for f in files):
                    raise ValueError('Некорректное вложение.')
                decoded = decode_files([f for f in files if f['type'] == 'file'])
                uploaded = [self.uploads.resolve(sid, f) for f in files if f['type'] == 'uploaded_file']
                inline = sum(len(body) for _, body in decoded) + sum(len(base64.b64decode(i['data'])) for i in images)
                if len(images) + len(files) > 8 or inline + sum(f['bytes'] for f in uploaded) > 512*1024*1024:
                    raise ValueError('До восьми вложений общим размером 512 МБ.')
                if inline > 16*1024*1024:
                    raise ValueError('Изображения в одном сообщении — до 16 МБ. Остальные файлы — до 512 МБ.')
                cwd = str(directory(req.get("cwd")))
                mode, model = req.get("mode", "manual"), req.get("model", "default")
                effort = validate_effort(req.get("effort", "default"))
                agent_role = req.get("agent_role", "")
                if agent_role not in ("", *AGENT_ROLES):
                    raise ValueError("Неизвестная роль агента.")
                custom_prompt=req.get("custom_prompt","")
                if not isinstance(custom_prompt,str) or len(custom_prompt.encode("utf-8"))>96000 or "\x00" in custom_prompt:
                    raise ValueError("Некорректный промпт сессии.")
                cluster_context=req.get("cluster_context") or {}
                if not isinstance(cluster_context,dict) or cluster_context.get("role","") not in ("","leader","worker"):
                    raise ValueError("Некорректный контекст кластера.")
                if cluster_context.get("role"):
                    cluster_context={"run":valid_id(cluster_context.get("run")),"role":cluster_context["role"],"name":str(cluster_context.get("name", ""))[:160]}
                with self.run_lock:
                    self.custom_prompt=custom_prompt
                    self.cluster_context=cluster_context
                    if cluster_context.get("role")=="leader" and self.cluster is None:
                        source=globals().get("CLUSTER_SOURCE")
                        if not source:raise ValueError("Обнови Relay для управления исполнителями.")
                        namespace={"__name__":"relay_cluster_adapter","SOURCE":source}
                        exec(source,namespace)
                        self.cluster=namespace["RelayCluster"](self.send)
                        self.cluster_instructions=namespace["LEADER_INSTRUCTIONS"]
                    self.agent_role = agent_role
                    desktop_enabled=bool(req.get("desktop_enabled",False))
                    if desktop_enabled and self.desktop is None:
                        namespace={"__name__":"relay_desktop_adapter"}
                        source=globals().get("DESKTOP_SOURCE")
                        if not source:raise ValueError("Обнови Relay для работы с этим ПК.")
                        namespace["SOURCE"]=source
                        exec(source,namespace)
                        self.desktop=namespace["RelayDesktop"](self.send)
                    self.desktop_requested=desktop_enabled
                    self.actor=str(req.get("actor") or "")[:80]
                    self.desktop_context=json.dumps(req.get("desktop_context",{}),ensure_ascii=False)[:5000] if desktop_enabled else ""
                    run = self.runs.get(sid)
                    steer = bool(run and run.busy and run.turns > 0 and not run.closed and run.process.poll() is None)
                    if run and run.busy and not run.closed and run.process.poll() is None:
                        # Mid-answer: the message joins Claude's queue; model and mode switch live.
                        if model != run.model:
                            run.control("set_model", model=None if model == "default" else model); run.model = model
                        if mode != run.mode:
                            run.control("set_permission_mode", mode=mode); run.mode = mode
                    elif run and (run.closed or run.process.poll() is not None or (run.cwd, run.model, run.mode, run.effort, getattr(run,"desktop_enabled",False), getattr(run,"desktop_context",""), getattr(run,"agent_role",""), getattr(run,"custom_prompt",""), getattr(run,"cluster_context",{})) != (cwd, model, mode, effort, desktop_enabled, self.desktop_context, agent_role, custom_prompt, cluster_context)):
                        run.close()
                        self.runs.pop(sid, None)
                        run = None
                    if run is None:
                        if len(self.runs) >= 24:
                            idle = next((key for key, value in self.runs.items() if not value.busy), None)
                            if idle:
                                self.runs.pop(idle).close()
                            else:
                                raise ValueError("Уже работают 24 разговора. Дождись завершения одного из них.")
                        existing = self.transcripts.find(sid)
                        if existing and self.transcripts.metadata(existing)["cwd"] != cwd:
                            raise ValueError("Открой исходную рабочую папку этой сессии.")
                        run = Run(self, sid, cwd, model, mode, bool(existing), effort)
                        self.runs[sid] = run
                    claimed = not steer and not run.busy
                if decoded or uploaded:
                    saved = store_files(sid, decoded) if decoded else []
                    saved.extend(uploaded)
                    text += "\n\n<relay-files>\n" + json.dumps(saved, ensure_ascii=False) + "\n</relay-files>"
                run.prompt(text, images, steer=steer)
            elif op == "cluster_reply":
                if self.cluster:self.cluster.result(valid_id(req.get("id")),valid_id(req.get("sid")),valid_id(req.get("run")),req.get("result"))
            elif op == "permission":
                run = self.runs.get(valid_id(req.get("sid")))
                if run:
                    run.permit(str(req.get("id", "")), bool(req.get("allow")), req.get("answers"))
            elif op == "control":
                run = self.runs.get(valid_id(req.get("sid")))
                kind, value = req.get("action"), req.get("value")
                if kind == "model" and isinstance(value, str) and len(value) <= 160:
                    if run and not run.closed:
                        run.control("set_model", model=None if value == "default" else value); run.model = value
                elif kind == "mode" and value in ("manual", "auto", "acceptEdits", "plan"):
                    if run and not run.closed:
                        run.control("set_permission_mode", mode=value); run.mode = value
                else:
                    raise ValueError("Неизвестная команда.")
            elif op == "claude_update":
                self.update_claude(manual=True)
            elif op == "catalog":
                if self.catalog:
                    self.send(self.catalog)
                else:
                    self.probe_catalog()
            elif op == "stop":
                run = self.runs.get(valid_id(req.get("sid")))
                if run:
                    run.stop()
            elif op == "board":
                if not self.board:
                    raise ValueError("Обнови Relay: доска приходит вместе с ним.")
                board = self.board["Board"](str(req.get("board") or "sbrp"))
                self.send({"type": "board", "state": board.state(since=req.get("since"))})
            elif op == "board_apply":
                if not self.board:
                    raise ValueError("Обнови Relay: доска приходит вместе с ним.")
                board = self.board["Board"](str(req.get("board") or "sbrp"))
                actor = str(req.get("actor") or "Relay")[:80]
                host = str(req.get("host") or "relay")[:80]
                try:
                    result = board.apply(req.get("ops") or [], actor, host, "relay")
                except (ValueError, KeyError, TypeError) as error:
                    self.send({"type": "board_error", "message": str(error)[:500]})
                    result = None
                self.send({"type": "board", "state": board.state(), "applied": result})
            elif op == "ping":
                self.send({"type": "pong"})
            else:
                raise ValueError("Обнови приложение: сервер не знает это действие.")
        except (ValueError, OSError, subprocess.SubprocessError, TypeError, AttributeError, UnicodeError) as error:
            sid = req.get("sid", "")
            run = self.runs.get(sid)
            if op == "prompt" and run and claimed and run.turns == 0:
                run.busy = False
                self.run_state(run)
            self.send({"type": "error", "sid": sid, "op": op, "message": str(error)[:1800]})

    def close(self):
        for run in list(self.runs.values()):
            run.close()
        if self.cluster:self.cluster.close()
        if self.desktop:self.desktop.close()
        self.files.close()


def validate_images(images):
    if not isinstance(images, list) or len(images) > 8:
        raise ValueError("До восьми вложений в одном сообщении.")
    for source in images:
        if not isinstance(source, dict) or source.get("type") != "base64" or source.get("media_type") not in ("image/png", "image/jpeg", "image/webp", "image/gif"):
            raise ValueError("Неподдерживаемый формат изображения.")
        data = source.get("data", "")
        if not isinstance(data, str) or len(data) > 6 * 1024 * 1024:
            raise ValueError("Изображение слишком большое.")
        try:
            body = base64.b64decode(data, validate=True)
        except (ValueError, TypeError):
            raise ValueError("Изображение повреждено.")
        if not body or len(body) > 4 * 1024 * 1024:
            raise ValueError("Изображение слишком большое или пустое.")


def decode_files(files):
    if not isinstance(files, list) or len(files) > 8:
        raise ValueError("До восьми файлов в одном сообщении.")
    result = []
    total = 0
    for source in files:
        if not isinstance(source, dict) or source.get("type") != "file" or 'path' in source:
            raise ValueError("Некорректное вложение.")
        name = str(source.get("name", "Файл")).replace("\\", "/").split("/")[-1]
        name = "".join(c for c in name if ord(c) >= 32 and c != "\x7f").encode("utf-8")[:180].decode("utf-8", errors="ignore")
        if not name or name in (".", ".."):
            raise ValueError("Некорректное имя файла.")
        data = source.get("data", "")
        if not isinstance(data, str) or len(data) > 17*1024*1024:
            raise ValueError("Размер вложений — до 12 МБ.")
        try:
            body = base64.b64decode(data, validate=True)
        except (ValueError, TypeError):
            raise ValueError("Файл повреждён при передаче.")
        total += len(body)
        if total > 12*1024*1024:
            raise ValueError("Размер вложений — до 12 МБ.")
        result.append((name, body))
    return result


def store_files(sid, files, root=None):
    root = Path(root) if root else Path.home() / ".cache/relay-desktop/attachments"
    folder = root / valid_id(sid) / uuid.uuid4().hex
    folder.mkdir(parents=True, mode=0o700)
    result = []
    for name, body in files:
        target = folder / name
        if target.exists():
            target = folder / (uuid.uuid4().hex[:6] + "-" + name)
        with target.open("xb") as output:
            os.chmod(target, 0o600)
            output.write(body)
        result.append({"name": name, "path": str(target)})
    return result


CLIENT = threading.local()
# Replies go to the app that asked; what happens in a session goes to every app.
BROADCAST = {"claude", "permission", "permission_cancelled", "run_closed", "run_state", "sessions", "session_deleted", "catalog", "claude_update"}
UPDATE_EVERY = 6 * 3600
OWNED = {"desktop_request", "desktop_cancel", "cluster_request", "cluster_cancelled"}
HOST_IDLE = 30 * 60


class HostClient:
    """One connected app. A writer thread keeps a slow phone from holding up Claude's output."""
    def __init__(self, conn):
        self.conn = conn
        self.queue = collections.deque()
        self.ready = threading.Condition()
        self.alive = True
        threading.Thread(target=self.writer, daemon=True).start()

    def send(self, event):
        line = (json.dumps(event, ensure_ascii=False) + "\n").encode("utf-8")
        with self.ready:
            if not self.alive:
                return
            self.queue.append(line)
            if len(self.queue) > 20000:
                self.alive = False  # Hopelessly behind: it reconnects and reloads.
            self.ready.notify()

    def writer(self):
        try:
            while True:
                with self.ready:
                    while self.alive and not self.queue:
                        self.ready.wait()
                    if not self.alive:
                        break
                    line = self.queue.popleft()
                self.conn.sendall(line)
        except OSError:
            pass
        finally:
            self.close()

    def close(self):
        with self.ready:
            self.alive = False
            self.ready.notify()
        try:
            self.conn.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass


class Peer:
    """An older host (previous Relay version) that still has answers running. The new host
    shows them as running, passes their events on and sends their permissions and stops back;
    once they are done it lets go, and the old host exits."""
    def __init__(self, host, conn):
        self.host, self.conn = host, conn
        self.running = set()
        self.pending = {}  # sid -> {permission id: event}
        self.lock = threading.Lock()

    def send(self, req):
        with self.lock:
            self.conn.sendall((json.dumps(req, ensure_ascii=False) + "\n").encode("utf-8"))

    def watch(self):
        settle = time.time() + 5
        self.conn.settimeout(5)
        try:
            self.send({"op": "_peer"})
            stream = self.conn.makefile("rb")
            while True:
                try:
                    line = stream.readline()
                except socket.timeout:
                    if not self.running and time.time() > settle:
                        break
                    continue
                if not line:
                    break
                try:
                    event = json.loads(line)
                except ValueError:
                    continue
                kind, sid = event.get("type"), event.get("sid")
                if kind == "run_state":
                    (self.running.add if event.get("busy") else self.running.discard)(sid)
                    if not event.get("busy"):
                        self.pending.pop(sid, None)
                elif kind == "permission":
                    self.pending.setdefault(sid, {})[event.get("id")] = event
                elif kind == "permission_cancelled":
                    self.pending.get(sid, {}).pop(event.get("id"), None)
                elif kind == "run_closed":
                    self.running.discard(sid); self.pending.pop(sid, None)
                if kind in ("claude", "permission", "permission_cancelled", "run_closed", "run_state", "notice") or (kind == "error" and sid):
                    self.host.route(event)
                self.host.peers_changed()
                if not self.running and time.time() > settle:
                    break
        except OSError:
            pass
        finally:
            self.running.clear(); self.pending.clear()
            self.host.drop_peer(self)
            try:
                self.conn.close()
            except OSError:
                pass


class Host:
    """The long-lived process: one Bridge whose Claude runs outlive any app connection."""
    def __init__(self, bridge_factory=None):
        self.clients = []
        self.lock = threading.Lock()
        self.owners = {}  # sid -> the app that sent its last prompt (it serves the PC tools)
        self.quiet_since = time.time()
        self.retiring = False
        self.peers = []
        self.bridge = (bridge_factory or Bridge)(send=self.route)
        self.bridge.peer_running = {}

    def peers_changed(self):
        self.bridge.peer_running = {sid: peer for peer in list(self.peers) for sid in peer.running}

    def drop_peer(self, peer):
        self.peers = [p for p in self.peers if p is not peer]
        self.peers_changed()

    def adopt(self, own_path):
        """Join older hosts so the answers they are still giving stay visible."""
        for path in Path(own_path).parent.glob("*.sock"):
            if str(path) == str(own_path):
                continue
            conn = connect(path)
            if conn is None:
                try:
                    path.unlink()  # Stale socket of a host that is gone.
                except OSError:
                    pass
                continue
            peer = Peer(self, conn)
            self.peers.append(peer)
            threading.Thread(target=peer.watch, daemon=True).start()

    def peer_for(self, sid):
        return self.bridge.peer_running.get(sid)

    def route(self, event):
        kind = event.get("type")
        with self.lock:
            clients = [c for c in self.clients if c.alive]
        if kind in OWNED:
            owner = self.owners.get(event.get("sid"))
            if owner in clients:
                owner.send(event)
            elif kind == "desktop_request" and self.bridge.desktop:
                self.bridge.desktop.result(event.get("id"), {"ok": False, "message": "Relay на компьютере сейчас не подключён к серверу, поэтому действие на ПК не выполнено."})
            elif kind == "cluster_request" and clients:
                clients[0].send(event)
            return
        asking = getattr(CLIENT, "value", None)
        if asking is not None and kind not in BROADCAST:
            asking.send(event)
            return
        for client in clients:
            client.send(event)

    def replay(self, client):
        """A returning app learns what is still running and what is waiting for an answer."""
        for peer in list(self.peers):
            for sid in list(peer.running):
                client.send({"type": "run_state", "sid": sid, "busy": True, "queued": 0})
                for event in list(peer.pending.get(sid, {}).values()):
                    client.send(event)
        for run in list(self.bridge.runs.values()):
            if run.busy and not run.closed:
                client.send({"type": "run_state", "sid": run.sid, "busy": True, "queued": max(0, run.turns - 1),
                             "background": list(run.background)})
                for ident, req in list(run.pending.items()):
                    client.send({"type": "permission", "sid": run.sid, "id": ident,
                                 "tool": req.get("tool_name", "Действие"), "input": req.get("input", {})})

    def serve_client(self, conn):
        client = HostClient(conn)
        with self.lock:
            self.clients.append(client)
        CLIENT.value = client
        try:
            self.bridge.hello()
            self.replay(client)
            for line in conn.makefile("rb"):
                if len(line) > MAX_REQUEST:
                    continue
                try:
                    req = json.loads(line)
                except ValueError:
                    continue
                if not isinstance(req, dict):
                    continue
                if req.get("op") == "_peer":
                    # A newer host took over: finish what runs here, then exit. It still
                    # gets every event, but does not keep this host alive.
                    client.peer = True
                    self.retiring = True
                    self.quiet_since = 0
                    continue
                peer = self.peer_for(req.get("sid")) if req.get("op") in ("permission", "stop", "control", "prompt") else None
                if peer is not None:
                    # The answer is running in the older host: it gets the reply, the stop or the follow-up.
                    try:
                        peer.send(req)
                    except OSError:
                        pass
                    continue
                if req.get("op") == "prompt" and isinstance(req.get("sid"), str):
                    self.owners[req["sid"]] = client
                if req.get("op") in ("desktop_upload", "desktop_upload_end", "desktop_result"):
                    self.bridge.dispatch(req)
                else:
                    threading.Thread(target=self.dispatch, args=(client, req), daemon=True).start()
        except OSError:
            pass
        finally:
            client.close()
            with self.lock:
                self.clients = [c for c in self.clients if c is not client]
                if not self.clients:
                    self.quiet_since = time.time()

    def dispatch(self, client, req):
        CLIENT.value = client
        self.bridge.dispatch(req)

    def idle(self):
        with self.lock:
            if any(not getattr(c, "peer", False) for c in self.clients) and not self.retiring:
                return False
        if self.retiring:
            return not self.bridge.running()
        return not self.bridge.running() and time.time() - self.quiet_since > HOST_IDLE

    def serve(self, path):
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        path = socket_name(path)
        try:
            os.unlink(path)
        except FileNotFoundError:
            pass
        old = os.umask(0o177)
        try:
            listener.bind(path)
        finally:
            os.umask(old)
        listener.listen(16)
        listener.settimeout(30)
        threading.Thread(target=self.adopt, args=(os.path.abspath(path),), daemon=True).start()
        next_update = time.time() + 60
        try:
            while True:
                try:
                    conn, _ = listener.accept()
                except socket.timeout:
                    if self.idle():
                        break
                    # Claude Code updates itself only between answers, never under a running session.
                    if time.time() >= next_update and not self.bridge.running():
                        next_update = time.time() + UPDATE_EVERY
                        threading.Thread(target=self.bridge.update_claude, daemon=True).start()
                    continue
                conn.settimeout(None)
                threading.Thread(target=self.serve_client, args=(conn,), daemon=True).start()
        finally:
            try:
                os.unlink(path)
            except OSError:
                pass
            self.bridge.close()


def host_dir():
    path = Path.home() / ".cache/relay-desktop/host"
    path.mkdir(parents=True, exist_ok=True)
    os.chmod(path, 0o700)
    return path


def socket_name(path):
    """Unix socket paths are limited to ~100 bytes: long ones are reached from their folder."""
    path = str(path)
    if len(path.encode()) < 100:
        return path
    os.chdir(os.path.dirname(path))
    return os.path.basename(path)


def connect(path):
    conn = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        conn.connect(socket_name(path))
        return conn
    except OSError:
        conn.close()
        return None


def ensure_host(source):
    """The host of exactly this helper version, started detached if it is not running yet."""
    digest = hashlib.sha256(source).hexdigest()[:16]
    base = host_dir()
    path = base / (digest + ".sock")
    conn = connect(path)
    if conn:
        return conn
    with open(base / (digest + ".lock"), "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        conn = connect(path)
        if conn:
            return conn
        script = base / (digest + ".py")
        if not script.exists() or script.read_bytes() != source:
            temp = script.with_suffix(".tmp")
            temp.write_bytes(source)
            os.chmod(temp, 0o600)
            os.replace(temp, script)
        log = open(base / (digest + ".log"), "ab")
        subprocess.Popen([sys.executable, "-u", str(script), "--relay-host", str(path)], cwd=str(Path.home()),
                         stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True, close_fds=True)
        log.close()
        end = time.time() + 15
        while time.time() < end:
            conn = connect(path)
            if conn:
                return conn
            time.sleep(0.1)
    return None


def attach(conn):
    """The SSH side: pass lines both ways until either end goes away."""
    def upstream():
        try:
            for line in sys.stdin.buffer:
                conn.sendall(line)
        except OSError:
            pass
        finally:
            try:
                conn.shutdown(socket.SHUT_WR)
            except OSError:
                pass
    threading.Thread(target=upstream, daemon=True).start()
    out = sys.stdout.buffer
    try:
        for line in conn.makefile("rb"):
            out.write(line)
            out.flush()
    except (OSError, BrokenPipeError):
        pass
    finally:
        conn.close()


def main():
    if len(sys.argv) >= 3 and sys.argv[1] == "--relay-host":
        globals()["RELAY_HOSTED"] = True
        Host().serve(sys.argv[2])
        return
    source = globals().get("RELAY_SOURCE")
    if isinstance(source, (bytes, bytearray)) and os.environ.get("RELAY_IN_PROCESS") != "1":
        try:
            conn = ensure_host(bytes(source))
        except OSError:
            conn = None
        if conn:
            attach(conn)
            return
    in_process()


def in_process():
    """Older apps, or no host could start: everything stops when this connection closes."""
    bridge = None
    try:
        bridge = Bridge()
        bridge.hello()
        for line in sys.stdin:
            if len(line.encode("utf-8")) > MAX_REQUEST:
                continue
            try:
                req = json.loads(line)
                if isinstance(req, dict):
                    # Upload chunks and the result that follows them must stay in order.
                    if req.get("op") in ("desktop_upload", "desktop_upload_end", "desktop_result"):
                        bridge.dispatch(req)
                    else:
                        threading.Thread(target=bridge.dispatch, args=(req,), daemon=True).start()
            except ValueError:
                continue
    except (BrokenPipeError, KeyboardInterrupt):
        pass
    except Exception as error:
        emit({"type": "fatal", "message": str(error)[:1800]})
    finally:
        if bridge:
            bridge.close()


if __name__ == "__main__":
    main()

