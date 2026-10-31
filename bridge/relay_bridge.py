"""Relay's per-connection helper. Python stdlib only; no listening socket or installation.

The desktop passes this file to python3 over an authenticated SSH channel. Only
children created by this helper are stopped when the channel closes. Existing
Claude transcripts/settings are read, never rewritten by the helper.
"""
import collections
import base64
import json
import os
from pathlib import Path
import re
import shutil
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
        for row in chain:
            message = row.get("message") or {}
            if not isinstance(message,dict):continue
            role = message.get("role", row.get("type"))
            content = message.get("content", "")
            text = visible_text(plain(content)) if role == "user" else plain(content)
            if text:
                result.append({"role": role, "text": text, "uuid": row.get("uuid", "")})
            if role == "user" and isinstance(content, list):
                count = sum(1 for b in content if isinstance(b, dict) and b.get("type") == "image")
                if count:
                    result.append({"role": "user", "text": "Прикреплены изображения: " + str(count)})
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
        self.desktop_enabled = False
        self.desktop_context = getattr(owner,"desktop_context", "")
        self.busy = False
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
        system_prompt="Relay supports downloadable server files in chat. When the user requests a built mod or other generated artifact, include a Markdown download link [filename](absolute_server_file_path) for the real existing file. Verify it exists before offering it. A bare filename or a statement that the build succeeded is not file delivery. Never claim the file was delivered to the PC unless the transfer tool actually succeeded."
        servers = {}
        if getattr(owner, "board_file", None):
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
                        self.ready.set()
                    continue
                if kind == "control_request":
                    req = data.get("request", {})
                    request_id = data.get("request_id", "")
                    if req.get("subtype") == "can_use_tool":
                        if (self.desktop_enabled and req.get("tool_name") in ("mcp__relay_desktop__download_file","mcp__relay_desktop__run_local_command","mcp__relay_desktop__minecraft_test")) or str(req.get("tool_name","")).startswith("mcp__relay_board__"):
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
                if kind == "result":
                    self.busy = False
                    self.pending.clear()
                self.owner.send({"type": "claude", "sid": self.sid, "data": data})
        finally:
            self.ready.set()
            was_busy = self.busy
            self.busy = False
            if not self.closed:
                self.owner.send({"type": "run_closed", "sid": self.sid, "interrupted": was_busy,
                                 "detail": "\n".join(self.error)[-1800:]})

    def prompt(self, text, images=None):
        if not self.ready.wait(75):
            self.close()
            raise ValueError("Claude не ответил при запуске. Проверь авторизацию на сервере и повтори подключение.")
        if self.process.poll() is not None:
            raise ValueError("\n".join(self.error) or "Claude завершился при запуске.")
        self.busy = True
        content = ([{"type": "text", "text": text}] if text else []) + [{"type": "image", "source": image} for image in (images or [])]
        self.write({"type": "user", "session_id": self.sid, "message": {"role": "user", "content": content}, "parent_tool_use_id": None})

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
        self.desktop = None
        self.desktop_requested = False
        self.desktop_context = ""
        self.actor = ""
        self.board, self.board_file = load_board()

    def hello(self):
        try:
            version = subprocess.check_output([self.binary, "--version"], text=True, timeout=15).strip()
        except (OSError, subprocess.SubprocessError):
            version = "Claude Code"
        self.send({"type": "hello", "home": str(Path.home()), "hostname": os.uname().nodename,
                   "version": version, "protocol": 2})

    def dispatch(self, req):
        op = req.get("op")
        claimed = False
        try:
            if op == "sessions":
                self.send({"type": "sessions", "sessions": self.transcripts.sessions()})
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
                validate_images(images)
                decoded = decode_files(files)
                if len(images) + len(files) > 8 or sum(len(body) for _,body in decoded) + sum(len(base64.b64decode(i["data"])) for i in images) > 12*1024*1024:
                    raise ValueError("До восьми файлов общим размером 12 МБ.")
                cwd = str(directory(req.get("cwd")))
                mode, model = req.get("mode", "manual"), req.get("model", "default")
                effort = validate_effort(req.get("effort", "default"))
                with self.run_lock:
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
                    if run and run.busy:
                        raise ValueError("Дождись ответа Claude или останови его.")
                    if run and (run.closed or run.process.poll() is not None or (run.cwd, run.model, run.mode, run.effort, getattr(run,"desktop_enabled",False), getattr(run,"desktop_context","")) != (cwd, model, mode, effort, desktop_enabled, self.desktop_context)):
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
                    run.busy = True
                    claimed = True
                if decoded:
                    saved = store_files(sid, decoded)
                    text += "\n\n<relay-files>\n" + json.dumps(saved, ensure_ascii=False) + "\n</relay-files>"
                run.prompt(text, images)
            elif op == "permission":
                run = self.runs.get(valid_id(req.get("sid")))
                if run:
                    run.permit(str(req.get("id", "")), bool(req.get("allow")), req.get("answers"))
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
            if op == "prompt" and run and claimed:
                run.busy = False
            self.send({"type": "error", "sid": sid, "op": op, "message": str(error)[:1800]})

    def close(self):
        for run in list(self.runs.values()):
            run.close()
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
        if not isinstance(source, dict) or source.get("type") != "file":
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


def main():
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

