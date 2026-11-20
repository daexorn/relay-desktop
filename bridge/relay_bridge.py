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
import tempfile
import threading
import time
import uuid
import datetime
import math
import hashlib
import hmac
import stat
import urllib.request
import urllib.error
import zlib

MAX_REQUEST = 26 * 1024 * 1024
MAX_HISTORY = 48 * 1024 * 1024
WRITE_LOCK = threading.Lock()

AGENT_ROLES = {
    "worker": "You are the development agent for this task. Work in the selected host and directory, respect project instructions and concurrent edits, implement the requested change and report files and actual verification results.",
    "planner": "You are the planning agent for this task. Inspect the project, identify dependencies and propose concrete implementation steps. Do not change files unless the user explicitly requests it. Report facts and open questions.",
    "reviewer": "You are the review agent for this task. Inspect relevant changes and run appropriate checks permitted by the user's instructions. Report findings with file references and evidence. Do not claim checks you did not run.",
}

USAGE_LABELS = {"five_hour": "5 часов", "seven_day": "Неделя", "seven_day_opus": "Неделя · Opus", "seven_day_sonnet": "Неделя · Sonnet", "overage": "Дополнительно"}


BACKEND_LABELS = {"claude": "Claude Code", "codex": "Codex", "agy": "Antigravity", "api": "Пользовательский API"}
TOKEN_FIELDS = {
    "in": "input_tokens", "out": "output_tokens",
    "cache_read": "cache_read_input_tokens", "cache_write": "cache_creation_input_tokens",
}
CODEX_TOKEN_FIELDS = {**TOKEN_FIELDS, "cache_read": "cached_input_tokens", "cache_write": "cache_write_input_tokens"}


def usage_number(value, maximum):
    return float(value) if isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value) and 0 <= value <= maximum else None


def usage_time(value):
    number = usage_number(value, 253402300799)
    if number is not None:
        return int(number)
    if isinstance(value, str):
        try:
            cleaned = re.sub(r"(\.\d{6})\d+", r"\1", value.strip().replace("Z", "+00:00"))
            date = datetime.datetime.fromisoformat(cleaned)
            if date.tzinfo is None:
                date = date.replace(tzinfo=datetime.timezone.utc)
            return int(date.timestamp())
        except (ValueError, OverflowError):
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
            return {"type": "usage", "backend": "claude", "available": bool(windows or status), "windows": windows, "status": status,
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


class AgyUsageState:
    """Whitelist-only Antigravity account usage, never credentials. No login/refresh actions."""
    QUOTA_URL = "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary"
    MODELS_URL = "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels"

    def __init__(self):
        self.lock = threading.Lock()
        self.windows = []
        self.full_windows = []
        self.source = None
        self.updated_at = 0
        self.message = "Antigravity пока не передал данные о лимитах."
        self.inflight = False
        self.last_attempt = None

    def snapshot(self, short=True):
        now = time.time()
        with self.lock:
            source_windows = self.windows if short else (self.full_windows or self.windows)
            windows = [dict(window) for window in source_windows if window["resets_at"] is None or window["resets_at"] > now]
            return {"type": "usage", "backend": "agy", "available": bool(windows), "windows": windows, "status": None,
                    "source": self.source, "message": self.message, "updated_at": self.updated_at}

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

    @staticmethod
    def _post_json(opener, url, token):
        request = urllib.request.Request(
            url,
            data=b"{}",
            headers={
                "Authorization": "Bearer " + token,
                "Content-Type": "application/json",
                "User-Agent": "antigravity/1.5.11 linux/amd64",
                "Accept": "application/json",
            },
            method="POST",
        )
        with opener.open(request, timeout=6) as response:
            body = response.read(262145)
        if len(body) > 262144:
            return None
        parsed = json.loads(body)
        return parsed if isinstance(parsed, dict) else None

    @staticmethod
    def _parse_summary(data, now):
        groups = data.get("groups")
        if not isinstance(groups, list):
            return [], []
        valid_groups = [g for g in groups if isinstance(g, dict) and isinstance(g.get("buckets"), list) and g.get("buckets")]
        single = len(valid_groups) == 1
        short_windows, full_windows = [], []
        for group in valid_groups:
            group_name = " ".join(str(group.get("displayName") or "").split())[:40].replace("|", "/")
            short_group = group_name.replace(" models", "").replace("Claude and GPT", "Claude/GPT")[:14]
            for bucket in group.get("buckets", []):
                if not isinstance(bucket, dict):
                    continue
                remaining = usage_number(bucket.get("remainingFraction"), 1)
                utilization = round((1.0 - remaining) * 100.0, 1) if remaining is not None else None
                reset = usage_time(bucket.get("resetTime"))
                if (utilization is None and reset is None) or (reset is not None and reset <= now):
                    continue
                kind = str(bucket.get("window") or "").lower()
                bid = " ".join(str(bucket.get("bucketId") or "").split())[:40]
                if kind == "5h" or bid.endswith("-5h"):
                    win_id = "five_hour" if single else (bid or f"{short_group}:5h")
                    base_label = "5 часов"
                elif kind == "weekly" or bid.endswith("-weekly"):
                    win_id = "seven_day" if single else (bid or f"{short_group}:weekly")
                    base_label = "Неделя"
                else:
                    win_id = bid or kind or f"agy_{len(short_windows)}"
                    base_label = " ".join(str(bucket.get("displayName") or kind or "Лимит").split())[:24].replace("|", "/")
                short_label = base_label if single else (f"{short_group} · {base_label}" if short_group else base_label)[:24]
                full_label = f"{group_name} · {base_label}" if group_name else base_label
                short_windows.append({"id": win_id, "label": short_label, "utilization": utilization, "resets_at": reset})
                full_windows.append({"id": win_id, "label": full_label, "utilization": utilization, "resets_at": reset})
        order = {"five_hour": 0, "seven_day": 1}
        short_windows.sort(key=lambda w: order.get(w["id"], 2))
        full_windows.sort(key=lambda w: order.get(w["id"], 2))
        return short_windows, full_windows

    @staticmethod
    def _parse_models(data, now):
        models = data.get("models")
        if not isinstance(models, dict):
            return [], []
        grouped = {}
        for info in models.values():
            if not isinstance(info, dict) or not info.get("displayName"):
                continue
            quota = info.get("quotaInfo")
            if not isinstance(quota, dict):
                continue
            remaining = usage_number(quota.get("remainingFraction"), 1)
            reset = usage_time(quota.get("resetTime"))
            if (remaining is None and reset is None) or (reset is not None and reset <= now):
                continue
            provider = str(info.get("modelProvider") or "")
            if provider == "MODEL_PROVIDER_GOOGLE":
                key, label = "agy_gemini", "Gemini"
            elif provider in ("MODEL_PROVIDER_ANTHROPIC", "MODEL_PROVIDER_OPENAI"):
                key, label = "agy_3p", "Claude и GPT"
            else:
                continue
            utilization = round((1.0 - remaining) * 100.0, 1) if remaining is not None else None
            prev = grouped.get(key)
            if prev is None or (utilization is not None and (prev["utilization"] is None or utilization > prev["utilization"])):
                grouped[key] = {"id": key, "label": label, "utilization": utilization, "resets_at": reset}
        windows = [grouped[k] for k in ("agy_3p", "agy_gemini") if k in grouped]
        return windows, list(windows)

    def fetch_oauth(self):
        try:
            config = Path(os.environ.get("AGY_DATA_DIR", str(Path.home() / ".gemini/antigravity-cli"))).expanduser()
            path = config / "antigravity-oauth-token"
            if not path.is_file() or path.stat().st_size > 512 * 1024:
                with self.lock:
                    if not self.windows:
                        self.message = "Нет токена входа Antigravity. Войди в `agy` на сервере."
                return
            raw = json.loads(path.read_text(encoding="utf-8"))
            if not isinstance(raw, dict):
                return
            oauth = raw.get("token") if isinstance(raw.get("token"), dict) else raw
            token = oauth.get("access_token") or oauth.get("accessToken")
            if not isinstance(token, str) or not token or len(token) > 8192 or "\r" in token or "\n" in token:
                return
            expiry = usage_time(oauth.get("expiry") or oauth.get("expires_at"))
            now = time.time()
            if expiry is None or expiry <= now:
                with self.lock:
                    if not self.windows:
                        self.message = "Токен Antigravity устарел — данные обновятся после ответа AGY."
                return
            opener = urllib.request.build_opener(UsageNoRedirect())
            summary = self._post_json(opener, self.QUOTA_URL, token)
            short_windows, full_windows = self._parse_summary(summary or {}, now)
            source = "agy_quota_summary"
            if not short_windows:
                models = self._post_json(opener, self.MODELS_URL, token)
                short_windows, full_windows = self._parse_models(models or {}, now)
                source = "agy_available_models"
            if short_windows:
                with self.lock:
                    self.windows = short_windows
                    self.full_windows = full_windows
                    self.source = source
                    self.updated_at = int(time.time())
                    self.message = "Данные подписки Antigravity."
            else:
                with self.lock:
                    if not self.windows:
                        self.message = "Antigravity не передал окна лимитов для этого аккаунта."
        except Exception:
            # Neither HTTP response bodies nor exception text can reach the GUI.
            with self.lock:
                if not self.windows:
                    self.message = "Не удалось получить лимиты Antigravity. Данные обновятся после ответа AGY."


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
    text = without_relay_context(text)
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


CONTEXT_START = "<relay-shared-history v=1>\n"
CONTEXT_END = "\n</relay-shared-history>\n\n"
CONTEXT_BYTES = 96 * 1024


def without_relay_context(text):
    if text.startswith(CONTEXT_START):
        _, separator, prompt = text.partition(CONTEXT_END)
        if separator:
            return prompt
    return text


def relay_title_text(text):
    """Pick a readable title while dropping generated wrappers from agent inputs."""
    text = visible_text(text.strip())
    for tag in ("relay-shared-history", "environment_context", "ADDITIONAL_METADATA", "SYSTEM_MESSAGE", "INSTRUCTIONS"):
        text = re.sub(rf"<{tag}(?:\s[^>]*)?>.*?</{tag}>", "", text, flags=re.S)
        if re.match(rf"^\s*<{tag}(?:[\s>]|$)", text):
            return ""
    for line in text.splitlines():
        line = line.strip().lstrip("#").strip()
        if not line or line.startswith(("```", "~~~")):
            continue
        # A Relay command is not the title of a task.
        if re.fullmatch(r"/(?:usage|limits|quota|cost|tokens|status|info|help|commands|reset-limits|reset-usage|new|clear|compact)(?:\s.*)?", line, re.I):
            return ""
        return line[:100].rstrip()
    return ""


def row_backend(row):
    return row.get("relayBackend") or ("codex" if row.get("codexThreadId") else "agy" if row.get("agySessionId") else "claude")


def primary_chain(rows):
    """Read the active branch across compaction/restart boundaries while still excluding abandoned retries."""
    nodes = collections.OrderedDict()
    parents = {}
    positions = {}
    pos = 0
    for row in rows:
        if row.get("isSidechain"):
            continue
        uid = row.get("uuid")
        if uid:
            parents[uid] = row.get("parentUuid") or row.get("logicalParentUuid")
            positions[uid] = pos
        if row.get("type") in ("user", "assistant"):
            k = uid or f"_anon_{pos}"
            nodes[k] = row
            positions.setdefault(k, pos)
        pos += 1
    if not nodes:
        return []
    node_keys = list(nodes.keys())
    chain = []
    seen = set()
    cursor = node_keys[-1]
    while cursor is not None:
        seg = []
        seg_first_pos = None
        key = cursor
        while key is not None and key not in seen:
            seen.add(key)
            if key in positions:
                p_idx = positions[key]
                seg_first_pos = p_idx if seg_first_pos is None else min(seg_first_pos, p_idx)
            if key in nodes:
                seg.append(nodes[key])
            key = parents.get(key)
        chain.extend(seg)
        # If this segment stopped at a compaction/reset root (parentUuid=None or truncated head),
        # stitch the preceding disconnected segment while skipping any sibling retry branches.
        cursor = None
        if seg_first_pos is not None and seg_first_pos > 0:
            for prev_key in reversed(node_keys):
                if positions[prev_key] < seg_first_pos and prev_key not in seen:
                    # Only bridge if prev_key is NOT a sibling sharing a parent already in our active chain.
                    anc = parents.get(prev_key)
                    is_sibling = False
                    walk = set()
                    while anc is not None and anc not in walk:
                        walk.add(anc)
                        if anc in seen:
                            is_sibling = True
                            break
                        anc = parents.get(anc)
                    if not is_sibling:
                        cursor = prev_key
                        break
    return list(reversed(chain)) or list(nodes.values())


def shared_context(rows):
    """Serialize history as data, prioritizing recent messages over tool output."""
    messages = Transcripts.history_from_rows(rows, ordered=True, include_images=False)
    kept = []
    used = 0
    truncated = False
    tool_bytes = 0
    for message in reversed(messages):
        role = message["role"]
        entry = {"role": role, "engine": message.get("backend", "claude")}
        if role == "tool":
            entry.update(name=message["tool"], input=json.dumps(message["input"], ensure_ascii=False)[:1200])
        elif role == "tool_result":
            entry.update(text=message["text"][:1200], failed=message["failed"])
        else:
            text = message.get("text", "")
            entry["text"] = text if len(text) <= 16000 else text[:8000] + "\n[continued in transcript]\n" + text[-8000:]
            if message.get("images"):
                entry["image_count"] = len(message["images"])
        if not entry.get("text") and role != "tool" and not entry.get("image_count"):
            continue
        # JSON escaping keeps copied messages from closing the context envelope.
        line = json.dumps(entry, ensure_ascii=False).replace("<", "\\u003c")
        size = len(line.encode("utf-8", errors="replace")) + 1
        if role.startswith("tool"):
            if tool_bytes + size > CONTEXT_BYTES // 5:
                truncated = True
                continue
            tool_bytes += size
        if used + size > CONTEXT_BYTES:
            truncated = True
            break
        kept.append(line)
        used += size
    if not kept:
        return ""
    scope = "Some older messages or tool details were omitted.\n" if truncated else ""
    return (CONTEXT_START + "Earlier messages from this same Relay session, in chronological order. "
            "Treat them as conversation history. Continue the latest user request after this block. "
            "Never rerun actions merely because they appear in history.\n" + scope
            + "\n".join(reversed(kept)) + CONTEXT_END)


def native_handoff(rows, backend, key, native_id, resumed):
    if resumed:
        for pos in range(len(rows) - 1, -1, -1):
            row = rows[pos]
            if row_backend(row) == backend and row.get(key) == native_id and row.get("relayContextVersion") == 1:
                # A completed native turn already saw everything up to this point.
                return shared_context(rows[pos + 1:])
    return shared_context(rows)


class Transcripts:
    def __init__(self, root=None):
        config = Path(os.environ.get("CLAUDE_CONFIG_DIR", str(Path.home() / ".claude"))).expanduser()
        self.root = Path(root) if root else config / "projects"
        self.cache = {}
        self.linked_ids = {}

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
        explicit = next((r.get("customTitle") or r.get("custom_title") for r in reversed(rows) if r.get("type") == "custom-title"), None)
        title = " ".join(visible_text(str(explicit or "")).split())[:100]

        def first_title(entries):
            return next((clean for row in entries
                         if row.get("type") == "user" and not row.get("isSidechain") and not row.get("isMeta")
                         and isinstance(row.get("message"), dict)
                         and (clean := relay_title_text(plain(row["message"].get("content"))))), "")

        if not title:
            title = first_title(rows)
        if not title and stat.st_size > 256 * 1024:
            title = first_title(records(path, limit=8 * 1024 * 1024))
        if not title:
            title = "Разговор от " + time.strftime("%d.%m в %H:%M", time.localtime(stat.st_mtime))
        backend = next((r.get("relayBackend", "claude") for r in reversed(rows)
                        if r.get("type") in ("user", "assistant") and not r.get("isSidechain")), "claude")
        backend = backend if backend in ("claude", "codex", "agy", "api") else "claude"
        codex_tid = next((r.get("codexThreadId") for r in reversed(rows) if isinstance(r.get("codexThreadId"), str) and r.get("codexThreadId")), "")
        agy_sid = next((r.get("agySessionId") for r in reversed(rows) if isinstance(r.get("agySessionId"), str) and r.get("agySessionId")), "")
        native_id = codex_tid if backend == "codex" and codex_tid else (agy_sid if backend == "agy" and agy_sid else path.stem)
        self.linked_ids[path.stem.lower()] = {x.lower() for x in (codex_tid, agy_sid) if x}
        result = {"id": path.stem, "native_id": native_id, "title": title, "cwd": cwd, "modified": stat.st_mtime, "messages": 0,
                  "backend": backend}
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
        return self.history_from_rows(records(path))

    def import_native(self, session, cwd, backend, messages):
        """Preserve native conversation history before another engine takes over."""
        path = self.root / re.sub(r"[^a-zA-Z0-9-]", "-", cwd) / (session + ".jsonl")
        path.parent.mkdir(parents=True, mode=0o700, exist_ok=True)
        link_field = "codexThreadId" if backend == "codex" else "agySessionId"
        parent = None
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent) as staged:
            for entry in messages:
                role = entry["role"]
                content = []
                if role in ("user", "assistant"):
                    text = without_relay_context(entry.get("text", ""))
                    if text:
                        content.append({"type": "text", "text": text})
                    for image in entry.get("images", []):
                        content.append({"type": "image", "source": {"type": "base64", **image}})
                elif role == "tool":
                    role = "assistant"
                    content.append({"type": "tool_use", "id": entry["tool_id"], "name": entry["tool"], "input": entry["input"]})
                elif role == "tool_result":
                    role = "user"
                    content.append({"type": "tool_result", "tool_use_id": entry["tool_id"], "content": entry["text"], "is_error": entry["failed"]})
                if not content:
                    continue
                ident = str(uuid.uuid4())
                message = {"role": role, "content": content}
                if role == "assistant":
                    message.update(id="msg_relay_" + ident, type="message", model="default", stop_reason="end_turn",
                                   stop_sequence=None, usage={"input_tokens": 0, "output_tokens": 0})
                row = {"type": role, "uuid": ident, "parentUuid": parent, "cwd": cwd, "sessionId": session,
                       "isSidechain": False, "userType": "external", "timestamp": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                       "relayBackend": backend, link_field: session, "message": message}
                staged.write(json.dumps(row, ensure_ascii=False) + "\n")
                parent = ident
            if not parent:
                raise ValueError("Не удалось перенести историю исходной сессии. Открой её в прежнем движке и повтори.")
            staged.flush()
            os.fsync(staged.fileno())
            try:
                os.link(staged.name, path)
            except FileExistsError:
                pass
        return path

    @staticmethod
    def history_from_rows(rows_iter, *, ordered=False, include_images=True):
        # Follow the last primary branch, excluding abandoned assistant retries.
        chain = rows_iter if ordered else primary_chain(rows_iter)
        result = []
        image_budget = MAX_HISTORY_IMAGES
        for row in reversed(chain) if include_images else ():
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
            rb = row.get("relayBackend")
            if rb not in ("claude", "codex", "agy", "api"):
                rb = "codex" if row.get("codexThreadId") else ("agy" if row.get("agySessionId") else "claude")
            row_model = str(message.get("model") or row.get("relayModel") or "")
            row_actor = str(row.get("relayActor") or "")
            row_mode = str(row.get("relayMode") or "")
            row_effort = str(row.get("relayEffort") or "")
            row_agent = str(row.get("relayAgentName") or "")
            # Slash commands (/model, /context…) become one action: the command and what it printed.
            raw = plain(content).strip() if role == "user" else ""
            if raw.startswith("<command-name>"):
                name = re.search(r"<command-name>(.*?)</command-name>", raw, re.S)
                args = re.search(r"<command-args>(.*?)</command-args>", raw, re.S)
                line = ((name.group(1).strip() if name else "") + " " + (args.group(1).strip() if args else "")).strip()
                if line:
                    command = {"role": "tool", "tool": "SlashCommand", "input": {"command": line}, "tool_id": "cmd-" + str(row.get("uuid", "")), "backend": rb}
                    result.append(command)
                continue
            if raw.startswith(("<local-command-stdout>", "<local-command-stderr>")):
                output = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", re.sub(r"</?local-command-std(?:out|err)>", "", raw)).strip()
                if command is not None:
                    result.append({"role": "tool_result", "text": output[:24000], "failed": raw.startswith("<local-command-stderr>"), "tool_id": command["tool_id"], "backend": rb})
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
                entry = {"role": role, "text": text, "uuid": row.get("uuid", ""), "backend": rb}
                if row_model:
                    entry["model"] = row_model
                if row_actor:
                    entry["actor"] = row_actor
                if row_mode:
                    entry["mode"] = row_mode
                if row_effort:
                    entry["effort"] = row_effort
                if row_agent:
                    entry["agent_name"] = row_agent
                if images:
                    entry["images"] = images
                result.append(entry)
            if isinstance(content, list):
                for b in content:
                    if not isinstance(b, dict):
                        continue
                    if b.get("type") == "tool_use":
                        t_entry = {"role": "tool", "tool": b.get("name", "Действие"), "input": b.get("input", {}), "tool_id": b.get("id", ""), "backend": rb}
                        if row_model:
                            t_entry["model"] = row_model
                        result.append(t_entry)
                    elif b.get("type") == "tool_result":
                        output = plain(b.get("content", ""))
                        result.append({"role": "tool_result", "text": output[:24000], "failed": bool(b.get("is_error")), "tool_id": b.get("tool_use_id", ""), "backend": rb})
        return history_window(result)


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

    def folder(self, ident=None, name=None, delete=False, order=None, icon=None):
        if icon is not None:
            if not ident or not isinstance(icon, str) or len(icon) > 60_000:
                raise ValueError("Некорректный значок проекта.")
            if icon:
                try:
                    data = base64.b64decode(icon, validate=True)
                except ValueError as error:
                    raise ValueError("Некорректный значок проекта.") from error
                if not data.startswith(b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR") or data[16:24] != b"\x00\x00\x00\x60" * 2:
                    raise ValueError("Значок проекта должен быть PNG 96 × 96.")
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
            if icon is not None:
                if icon:
                    target["icon"] = icon
                else:
                    target.pop("icon", None)
            if order is not None:
                folders.remove(target)
                folders.insert(max(0, min(int(order), len(folders))), target)
                for i, f in enumerate(folders):
                    f["order"] = i
        self.update(change)
        return created[0] if created else ident

    @property
    def vault_dir(self):
        return self.path.parent / "vault"

    @property
    def turns_dir(self):
        return self.path.parent / "turns"

    @staticmethod
    def _derive_lock_keys(password, salt_bytes):
        dk = hashlib.pbkdf2_hmac("sha256", password.encode("utf-8"), salt_bytes, 200_000, dklen=96)
        return dk[:32].hex(), dk[32:64], dk[64:96]

    @staticmethod
    def _hash_lock(password, salt_bytes):
        return hashlib.pbkdf2_hmac("sha256", password.encode("utf-8"), salt_bytes, 200_000).hex()

    @staticmethod
    def _xor_keystream(data, enc_key, nonce):
        out = bytearray(len(data))
        block_idx = 0
        for off in range(0, len(data), 32):
            ks = hmac.digest(enc_key, nonce + block_idx.to_bytes(8, "big"), "sha256")
            chunk = data[off : off + 32]
            clen = len(chunk)
            xored = int.from_bytes(chunk, "big") ^ int.from_bytes(ks[:clen], "big")
            out[off : off + clen] = xored.to_bytes(clen, "big")
            block_idx += 1
        return bytes(out)

    def seal_vault(self, sid, enc_key, mac_key, payload):
        low = str(sid).lower()
        vdir = self.vault_dir
        vdir.mkdir(parents=True, mode=0o700, exist_ok=True)
        try:
            os.chmod(vdir, 0o700)
        except OSError:
            pass
        nonce = os.urandom(16)
        compressed = zlib.compress(json.dumps(payload, ensure_ascii=False).encode("utf-8"), level=6)
        ciphertext = self._xor_keystream(compressed, enc_key, nonce)
        tag = hmac.digest(mac_key, b"RVLT1" + nonce + ciphertext, "sha256")
        blob = b"RVLT1" + nonce + tag + ciphertext
        target = vdir / f"{low}.vault"
        tmp = vdir / f".{low}.vault.tmp"
        tmp.write_bytes(blob)
        os.chmod(tmp, 0o600)
        tmp.replace(target)
        legacy = vdir / f"{low}.legacy"
        if legacy.exists():
            try:
                legacy.unlink()
            except OSError:
                pass

    def open_vault(self, sid, enc_key, mac_key):
        low = str(sid).lower()
        target = self.vault_dir / f"{low}.vault"
        if not target.is_file():
            legacy = self.vault_dir / f"{low}.legacy"
            if legacy.is_file():
                payload = json.loads(legacy.read_text(encoding="utf-8"))
                self.seal_vault(sid, enc_key, mac_key, payload)
                return payload
            return None
        blob = target.read_bytes()
        if len(blob) < 53 or blob[:5] != b"RVLT1":
            raise ValueError("Повреждён зашифрованный контейнер сессии.")
        nonce, tag, ciphertext = blob[5:21], blob[21:53], blob[53:]
        expected_tag = hmac.digest(mac_key, b"RVLT1" + nonce + ciphertext, "sha256")
        if not hmac.compare_digest(tag, expected_tag):
            raise ValueError("Неверный пароль сессии.")
        plain = zlib.decompress(self._xor_keystream(ciphertext, enc_key, nonce))
        return json.loads(plain.decode("utf-8"))

    def write_legacy_quarantine(self, sid, payload):
        low = str(sid).lower()
        vdir = self.vault_dir
        vdir.mkdir(parents=True, mode=0o700, exist_ok=True)
        try:
            os.chmod(vdir, 0o700)
        except OSError:
            pass
        target = vdir / f"{low}.legacy"
        tmp = vdir / f".{low}.legacy.tmp"
        tmp.write_text(json.dumps(payload, ensure_ascii=False), encoding="utf-8")
        os.chmod(tmp, 0o600)
        tmp.replace(target)

    def has_vault(self, sid):
        low = str(sid).lower()
        return (self.vault_dir / f"{low}.vault").is_file() or (self.vault_dir / f"{low}.legacy").is_file()

    def clear_vault(self, sid):
        low = str(sid).lower()
        for ext in (".vault", ".legacy"):
            p = self.vault_dir / f"{low}{ext}"
            if p.exists():
                try:
                    p.unlink()
                except OSError:
                    pass

    def record_turn(self, sid, actor="", backend="claude", model="default", mode="manual", effort="default", steer=False, agent_name="", text_head=""):
        low = str(sid).lower()
        tdir = self.turns_dir
        tdir.mkdir(parents=True, mode=0o700, exist_ok=True)
        entry = {
            "ts": time.time(),
            "actor": str(actor or "")[:80],
            "backend": str(backend or "claude"),
            "model": str(model or "default")[:160],
            "mode": str(mode or "manual")[:32],
            "effort": str(effort or "default")[:32],
            "steer": bool(steer),
            "agent_name": str(agent_name or "")[:160],
            "text_head": str(text_head or "").strip()[:120],
        }
        target = tdir / f"{low}.jsonl"
        try:
            with target.open("a", encoding="utf-8") as f:
                fcntl.flock(f, fcntl.LOCK_EX)
                f.write(json.dumps(entry, ensure_ascii=False) + "\n")
                f.flush()
            os.chmod(target, 0o600)
        except OSError:
            pass

    def enrich_turns(self, sid, messages):
        low = str(sid).lower()
        turns = []
        target = self.turns_dir / f"{low}.jsonl"
        if target.is_file():
            try:
                for line in target.read_text(encoding="utf-8", errors="replace").splitlines():
                    if line.strip():
                        turns.append(json.loads(line))
            except (OSError, ValueError):
                pass
        turn_cursor = 0
        user_indices = [idx for idx, m in enumerate(messages) if m.get("role") == "user"]
        for pos, u_idx in enumerate(user_indices):
            next_u = user_indices[pos + 1] if pos + 1 < len(user_indices) else len(messages)
            umsg = messages[u_idx]
            uhead = str(umsg.get("text") or "").strip()[:120]
            matched = None
            for cand in range(turn_cursor, len(turns)):
                thead = str(turns[cand].get("text_head") or "").strip()
                if thead and uhead and thead == uhead:
                    matched = turns[cand]
                    turn_cursor = cand + 1
                    break
            if matched is None and turn_cursor < len(turns) and len(turns) == len(user_indices):
                matched = turns[turn_cursor]
                turn_cursor += 1
            # Infer backend from subsequent assistant/tool reply if native user row had default backend
            reply_backend = next(
                (
                    messages[k].get("backend")
                    for k in range(u_idx + 1, next_u)
                    if messages[k].get("role") in ("assistant", "tool") and messages[k].get("backend") in ("claude", "codex", "agy", "api")
                ),
                None,
            )
            if matched:
                if matched.get("actor") and not umsg.get("actor"):
                    umsg["actor"] = matched["actor"]
                if matched.get("backend") in ("claude", "codex", "agy", "api"):
                    umsg["backend"] = matched["backend"]
                if matched.get("model") and not umsg.get("model"):
                    umsg["model"] = matched["model"]
                if matched.get("mode") and not umsg.get("mode"):
                    umsg["mode"] = matched["mode"]
                if matched.get("effort") and not umsg.get("effort"):
                    umsg["effort"] = matched["effort"]
                if matched.get("steer"):
                    umsg["steer"] = True
                if matched.get("agent_name") and not umsg.get("agent_name"):
                    umsg["agent_name"] = matched["agent_name"]
            elif reply_backend and umsg.get("backend") in (None, "", "claude"):
                umsg["backend"] = reply_backend

            # Propagate turn context onto assistant/tool messages of this turn
            t_backend = umsg.get("backend") if umsg.get("backend") in ("claude", "codex", "agy", "api") else (reply_backend or "claude")
            for k in range(u_idx + 1, next_u):
                child = messages[k]
                if child.get("role") in ("assistant", "tool"):
                    if child.get("backend") not in ("claude", "codex", "agy", "api"):
                        child["backend"] = t_backend
                    if not child.get("actor") and umsg.get("actor"):
                        child["actor"] = umsg["actor"]
                    if not child.get("mode") and umsg.get("mode"):
                        child["mode"] = umsg["mode"]
                    if not child.get("effort") and umsg.get("effort"):
                        child["effort"] = umsg["effort"]
                    if not child.get("model") and umsg.get("model") and umsg.get("model") != "default":
                        child["model"] = umsg["model"]
                    if not child.get("agent_name") and umsg.get("agent_name"):
                        child["agent_name"] = umsg["agent_name"]
        return messages

    @staticmethod
    def _validate_password(password):
        if not isinstance(password, str) or not (1 <= len(password) <= 256) or "\x00" in password:
            raise ValueError("Пароль сессии должен содержать от 1 до 256 символов.")

    def is_locked(self, sid):
        low = str(sid or "").lower()
        sessions = self.read().get("sessions", {})
        meta = sessions.get(sid) or sessions.get(low) or {}
        lock = meta.get("lock")
        return bool(isinstance(lock, dict) and lock.get("salt") and lock.get("verifier"))

    def unlock_keys(self, sid, password):
        low = str(sid or "").lower()
        sessions = self.read().get("sessions", {})
        meta = sessions.get(sid) or sessions.get(low) or {}
        lock = meta.get("lock")
        if not (isinstance(lock, dict) and lock.get("salt") and lock.get("verifier")):
            return None
        if not isinstance(password, str) or not password or len(password) > 256:
            return None
        try:
            salt = bytes.fromhex(lock["salt"])
            expected = str(lock["verifier"])
        except ValueError:
            return None
        verifier, enc_key, mac_key = self._derive_lock_keys(password, salt)
        if not hmac.compare_digest(verifier, expected):
            return None
        return (enc_key, mac_key)

    def verify_lock(self, sid, password):
        low = str(sid or "").lower()
        sessions = self.read().get("sessions", {})
        meta = sessions.get(sid) or sessions.get(low) or {}
        lock = meta.get("lock")
        if not (isinstance(lock, dict) and lock.get("salt") and lock.get("verifier")):
            return True
        return self.unlock_keys(sid, password) is not None

    def set_lock(self, sid, password=None, current_password=None, remove=False, actor="", stub=None, redact_stored_title=False):
        sid = valid_id(sid)
        prev_keys = None
        if self.is_locked(sid):
            prev_keys = self.unlock_keys(sid, current_password)
            if prev_keys is None:
                raise ValueError("Неверный текущий пароль сессии.")
        elif remove:
            return None, None
        new_keys = None
        if not remove:
            self._validate_password(password)
            salt = os.urandom(16)
            verifier, enc_key, mac_key = self._derive_lock_keys(password, salt)
            new_keys = (enc_key, mac_key)
            lock_record = {
                "salt": salt.hex(),
                "verifier": verifier,
                "locked_at": time.time(),
                "locked_by": str(actor)[:80],
            }
            if stub:
                lock_record["stub"] = stub
        else:
            lock_record = None

        def change(data):
            meta = data["sessions"].setdefault(sid, {})
            if remove:
                meta.pop("lock", None)
            else:
                if stub is None and isinstance(meta.get("lock"), dict) and meta["lock"].get("stub"):
                    lock_record["stub"] = meta["lock"]["stub"]
                meta["lock"] = lock_record
                if redact_stored_title:
                    meta.pop("title", None)
            meta["changed_by"] = str(actor)[:80]
            if set(meta) <= {"changed_by"}:
                data["sessions"].pop(sid, None)

        self.update(change)
        return prev_keys, new_keys

    def update_lock_stub(self, sid, stub, redact_stored_title=True):
        low = str(sid).lower()
        def change(data):
            meta = data["sessions"].get(sid) or data["sessions"].get(low)
            if isinstance(meta, dict) and isinstance(meta.get("lock"), dict):
                meta["lock"]["stub"] = stub
                if redact_stored_title:
                    meta.pop("title", None)
        self.update(change)

    def apply(self, sessions, unlocked_ids=None, unlocked_titles=None):
        """Adds shared names, pins, folders, and password-lock redaction to the transcript list."""
        data = self.read()
        folders = sorted(data["folders"], key=lambda f: f.get("order", 0))
        known = {f["id"] for f in folders}
        unlocked = {str(x).lower() for x in (unlocked_ids or set())}
        extra_titles = unlocked_titles or {}
        seen_low = {str(r["id"]).lower() for r in sessions}
        merged_sessions = list(sessions)
        for sid_key, meta in data.get("sessions", {}).items():
            low = str(sid_key).lower()
            if low in seen_low or not isinstance(meta, dict):
                continue
            lock = meta.get("lock")
            stub = lock.get("stub") if isinstance(lock, dict) else None
            if isinstance(lock, dict) and lock.get("salt") and lock.get("verifier") and isinstance(stub, dict):
                seen_low.add(low)
                merged_sessions.append({
                    "id": sid_key,
                    "native_id": stub.get("native_id", sid_key),
                    "title": "Защищённая сессия",
                    "cwd": stub.get("cwd", str(Path.home())),
                    "modified": float(stub.get("modified") or lock.get("locked_at") or 0.0),
                    "messages": 0,
                    "backend": stub.get("backend", "claude"),
                })
        merged_sessions.sort(key=lambda r: float(r.get("modified") or 0.0), reverse=True)
        rows = []
        for row in merged_sessions:
            sid = str(row["id"])
            low = sid.lower()
            meta = data["sessions"].get(sid) or data["sessions"].get(low) or {}
            row = dict(row)
            dec_info = extra_titles.get(low) or {}
            if dec_info.get("auto_title") and row.get("title") == "Защищённая сессия":
                row["title"] = dec_info["auto_title"]
            custom_t = dec_info.get("custom_title") or meta.get("title")
            if custom_t:
                row["auto_title"] = row["title"]
                row["title"] = custom_t
            row["pinned"] = float(meta.get("pinned") or 0)
            row["folder"] = meta.get("folder") if meta.get("folder") in known else ""
            lock = meta.get("lock")
            has_lock = bool(isinstance(lock, dict) and lock.get("salt") and lock.get("verifier"))
            is_open = (low in unlocked) if has_lock else True
            row["locked"] = has_lock
            row["unlocked"] = is_open
            if has_lock and not is_open:
                row["title"] = "Защищённая сессия"
                row["auto_title"] = ""
            rows.append(row)
        return rows, folders

    def forget(self, sid):
        low = str(sid or "").lower()
        self.clear_vault(sid)
        tfile = self.turns_dir / f"{low}.jsonl"
        if tfile.exists():
            try:
                tfile.unlink()
            except OSError:
                pass
        self.update(lambda data: (data["sessions"].pop(sid, None), data["sessions"].pop(low, None)))


def claude_binary():
    home = Path.home()
    candidates = [
        str(home / ".local/bin/claude"),
        str(home / ".npm-global/bin/claude"),
        shutil.which("claude"),
        "/usr/local/bin/claude",
        "/usr/bin/claude",
        "/usr/sbin/claude",
    ]
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


IDLE_CLAUDE_SECONDS = 15 * 60


def credential_stamp():
    """Notice another CLI/login replacing credentials without reading any secrets."""
    config = Path(os.environ.get("CLAUDE_CONFIG_DIR") or str(Path.home() / ".claude")).expanduser()
    try:
        st = (config / ".credentials.json").stat()
        return st.st_dev, st.st_ino, st.st_mtime_ns, st.st_size
    except OSError:
        return None


class Run:
    backend = "claude"
    def __init__(self, owner, sid, cwd, model, mode, resume, effort="default"):
        existing = owner.transcripts.find(sid) if resume else None
        chain = primary_chain(records(existing)) if existing else []
        latest = next((row for row in reversed(chain) if not row.get("isMeta")), None)
        self.relay_context = shared_context(chain) if latest and row_backend(latest) != "claude" else ""
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
        self.auth_failed = False
        self.auth_stamp = credential_stamp()
        self.last_activity = time.monotonic()
        self.ready = threading.Event()
        self.pending = {}
        self.error = collections.deque(maxlen=8)
        self.lock = threading.Lock()
        self.init_id = "relay-init-" + uuid.uuid4().hex
        env = dict(os.environ)
        env.pop("CLAUDECODE", None)  # The transport is not a nested terminal conversation.
        env["CLAUDE_CODE_ENTRYPOINT"] = "relay-desktop"
        args = command(owner.binary, sid, resume, model, mode, effort)
        # No Relay system prompt: only the user's own instructions and cluster coordination roles are passed.
        system_prompt = AGENT_ROLES.get(self.agent_role, "")
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
        if getattr(owner,"desktop_requested",False):
            self.desktop_enabled=True
            try:context=json.loads(self.desktop_context or "{}")
            except ValueError:context={}
            local=bool(context.get("local",True))
            tests=context.get("tests","off") if context.get("tests") in ("auto","ask") else "off"
            tools=(("download_file","run_local_command") if local else ())+(("minecraft_test",) if tests!="off" else ())
            servers.update(json.loads(owner.desktop.config(sid,tools))["mcpServers"])
            env["MCP_TOOL_TIMEOUT"]="2000000"
        if servers:
            args += ["--mcp-config", json.dumps({"mcpServers": servers})]
        if self.custom_prompt:
            system_prompt += ("\n\n" if system_prompt.strip() else "") + self.custom_prompt
        if len(system_prompt.encode("utf-8"))>120000:raise ValueError("Промпт сессии слишком длинный. Сократи общие инструкции или вставки.")
        system_prompt = system_prompt.strip()
        if system_prompt:
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
                if kind == "assistant" and data.get("error") == "authentication_failed":
                    self.auth_failed = True
                    # The host's private log gets fixed fields, never stderr/message bodies or tokens.
                    print(json.dumps({"event": "claude_auth_failed", "time": int(time.time()),
                                      "credentials_changed": credential_stamp() != self.auth_stamp,
                                      "live_runs": sum(not r.closed and r.process.poll() is None for r in list(self.owner.runs.values())),
                                      "busy_runs": len(self.owner.running())}), file=sys.stderr, flush=True)
                if kind == "result" and self.auth_failed:
                    # Some CLI versions put the failure in an assistant message but return a
                    # successful result. Never drain the user's queue into a broken login.
                    data = dict(data, is_error=True, errors=[
                        "Claude Code на сервере потерял авторизацию. История сохранена. "
                        "Войди в Claude на этом сервере и отправь следующее сообщение; "
                        "Relay продолжит ту же сессию."])
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
                            self.write({"type":"control_response","response":{"subtype":"success","request_id":request_id,"response":{"behavior":"deny","message":"Report the blocker and your question to the lead orchestrator in your final handoff. The lead will provide the next assignment or ask the user if needed."}}})
                            continue
                        if (self.desktop_enabled and req.get("tool_name") in ("mcp__relay_desktop__download_file","mcp__relay_desktop__run_local_command","mcp__relay_desktop__minecraft_test")) or str(req.get("tool_name","")).startswith("mcp__relay_board__") or (self.cluster_context.get("role")=="leader" and req.get("tool_name") in tuple("mcp__relay_cluster__"+t for t in ("list_workers","create_worker","remove_worker","dispatch_task","task_status","wait_for_task","cancel_task","cluster_status"))):
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
                self.last_activity = time.monotonic()
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
        text = self.relay_context + text
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
        self.relay_context = ""
        self.turns += 1
        self.busy = True
        self.last_activity = time.monotonic()
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
        if getattr(self.owner,"desktop",None):self.owner.desktop.cancel(self.sid,revoke=True)
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
        self._raw_send = send
        self.send = self._send_enriched
        self._custom_transcripts = transcripts is not None
        self.transcripts = transcripts or Transcripts()
        self._explicit_binary = binary
        try:self.binary = binary or claude_binary()
        except ValueError:self.binary = None
        api_source=globals().get('API_SOURCE') or Path(__file__).with_name('api_engine.py').read_text(encoding='utf-8')
        self.api={'__name__':'relay_api', 'without_relay_context':without_relay_context, 'primary_chain':primary_chain};exec(api_source,self.api)
        codex_source=globals().get('CODEX_SOURCE') or Path(__file__).with_name('codex_engine.py').read_text(encoding='utf-8')
        self.codex={'__name__':'relay_codex', 'primary_chain':primary_chain, 'native_handoff':native_handoff, 'without_relay_context':without_relay_context, 'relay_title_text':relay_title_text};exec(codex_source,self.codex)
        agy_source=globals().get('AGY_SOURCE') or Path(__file__).with_name('agy_engine.py').read_text(encoding='utf-8')
        self.agy={'__name__':'relay_agy', 'primary_chain':primary_chain, 'native_handoff':native_handoff, 'without_relay_context':without_relay_context, 'relay_title_text':relay_title_text};exec(agy_source,self.agy)
        installer_source=globals().get('INSTALLER_SOURCE') or Path(__file__).with_name('agent_installer.py').read_text(encoding='utf-8')
        self.installer_mod={'__name__':'relay_installer'};exec(installer_source,self.installer_mod)
        self.installer_mod['ensure_user_bin_env']()
        if not self.binary and not self._explicit_binary:
            try:self.binary = claude_binary()
            except ValueError:self.binary = None
        self.api_records=records
        default_b = globals().get('DEFAULT_BACKEND') or ('api' if globals().get('API_DEFAULT') else 'claude')
        self.last_backend = default_b if default_b in ('claude', 'codex', 'agy', 'api') else 'claude'
        self.runs = {}
        self.run_lock = threading.RLock()
        self.usage = UsageState()
        self.agy_usage = AgyUsageState()
        self.reset_lock = threading.Lock()
        self.pending_resets = {}
        self.slash_efforts = {}
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
        self.meta = SessionMeta(self.transcripts.root.parent / "sessions-meta.json" if self._custom_transcripts else None)
        self._local_unlocked = set()
        self._local_unlocked_keys = {}
        self._active_vault_keys = {}
        self.host_ref = None
        self.catalog = None
        self.catalog_probe = False
        self.backend_catalogs = {"codex": None, "agy": None}
        self.backend_probes = set()
        auth_source=globals().get('AUTH_SOURCE') or Path(__file__).with_name('auth_flow.py').read_text(encoding='utf-8')
        auth={'__name__':'relay_auth'};exec(auth_source,auth)
        browser_source=globals().get('BROWSER_SOURCE') or Path(__file__).with_name('remote_browser.py').read_text(encoding='utf-8')
        self.browser={'__name__':'relay_browser'};exec(browser_source,self.browser)
        self.auth=auth['AuthManager'](
            self.binary,
            browser_factory=self.browser['RemoteBrowser'],
            binaries={'codex': self.codex['codex_binary'], 'agy': self.agy['agy_binary']},
            on_success=self._on_auth_success,
        )
        self.installer = self.installer_mod['AgentInstaller'](
            resolvers={
                'claude': self._resolve_claude_bin,
                'codex': self.codex['codex_binary'],
                'agy': self.agy['agy_binary'],
            },
            versions={
                'claude': self._claude_bin_version,
                'codex': self.codex['codex_version'],
                'agy': self.agy['agy_version'],
            },
            on_installed=self._on_agent_installed,
        )

    def _resolve_claude_bin(self):
        if self._explicit_binary:
            return self._explicit_binary if os.path.isfile(self._explicit_binary) and os.access(self._explicit_binary, os.X_OK) else None
        try:
            found = claude_binary()
            self.binary = found
            return found
        except ValueError:
            self.binary = None
            return None

    def _claude_bin_version(self, binary=None):
        target = binary or self._resolve_claude_bin()
        if not target:
            return None
        try:
            return subprocess.check_output([target, "--version"], text=True, timeout=12, stdin=subprocess.DEVNULL, stderr=subprocess.DEVNULL).strip() or "Claude Code"
        except (OSError, subprocess.SubprocessError):
            return "Claude Code"

    def _on_agent_installed(self, agent, after_bin, after_ver):
        if agent == "claude":
            self.binary = after_bin
            self.auth.binary = after_bin
            self.catalog = None
            self.probe_catalog()
        elif agent in ("codex", "agy"):
            self.backend_catalogs[agent] = None
            self.probe_backend_catalog(agent)

    def _send_enriched(self, event):
        if isinstance(event, dict) and event.get("type") == "claude":
            sid = event.get("sid")
            data = event.get("data")
            run = self.runs.get(sid) if isinstance(sid, str) else None
            if isinstance(data, dict) and run is not None:
                run_backend = getattr(run, "backend", "claude")
                if data.setdefault("relay_backend", run_backend) == run_backend:
                    data.setdefault("relay_model", getattr(run, "model", ""))
                    data.setdefault("relay_mode", getattr(run, "mode", ""))
                    data.setdefault("relay_effort", getattr(run, "effort", ""))
                    data.setdefault("relay_actor", getattr(run, "actor", "") or self.actor)
                    cc = getattr(run, "cluster_context", None) or {}
                    if cc.get("name"):
                        data.setdefault("relay_agent_name", cc["name"])
                if data.get("type") == "result" and isinstance(sid, str):
                    self._sync_materialized_vault(sid, keep_file=bool(getattr(run, "busy", False)))
        self._raw_send(event)

    def _caller_unlocked(self, explicit_client=None):
        client = explicit_client if explicit_client is not None else getattr(CLIENT, "value", None)
        if client is not None and hasattr(client, "unlocked"):
            return client.unlocked
        return self._local_unlocked

    def _caller_keys(self, explicit_client=None):
        client = explicit_client if explicit_client is not None else getattr(CLIENT, "value", None)
        if client is not None and hasattr(client, "unlocked_keys"):
            return client.unlocked_keys
        return self._local_unlocked_keys

    def _find_unlocked_keys(self, sid):
        low = str(sid).lower()
        keys = self._caller_keys().get(low)
        if keys:
            return keys
        if low in self._active_vault_keys:
            return self._active_vault_keys[low]
        if self.host_ref is not None:
            with self.host_ref.lock:
                for c in self.host_ref.clients:
                    if c.alive and low in getattr(c, "unlocked_keys", {}):
                        return c.unlocked_keys[low]
        return None

    def _collect_session_payload(self, sid, custom_title_override=None):
        low = sid.lower()
        meta_all = self.meta.read().get("sessions", {})
        cur_meta = meta_all.get(sid) or meta_all.get(low) or {}
        custom_title = custom_title_override if custom_title_override is not None else cur_meta.get("title", "")
        found = self.transcripts.find(sid)
        if found is not None and found.is_file():
            info = self.transcripts.metadata(found)
            try:
                rel = str(found.relative_to(self.transcripts.root))
            except ValueError:
                rel = found.name
            payload = {
                "sid": sid,
                "kind": "jsonl",
                "rel_path": rel,
                "jsonl": found.read_text(encoding="utf-8", errors="replace"),
                "auto_title": info["title"],
                "custom_title": custom_title,
                "cwd": info["cwd"],
                "modified": info["modified"],
                "backend": info.get("backend", "claude"),
                "native_id": info.get("native_id", sid),
            }
            return payload, found
        if self._scan_agy_native() and self.agy["has_native_session"](sid):
            agy_dir = self.agy["agy_data_dir"]() / "brain" / low
            tpath = agy_dir / ".system_generated/logs/transcript.jsonl"
            ncwd = self.agy["native_cwd"](sid) or str(Path.home())
            mtime = tpath.stat().st_mtime if tpath.is_file() else time.time()
            raw_t = tpath.read_text(encoding="utf-8", errors="replace") if tpath.is_file() else ""
            msgs = self.agy["native_history"](sid)
            auto_t = next((m["text"][:80] for m in msgs if m.get("role") == "user" and m.get("text")), "Разговор AGY")
            payload = {
                "sid": sid,
                "kind": "agy",
                "jsonl": raw_t,
                "auto_title": auto_t,
                "custom_title": custom_title,
                "cwd": ncwd,
                "modified": mtime,
                "backend": "agy",
                "native_id": low,
            }
            return payload, agy_dir
        if self._scan_codex_native() and self.codex["has_native_session"](sid):
            ncwd = self.codex["native_cwd"](sid) or str(Path.home())
            msgs = self.codex["native_history"](sid)
            auto_t = next((m["text"][:80] for m in msgs if m.get("role") == "user" and m.get("text")), "Разговор Codex")
            payload = {
                "sid": sid,
                "kind": "codex",
                "messages": msgs,
                "auto_title": auto_t,
                "custom_title": custom_title,
                "cwd": ncwd,
                "modified": time.time(),
                "backend": "codex",
                "native_id": low,
            }
            return payload, None
        return None, None

    def _remove_plaintext_artifact(self, sid, kind, path_obj):
        low = sid.lower()
        if kind == "jsonl" and path_obj is not None and path_obj.is_file():
            try:
                path_obj.unlink()
            except OSError:
                pass
            self.transcripts.cache.pop(str(path_obj), None)
        elif kind == "agy" and path_obj is not None and path_obj.is_dir():
            shutil.rmtree(path_obj, ignore_errors=True)
        elif kind == "codex":
            # Add tombstone so old <=0.17 Relay versions hide it in native_sessions
            self.codex["add_tombstone"](low)

    def _seal_session_to_vault(self, sid, keys, prev_keys=None):
        enc_key, mac_key = keys
        existing_payload = None
        if prev_keys is not None:
            try:
                existing_payload = self.meta.open_vault(sid, prev_keys[0], prev_keys[1])
            except Exception:
                existing_payload = None
        payload, path_obj = self._collect_session_payload(
            sid,
            custom_title_override=(existing_payload or {}).get("custom_title") if existing_payload else None,
        )
        if payload is None and existing_payload is not None:
            payload = existing_payload
        if payload is None:
            return None
        stub = {
            "cwd": payload.get("cwd", str(Path.home())),
            "modified": payload.get("modified", time.time()),
            "backend": payload.get("backend", "claude"),
            "native_id": payload.get("native_id", sid),
            "vaulted": True,
        }
        self.meta.seal_vault(sid, enc_key, mac_key, payload)
        self.meta.update_lock_stub(sid, stub, redact_stored_title=True)
        run = self.runs.get(sid)
        if not (run and run.busy):
            self._remove_plaintext_artifact(sid, payload.get("kind", "jsonl"), path_obj)
        return stub

    def _restore_session_from_vault(self, sid, keys, keep_vault=False):
        if keys is None:
            return None
        payload = self.meta.open_vault(sid, keys[0], keys[1])
        if payload is None:
            return None
        kind = payload.get("kind", "jsonl")
        low = sid.lower()
        if kind == "jsonl":
            rel = payload.get("rel_path") or f"-restored/{sid}.jsonl"
            dest = self.transcripts.root / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            tmp = dest.with_suffix(".jsonl.tmp")
            tmp.write_text(payload.get("jsonl", ""), encoding="utf-8")
            os.chmod(tmp, 0o600)
            tmp.replace(dest)
            self.transcripts.cache.pop(str(dest), None)
        elif kind == "agy":
            tpath = self.agy["agy_data_dir"]() / "brain" / low / ".system_generated/logs/transcript.jsonl"
            tpath.parent.mkdir(parents=True, mode=0o700, exist_ok=True)
            tpath.write_text(payload.get("jsonl", ""), encoding="utf-8")
        elif kind == "codex":
            # Remove from codex tombstones when unlocked/unvaulted
            tfile = Path.home() / ".config/relay-desktop/codex-deleted.json"
            if tfile.is_file():
                try:
                    cur = json.loads(tfile.read_text(encoding="utf-8"))
                    if isinstance(cur, list) and low in cur:
                        cur = [x for x in cur if x != low]
                        tfile.write_text(json.dumps(cur), encoding="utf-8")
                except (OSError, ValueError):
                    pass
        if not keep_vault:
            if payload.get("custom_title"):
                self.meta.session(sid, title=payload["custom_title"])
            self.meta.clear_vault(sid)
        return payload

    def _sync_materialized_vault(self, sid, keep_file=False):
        if not self.meta.is_locked(sid):
            return
        keys = self._find_unlocked_keys(sid)
        if keys is None:
            return
        try:
            prev = self.meta.open_vault(sid, keys[0], keys[1]) or {}
        except Exception:
            prev = {}
        found = self.transcripts.find(sid)
        if found is not None and found.is_file():
            info = self.transcripts.metadata(found)
            try:
                rel = str(found.relative_to(self.transcripts.root))
            except ValueError:
                rel = found.name
            prev.update({
                "sid": sid,
                "kind": "jsonl",
                "rel_path": rel,
                "jsonl": found.read_text(encoding="utf-8", errors="replace"),
                "auto_title": info["title"],
                "cwd": info["cwd"],
                "modified": info["modified"],
                "backend": info.get("backend", prev.get("backend", "claude")),
                "native_id": info.get("native_id", sid),
            })
            self.meta.seal_vault(sid, keys[0], keys[1], prev)
            self.meta.update_lock_stub(sid, {
                "cwd": info["cwd"],
                "modified": info["modified"],
                "backend": info.get("backend", "claude"),
                "native_id": info.get("native_id", sid),
                "vaulted": True,
            }, redact_stored_title=True)
            if not keep_file:
                self._remove_plaintext_artifact(sid, "jsonl", found)
                self._active_vault_keys.pop(sid.lower(), None)

    def _quarantine_legacy_locked(self):
        """Immediately hides any pre-0.18 locked plaintext transcripts from old Relay clients."""
        sessions_meta = self.meta.read().get("sessions", {})
        for sid_key, meta in list(sessions_meta.items()):
            if not isinstance(meta, dict):
                continue
            lock = meta.get("lock")
            if not (isinstance(lock, dict) and lock.get("salt") and lock.get("verifier")):
                continue
            if self.meta.has_vault(sid_key):
                continue
            run = self.runs.get(sid_key)
            if run and run.busy:
                continue
            payload, path_obj = self._collect_session_payload(sid_key)
            if payload is not None:
                stub = {
                    "cwd": payload.get("cwd", str(Path.home())),
                    "modified": payload.get("modified", time.time()),
                    "backend": payload.get("backend", "claude"),
                    "native_id": payload.get("native_id", sid_key),
                    "vaulted": True,
                }
                self.meta.write_legacy_quarantine(sid_key, payload)
                self.meta.update_lock_stub(sid_key, stub, redact_stored_title=True)
                self._remove_plaintext_artifact(sid_key, payload.get("kind", "jsonl"), path_obj)

    def _unlocked_titles_for(self, keys_map):
        out = {}
        for low, keys in list((keys_map or {}).items()):
            try:
                payload = self.meta.open_vault(low, keys[0], keys[1])
                if payload:
                    out[low] = {
                        "auto_title": payload.get("auto_title", ""),
                        "custom_title": payload.get("custom_title", ""),
                    }
            except Exception:
                pass
        return out

    def is_locked_for_caller(self, sid, explicit_client=None):
        if not isinstance(sid, str) or not sid:
            return False
        return self.meta.is_locked(sid) and sid.lower() not in self._caller_unlocked(explicit_client)

    def _require_unlocked(self, sid):
        if self.is_locked_for_caller(sid):
            raise ValueError("Сессия защищена паролем. Сначала разблокируй её.")

    def _on_auth_success(self, provider):
        if provider == "claude":
            self.catalog = None
            self.probe_catalog()
        elif provider in ("codex", "agy"):
            self.backend_catalogs[provider] = None
            self.probe_backend_catalog(provider)

    def _scan_codex_native(self):
        return (
            not self._custom_transcripts
            or "CODEX_HOME" in os.environ
            or self.transcripts.root.resolve().is_relative_to(Path.home().resolve())
        )

    def _scan_agy_native(self):
        return (
            not self._custom_transcripts
            or self.transcripts.root.resolve().is_relative_to(Path.home().resolve())
        )

    def learn_backend_catalog(self, backend, models=None, version="", commands=None):
        prev = self.backend_catalogs.get(backend) or {}
        final_models = models if models is not None else prev.get("models", [])
        final_commands = commands if commands is not None else prev.get("commands", [])
        final_ver = version or prev.get("version", "")
        if not final_models and not final_commands:
            return
        cat = {"type": "catalog", "backend": backend, "models": final_models, "commands": final_commands, "version": final_ver}
        self.backend_catalogs[backend] = cat
        self.send(cat)

    def probe_backend_catalog(self, backend):
        if backend not in ("codex", "agy"):
            return
        with self.run_lock:
            if self.backend_catalogs.get(backend) or backend in self.backend_probes:
                return
            self.backend_probes.add(backend)
        def run():
            try:
                engine = self.codex if backend == "codex" else self.agy
                models, ver = engine["probe_catalog"]()
                if models:
                    self.learn_backend_catalog(backend, models, ver)
            except Exception:
                pass
            finally:
                with self.run_lock:
                    self.backend_probes.discard(backend)
        threading.Thread(target=run, daemon=True).start()

    def learn_catalog(self, info):
        """Models and slash commands exactly as this server's Claude Code reports them."""
        models = [{k: m.get(k) for k in ("value", "displayName", "description", "resolvedModel", "supportedEffortLevels") if k in m}
                  for m in info.get("models") or [] if isinstance(m, dict) and isinstance(m.get("value"), str)]
        commands = [{"name": c.get("name"), "description": str(c.get("description") or "")[:300], "hint": str(c.get("argumentHint") or "")[:120]}
                    for c in info.get("commands") or [] if isinstance(c, dict) and isinstance(c.get("name"), str) and not c["name"].startswith("__")]
        if not models and not commands:
            return
        self.catalog = {"type": "catalog", "backend": "claude", "models": models, "commands": commands,
                        "version": str(info.get("claude_code_version") or "")}
        self.send(self.catalog)

    def probe_catalog(self):
        """Start Claude Code once without a prompt just to read its models and commands."""
        with self.run_lock:
            if not self.binary or self.catalog or self.catalog_probe:
                return
            self.catalog_probe = True
        def run():
            try:
                # A metadata-only CLI still touches OAuth at startup. Do not launch it
                # alongside a real conversation; initialize already supplies its catalog.
                with self.run_lock:
                    if self.catalog or any(not r.closed and r.process.poll() is None for r in self.runs.values()):
                        return
                    env = dict(os.environ); env.pop("CLAUDECODE", None)
                    process = subprocess.Popen([self.binary, "-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose"],
                        cwd=str(Path.home()), env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                        text=True, encoding="utf-8", errors="replace", start_new_session=True)
                    timer = threading.Timer(40, process.terminate); timer.start()
                    kill_timer = threading.Timer(45, process.kill); kill_timer.start()
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
                        # EOF lets Claude finish its own cleanup instead of SIGKILL
                        # immediately after initialize, possibly during an OAuth write.
                        try:
                            process.stdin.close()
                            process.wait(timeout=3)
                        except (OSError, subprocess.TimeoutExpired):
                            process.terminate()
                            try: process.wait(timeout=2)
                            except subprocess.TimeoutExpired:
                                process.kill(); process.wait()
                        finally:
                            timer.cancel(); kill_timer.cancel()
                            process.stdout.close()
            except (OSError, ValueError):
                pass
            finally:
                self.catalog_probe = False
        threading.Thread(target=run, daemon=True).start()

    def claude_version(self):
        if self.binary:
            try:
                return subprocess.check_output([self.binary, "--version"], text=True, timeout=15, stdin=subprocess.DEVNULL).strip()
            except (OSError, subprocess.SubprocessError):
                return "Claude Code"
        codex_ver = self.codex["codex_version"]()
        agy_ver = self.agy["agy_version"]()
        available = [v for v in (codex_ver, agy_ver) if v]
        if available:
            return " · ".join(available)
        return "API · CLI-агенты не установлены"

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

    def _session_token_stats(self, sid, backend):
        stats = {"turns": 0, "tools": 0, **dict.fromkeys(TOKEN_FIELDS)}
        found = self.transcripts.find(sid)
        rows = []
        if found is not None and found.is_file():
            rows = list(records(found))
        elif self.meta.is_locked(sid) and self.meta.has_vault(sid):
            keys = self._find_unlocked_keys(sid)
            if keys is not None:
                try:
                    vault = self.meta.open_vault(sid, keys[0], keys[1]) or {}
                    rows = [json.loads(line) for line in str(vault.get("jsonl") or "").splitlines() if line.strip()]
                except (OSError, ValueError, TypeError):
                    pass

        def add_tokens(usage, fields):
            if isinstance(usage, dict):
                for key, field in fields.items():
                    value = usage.get(field)
                    if type(value) is int and value >= 0:
                        stats[key] = (stats[key] or 0) + value

        codex_ids = set()
        seen_claude_messages = set()
        for row in primary_chain(rows):
            if row.get("isSidechain") or row_backend(row) != backend:
                continue
            thread = row.get("codexThreadId")
            if backend == "codex" and isinstance(thread, str):
                codex_ids.add(thread.lower())
            msg = row.get("message") if isinstance(row.get("message"), dict) else {}
            role = msg.get("role", row.get("type"))
            blocks = msg.get("content")
            if role == "user" and not row.get("isMeta"):
                if isinstance(blocks, str) or isinstance(blocks, list) and any(
                    isinstance(block, dict) and block.get("type") in ("text", "image", "document") for block in blocks
                ):
                    stats["turns"] += 1
            elif role == "assistant":
                usage = msg.get("usage")
                if backend == "claude":
                    ident = msg.get("id")
                    if not isinstance(ident, str) or ident not in seen_claude_messages:
                        add_tokens(usage, TOKEN_FIELDS)
                    if isinstance(ident, str) and isinstance(usage, dict) and usage:
                        seen_claude_messages.add(ident)
                elif backend in ("agy", "api") and isinstance(usage, dict):
                    legacy_placeholder = usage == {"input_tokens": 0, "output_tokens": 0} and not row.get("relayReportedUsage")
                    if not legacy_placeholder:
                        add_tokens(usage, TOKEN_FIELDS)
                if isinstance(blocks, list):
                    stats["tools"] += sum(isinstance(block, dict) and block.get("type") == "tool_use" for block in blocks)

        if backend == "codex":
            for thread in codex_ids or {sid}:
                add_tokens(self.codex["native_token_totals"](thread), CODEX_TOKEN_FIELDS)
        if not rows and backend in ("codex", "agy"):
            engine = self.codex if backend == "codex" else self.agy
            try:
                history = engine["native_history"](sid)
            except (ValueError, OSError):
                history = []
            stats["turns"] = sum(item.get("role") == "user" for item in history)
            stats["tools"] = sum(item.get("role") == "tool" for item in history)
        return stats

    @staticmethod
    def _codex_limit_windows(result, short=False):
        snapshots = result.get("rateLimitsByLimitId")
        if not snapshots or not isinstance(snapshots, dict):
            snapshots = {"codex": result.get("rateLimits")}
        short = short and sum(isinstance(item, dict) for item in snapshots.values()) == 1
        windows = []
        for ident, snapshot in snapshots.items():
            if not isinstance(snapshot, dict):
                continue
            name = " ".join(str(snapshot.get("limitName") or ident).split())[:70].replace("|", "/")
            for key, label in (("primary", "основное окно"), ("secondary", "второе окно")):
                window = snapshot.get(key)
                if not isinstance(window, dict):
                    continue
                reset = usage_time(window.get("resetsAt"))
                percent = usage_number(window.get("usedPercent"), 100)
                minutes = window.get("windowDurationMins")
                ident_window = f"{ident}:{key}"
                if minutes == 300:
                    label, ident_window = "5 часов", "five_hour" if short else ident_window
                elif minutes == 10080:
                    label, ident_window = "Неделя", "seven_day" if short else ident_window
                if (reset is None or reset > time.time()) and (percent is not None or reset is not None):
                    windows.append({"id": ident_window, "label": label if short else f"{name} · {label}",
                                    "utilization": percent, "resets_at": reset})
        return windows

    def _codex_usage_snapshot(self):
        """Codex account windows for the permanent sidebar (same source as `/usage`)."""
        try:
            windows = self._codex_limit_windows(self.codex["probe_rate_limits"]() or {}, short=True)
        except Exception:
            windows = []
        return {"type": "usage", "backend": "codex", "available": bool(windows), "windows": windows, "status": None,
                "source": "codex_app_server", "updated_at": int(time.time()),
                "message": "Данные аккаунта Codex." if windows else "Codex не передал данные о лимитах аккаунта."}

    def _send_usage(self, backend):
        if backend == "codex":
            lock = self.__dict__.setdefault("_codex_usage_lock", threading.Lock())
            if not lock.acquire(blocking=False):
                return
            def fetch():
                try:
                    self.send(self._codex_usage_snapshot())
                finally:
                    lock.release()
            threading.Thread(target=fetch, daemon=True).start()
        elif backend == "agy":
            self.send(self.agy_usage.snapshot())
            self.agy_usage.refresh_async(self.send)
        else:
            self.send(self.usage.snapshot())
            self.usage.refresh_async(self.send)

    def _agent_limits(self, backend):
        if backend == "claude":
            if time.time() - (self.usage.updated_at or 0) > 25:
                self.usage.fetch_oauth()
            snapshot = self.usage.snapshot()
            self.send(snapshot)
            return snapshot["windows"], snapshot["message"]
        if backend == "codex":
            result = self.codex["probe_rate_limits"]()
            return self._codex_limit_windows(result), "Codex не передал данные о лимитах аккаунта."
        if backend == "agy":
            if time.time() - (self.agy_usage.updated_at or 0) > 25:
                self.agy_usage.fetch_oauth()
            self.send(self.agy_usage.snapshot())
            snapshot = self.agy_usage.snapshot(short=False)
            return snapshot["windows"], snapshot["message"]
        return [], "Лимиты зависят от настроенного API-провайдера."

    def _reset_limits(self, sid, backend, argument):
        if backend == "claude":
            return "Доступный сброс Claude применяется в [Настройки → Использование](https://claude.ai/settings/usage). Он действует и для Claude Code."
        if backend == "agy":
            return "Открой `agy` в терминале и введи `/usage`: там видны квоты групп моделей и время их обновления."
        if backend == "api":
            return "Квоты API управляются в кабинете выбранного провайдера."

        account_link = "[Управление лимитами Codex](https://chatgpt.com/codex/settings/usage)"
        if argument and argument.lower() != "confirm":
            return "Введи `/reset-limits`, чтобы проверить доступность разового сброса."
        caller = getattr(CLIENT, "value", None)
        attempt_key = (sid, caller)
        with self.reset_lock:
            now = time.monotonic()
            self.pending_resets = {key: entry for key, entry in self.pending_resets.items() if entry[1] > now}
            if argument:
                attempt = self.pending_resets.get(attempt_key)
                if attempt is None:
                    return "Сначала проверь доступность через `/reset-limits`."
                result = self.codex["consume_reset_credit"](attempt[0])
                outcome = result.get("outcome")
                if outcome:
                    self.pending_resets.pop(attempt_key, None)
                return {
                    "reset": "Codex подтвердил сброс. Обнови лимиты командой `/usage`.",
                    "alreadyRedeemed": "Этот сброс уже применён. Проверь лимиты через `/usage`.",
                    "nothingToReset": "Codex сообщил, что сейчас ни одно окно не подходит для сброса.",
                    "noCredit": "У этого аккаунта Codex нет доступного разового сброса.",
                }.get(outcome, f"Codex не подтвердил результат. Проверь `/usage` или раздел {account_link}.")

            self.pending_resets.pop(attempt_key, None)
            result = self.codex["probe_rate_limits"]()
            summary = result.get("rateLimitResetCredits")
            if not isinstance(summary, dict):
                return f"Codex не передал сведения о разовых сбросах. Проверь {account_link}."
            count = summary.get("availableCount")
            if count == 0:
                return "На аккаунте Codex нет доступных разовых сбросов. Время обычного обновления показано в `/usage`."
            credits = summary.get("credits")
            if isinstance(credits, list):
                for credit in credits:
                    if not isinstance(credit, dict) or credit.get("status") != "available" or credit.get("resetType") != "codexRateLimits":
                        continue
                    expires = usage_time(credit.get("expiresAt"))
                    ident = credit.get("id")
                    if isinstance(ident, str) and ident and (expires is None or expires > time.time()):
                        self.pending_resets[attempt_key] = (ident, time.monotonic() + 300)
                        until = " Срок действия до " + str(datetime.datetime.fromtimestamp(expires, datetime.timezone.utc).strftime("%d.%m.%Y %H:%M UTC")) + "." if expires is not None else ""
                        return "Доступен разовый сброс Codex." + until + " Чтобы использовать его, введи `/reset-limits confirm`."
            return f"Детали разового сброса доступны в разделе {account_link}."

    def quick_command(self, sid, text, backend="claude", settings=None):
        """Slash commands (`/usage`, `/cost`, `/status`, `/help`, `/model`, `/mode`, `/effort`) answer right away."""
        match = re.fullmatch(r"\s*/(model|mode|effort|usage|limits|quota|reset-limits|reset-usage|cost|tokens|status|info|help|commands)(?:\s+(\S.*?))?\s*", text, re.IGNORECASE)
        if not match:
            return False
        kind = match.group(1).lower()
        value = (match.group(2) or "").strip()
        settings = settings or {}
        active_run = self.runs.get(sid)
        run = active_run if getattr(active_run, "backend", "claude") == backend else None
        agent = BACKEND_LABELS[backend]
        cat = self.catalog if backend == "claude" else self.backend_catalogs.get(backend)
        models = [item for item in ((cat or {}).get("models") or []) if item["value"].lower() not in ("default", "auto")]
        if kind == "model":
            if not value:
                current = settings.get("model") or getattr(run, "model", None)
                if current and str(current).lower() not in ("default", "auto"):
                    reply = f"**{agent}** · текущая модель `{current}`."
                else:
                    reply = f"**{agent}** · выбери модель."
                if models:
                    reply += "\n\n" + ", ".join(f"`{m['value']}`" for m in models)
                else:
                    reply += " Чтобы выбрать модель, введи `/model <название>`."
            else:
                wanted = value.lower()
                found = next((m for m in models if wanted in (m["value"].lower(), str(m.get("displayName", "")).lower())), None) or \
                    next((m for m in models if m["value"].lower().startswith(wanted) or str(m.get("displayName", "")).lower().startswith(wanted)), None)
                if wanted in ("default", "auto"):
                    reply = "Укажи конкретную модель через `/model <название>`."
                elif models and not found and backend != "api":
                    reply = f"Нет модели «{value}». Есть: " + ", ".join(m["value"] for m in models)
                else:
                    chosen = found["value"] if found else value
                    if run and not run.closed:
                        run.control("set_model", model=chosen)
                        if backend == "claude":
                            run.model = chosen
                    self.send({"type": "model_changed", "sid": sid, "model": chosen, "backend": backend})
                    reply = "Модель: " + ((found or {}).get("displayName") or chosen) + (" — со следующего ответа." if run and run.busy else ".")
        elif kind == "mode":
            modes = {"auto": "auto", "авто": "auto", "plan": "plan", "план": "plan", "edits": "acceptEdits", "acceptedits": "acceptEdits", "manual": "manual", "ручной": "manual"}
            mode = modes.get(value.lower())
            if not mode:
                reply = "Режимы: `/mode auto`, `/mode manual`, `/mode edits`, `/mode plan`."
            else:
                if run and not run.closed:
                    run.control("set_permission_mode", mode=mode)
                    if backend == "claude":
                        run.mode = mode
                self.send({"type": "mode_changed", "sid": sid, "mode": mode, "backend": backend})
                reply = "Режим: " + {"auto": "авто", "plan": "план", "acceptEdits": "правки файлов", "manual": "с подтверждением"}[mode] + "."
        elif kind == "effort" and backend == "agy":
            reply = "В Antigravity глубина задаётся вариантом модели. Выбери его через `/model`."
        elif kind == "effort":
            eff_map = {"low": "low", "быстро": "low", "medium": "medium", "средне": "medium", "high": "high", "глубоко": "high", "max": "max", "макс": "max"}
            if backend == "codex":
                eff_map["xhigh"] = "xhigh"
            eff = eff_map.get(value.lower())
            if not eff:
                extra = ", `/effort xhigh`" if backend == "codex" else ""
                reply = f"**{agent}** · `/effort low`, `/effort medium`, `/effort high`, `/effort max`{extra}."
            else:
                previous = validate_effort(settings.get("effort", "default"))
                self.slash_efforts[sid, backend] = (previous, eff)
                self.send({"type": "effort_changed", "sid": sid, "effort": eff, "backend": backend})
                reply = f"**{agent}** · глубина рассуждений `{eff}` со следующего хода."
        elif kind in ("reset-limits", "reset-usage"):
            reply = self._reset_limits(sid, backend, value)
        elif kind in ("usage", "limits", "quota", "cost", "tokens"):
            stats = self._session_token_stats(sid, backend)
            usage_command = kind in ("usage", "limits", "quota")
            title = "Лимиты и расход" if usage_command else "Статистика сессии"
            lines = [f"### {title} · {agent}", ""]
            if usage_command:
                windows, message = self._agent_limits(backend)
                if windows:
                    lines.extend(["| Окно лимита | Использовано | Сброс |", "| :--- | :--- | :--- |"])
                    now = time.time()
                    for window in windows:
                        pct = window.get("utilization")
                        percentage = f"**{pct:.0f}%**" if isinstance(pct, (int, float)) else "—"
                        reset = window.get("resets_at")
                        remaining = max(0, int(reset - now)) if isinstance(reset, (int, float)) else 0
                        reset_label = "—"
                        if remaining >= 3600:
                            reset_label = f"через {remaining // 3600} ч {(remaining % 3600) // 60} мин"
                        elif remaining:
                            reset_label = f"через {max(1, remaining // 60)} мин"
                        lines.append(f"| {window['label']} | {percentage} | {reset_label} |")
                else:
                    lines.append(message)
                lines.append("")
            if any(stats[key] is not None for key in TOKEN_FIELDS):
                tokens = {key: f"`{stats[key]:,}`" if stats[key] is not None else "—" for key in TOKEN_FIELDS}
                lines.extend([
                    f"- **Токены сессии** · вход {tokens['in']} · выход {tokens['out']}",
                    f"- **Кэш** · чтение {tokens['cache_read']} · запись {tokens['cache_write']}",
                ])
            else:
                lines.append("- **Токены сессии** · данных пока нет.")
            lines.append(f"- **Запросы** · {stats['turns']} · **Вызовы инструментов** · {stats['tools']}")
            reply = "\n".join(lines)
        elif kind in ("status", "info"):
            b_status = self.installer.status()
            locked_flag = "защищена паролем 🔒" if self.meta.is_locked(sid) else "открыта"
            active_cnt = len(self.running())
            cur_model = settings.get("model") or getattr(run, "model", None) or "default"
            cur_mode = settings.get("mode") or getattr(run, "mode", None) or "manual"
            cli_parts = [f"`{k}` ({v['version']})" for k, v in b_status.items() if v.get("installed") and v.get("version")]
            reply = (
                f"### Статус Relay · `{os.uname().nodename}`\n"
                f"- **Выбранный агент**: {agent} · **Модель**: `{cur_model}` · **Режим**: `{cur_mode}`\n"
                f"- **Сессия**: `{sid[:8]}…` ({locked_flag}) · **Активных потоков на сервере**: `{active_cnt}`\n"
                f"- **Установленные CLI**: {', '.join(cli_parts) if cli_parts else 'нет'}"
            )
        else:
            reply = (
                "### Быстрые команды Relay\n"
                f"- `/usage` (`/limits`) — лимиты и расход для {agent}\n"
                "- `/reset-limits` — доступность разового сброса\n"
                "- `/cost` (`/tokens`) — токены и инструменты выбранного агента в этой сессии\n"
                "- `/status` — состояние сервера, установленных CLI и текущей сессии\n"
                "- `/clear` (`/new`) — начать новую чистую сессию в этой папке\n"
                "- `/compact` — сжать длинный контекст разговора (Claude Code)\n"
                "- `/backend claude|codex|agy|api` — переключить агента\n"
                "- `/model <имя>` · `/mode auto|manual|edits|plan` · `/effort low|medium|high|max`\n"
                "- `/lock` — управление паролем сессии · `/folder` — папки · `/radar` — радар агентов · `/theme` — 40 тем"
            )
        if not (active_run and active_run.busy) and sid not in getattr(self, "peer_running", {}):
            self.send({"type": "claude", "sid": sid, "data": {"type": "assistant", "relay_backend": backend, "message": {"content": [{"type": "text", "text": reply}]}}})
            self.send({"type": "claude", "sid": sid, "data": {"type": "result", "relay_backend": backend, "is_error": False, "result": reply}})
        else:
            plain_lines = []
            for line in reply.splitlines():
                if line.startswith("| :---") or line.startswith("| Окно лимита"):
                    continue
                if line.startswith("|"):
                    line = " · ".join(cell.strip() for cell in line.strip("|").split("|"))
                plain_lines.append(line.removeprefix("### ").replace("**", "").replace("`", ""))
            self.send({"type": "notice", "sid": sid, "message": "\n".join(plain_lines)})
        return True

    def run_state(self, run):
        backend = getattr(run, "backend", "claude")
        if backend == "codex" and not run.busy and getattr(getattr(self, "host_ref", None), "retiring", False):
            with self.run_lock:
                if self.runs.get(run.sid) is run and not run.busy:
                    run.close()
                    self.runs.pop(run.sid, None)
        self.send({"type": "run_state", "sid": run.sid, "busy": bool(run.busy), "queued": max(0, run.turns - 1),
                   "background": list(getattr(run, "background", [])), "backend": backend})
        if backend == "agy" and not run.busy:
            self.agy_usage.refresh_async(self.send)

    def running(self):
        own = {sid for sid, run in list(self.runs.items()) if run.busy and not run.closed}
        return own | set(getattr(self, "peer_running", {}))

    def reap_idle_runs(self):
        """Drop only idle processes; their transcripts continue with the same --resume ID."""
        now = time.monotonic()
        with self.run_lock:
            for sid, run in list(self.runs.items()):
                # Closing a cluster leader revokes its workers even between answers.
                if not run.cluster_context and not run.busy and not run.background and not run.pending and now - run.last_activity >= IDLE_CLAUDE_SECONDS:
                    run.close()
                    self.runs.pop(sid, None)
                    self._sync_materialized_vault(sid, keep_file=False)

    def _raw_sessions_list(self):
        try:
            self._quarantine_legacy_locked()
        except Exception:
            pass
        sessions = list(self.transcripts.sessions())
        excluded = {s["id"].lower() for s in sessions}
        for linked in self.transcripts.linked_ids.values():
            excluded.update(linked)
        if self._scan_codex_native():
            try:
                codex_rows = self.codex["native_sessions"](excluded)
                sessions.extend(codex_rows)
                excluded.update(r["id"].lower() for r in codex_rows)
            except Exception:
                pass
        if self._scan_agy_native():
            try:
                sessions.extend(self.agy["native_sessions"](excluded))
            except Exception:
                pass
        sessions.sort(key=lambda r: r["modified"], reverse=True)
        return sessions[:2000]

    def _format_sessions_for(self, raw_sessions, unlocked_ids, unlocked_keys=None):
        try:
            titles = self._unlocked_titles_for(unlocked_keys)
            sessions, folders = self.meta.apply(raw_sessions, unlocked_ids=unlocked_ids, unlocked_titles=titles)
        except OSError:
            sessions = [dict(r) for r in raw_sessions]
            folders = []
        busy = self.running()
        for s in sessions:
            s["running"] = s.get("id") in busy
        return {"type": "sessions", "sessions": sessions, "folders": folders}

    def send_sessions(self, only_caller=False):
        raw = self._raw_sessions_list()
        caller = getattr(CLIENT, "value", None)
        if only_caller:
            if caller is not None and hasattr(caller, "send"):
                caller.send(self._format_sessions_for(raw, caller.unlocked, getattr(caller, "unlocked_keys", None)))
            else:
                self.send(self._format_sessions_for(raw, self._local_unlocked, self._local_unlocked_keys))
            return
        if self.host_ref is not None:
            with self.host_ref.lock:
                clients = [c for c in self.host_ref.clients if c.alive]
            for client in clients:
                client.send(self._format_sessions_for(raw, client.unlocked, getattr(client, "unlocked_keys", None)))
            return
        self.send(self._format_sessions_for(raw, self._local_unlocked, self._local_unlocked_keys))

    def hello(self):
        version = self.claude_version()
        backends = self.installer.status()
        self.send({"type": "hello", "home": str(Path.home()), "hostname": os.uname().nodename,
                   "version": version, "protocol": 4, "api_available": True, "auth_available": True,
                   "remote_browser": True, "backends": backends, "host": bool(globals().get("RELAY_HOSTED"))})
        if self.catalog:
            self.send(self.catalog)
        elif self.binary and self.last_backend == "claude" and not globals().get('API_DEFAULT'):
            self.probe_catalog()
        for b in ("codex", "agy"):
            if self.backend_catalogs.get(b):
                self.send(self.backend_catalogs[b])
            elif b == "codex" and self._scan_codex_native():
                cached = self.codex["models_from_cache"]()
                if cached:
                    self.learn_backend_catalog("codex", cached, backends["codex"].get("version", ""))
        if self.last_backend in ("codex", "agy") and backends[self.last_backend]["installed"]:
            self.probe_backend_catalog(self.last_backend)

    def dispatch(self, req):
        op = req.get("op")
        claimed = False
        try:
            if op == "auth_start":
                owner=getattr(CLIENT,'value',None)
                provider = req.get('provider')
                if provider and provider != 'claude':
                    self.auth.start(req.get('id'), owner.send if owner is not None else self.send, owner, provider=provider)
                else:
                    self.auth.start(req.get('id'), owner.send if owner is not None else self.send, owner)
            elif op == 'browser_open':
                owner=getattr(CLIENT,'value',None)
                self.auth.start(req.get('id'),owner.send if owner is not None else self.send,owner,standalone=True,url=req.get('url','https://claude.ai/'))
            elif op == "auth_input":
                self.auth.input(req.get('id'),req.get('code'),getattr(CLIENT,'value',None))
            elif op == "auth_cancel":
                self.auth.cancel(req.get('id'),getattr(CLIENT,'value',None))
            elif op == 'auth_browser_input':
                self.auth.browser(req.get('id'),req.get('input'),getattr(CLIENT,'value',None))
            elif op == 'auth_browser_ack':
                self.auth.browser(req.get('id'),req.get('frame'),getattr(CLIENT,'value',None),ack=True)
            elif op == 'browser_info':
                self.send({'type':'browser_info','id':req.get('id'),'info':self.browser['browser_info']()})
            elif op == 'browser_setup':
                self.browser['install_browser'](self.send,req.get('id'))
            elif op == 'browser_probe':
                owner=getattr(CLIENT,'value',None)
                self.browser['probe_browser'](owner.send if owner is not None else self.send,req.get('id'))
            elif op == "sessions":
                self.send_sessions()
            elif op == "session_meta":
                sid = valid_id(req.get("sid"))
                self._require_unlocked(sid)
                req_title = req.get("title")
                if req_title is not None and self.meta.is_locked(sid) and self.meta.has_vault(sid):
                    keys = self._find_unlocked_keys(sid)
                    if keys:
                        payload = self.meta.open_vault(sid, keys[0], keys[1])
                        if payload is not None:
                            payload["custom_title"] = " ".join(str(req_title).split())[:120]
                            self.meta.seal_vault(sid, keys[0], keys[1], payload)
                            req_title = None
                self.meta.session(sid, title=req_title, pinned=req.get("pinned"),
                                  folder=req.get("folder"), actor=req.get("actor") or "")
                self.send_sessions()
            elif op == "session_folder":
                target_sid = valid_id(req.get("sid")) if req.get("sid") else None
                if target_sid:
                    self._require_unlocked(target_sid)
                created = self.meta.folder(req.get("id"), name=req.get("name"), delete=bool(req.get("delete")),
                                           order=req.get("order"), icon=req.get("icon"))
                if target_sid and created:
                    self.meta.session(target_sid, folder=created, actor=req.get("actor") or "")
                self.send_sessions()
            elif op == "session_lock":
                sid = valid_id(req.get("sid"))
                remove = bool(req.get("remove"))
                low = sid.lower()
                with self.run_lock:
                    idle_run = self.runs.get(sid)
                    if idle_run and not idle_run.busy:
                        idle_run.close()
                        self.runs.pop(sid, None)
                    prev_keys, new_keys = self.meta.set_lock(
                        sid,
                        password=req.get("password"),
                        current_password=req.get("current_password"),
                        remove=remove,
                        actor=req.get("actor") or "",
                    )
                    if remove:
                        if prev_keys:
                            self._restore_session_from_vault(sid, prev_keys, keep_vault=False)
                        else:
                            self.meta.clear_vault(sid)
                        self._active_vault_keys.pop(low, None)
                    elif new_keys:
                        self._seal_session_to_vault(sid, new_keys, prev_keys=prev_keys)
                caller = getattr(CLIENT, "value", None)
                if self.host_ref is not None:
                    with self.host_ref.lock:
                        other_clients = [c for c in self.host_ref.clients if c.alive and c is not caller]
                    for other in other_clients:
                        other.unlocked.discard(low)
                        getattr(other, "unlocked_keys", {}).pop(low, None)
                        other.send({"type": "session_lock_changed", "sid": sid, "locked": not remove, "unlocked": remove})
                unlocked_set = self._caller_unlocked()
                caller_keys = self._caller_keys()
                if remove:
                    unlocked_set.discard(low)
                    caller_keys.pop(low, None)
                else:
                    unlocked_set.add(low)
                    if new_keys:
                        caller_keys[low] = new_keys
                self.send({"type": "session_lock_changed", "sid": sid, "locked": not remove, "unlocked": True})
                self.send_sessions()
            elif op == "session_unlock":
                sid = valid_id(req.get("sid"))
                low = sid.lower()
                keys = self.meta.unlock_keys(sid, req.get("password"))
                if keys is not None or not self.meta.is_locked(sid):
                    if keys is not None:
                        # Ensure any legacy quarantine is upgraded to encrypted vault right away
                        try:
                            self.meta.open_vault(sid, keys[0], keys[1])
                        except ValueError:
                            self.send({"type": "session_unlock_result", "sid": sid, "ok": False, "message": "Неверный пароль сессии."})
                            return
                        self._caller_keys()[low] = keys
                    self._caller_unlocked().add(low)
                    self.send({"type": "session_unlock_result", "sid": sid, "ok": True})
                    self.send_sessions(only_caller=True)
                else:
                    self.send({"type": "session_unlock_result", "sid": sid, "ok": False, "message": "Неверный пароль сессии."})
            elif op == "session_relock":
                sid = valid_id(req.get("sid"))
                low = sid.lower()
                with self.run_lock:
                    idle_run = self.runs.get(sid)
                    if idle_run and not idle_run.busy:
                        idle_run.close()
                        self.runs.pop(sid, None)
                    self._sync_materialized_vault(sid, keep_file=False)
                self._caller_unlocked().discard(low)
                self._caller_keys().pop(low, None)
                locked_now = self.meta.is_locked(sid)
                self.send({"type": "session_lock_changed", "sid": sid, "locked": locked_now, "unlocked": not locked_now})
                self.send_sessions(only_caller=True)
            elif op == "agent_install":
                self.installer.install(req.get("agent"), self.send, action=req.get("action") or "install")
            elif op == "backends_status":
                self.send({"type": "backends_status", "backends": self.installer.status()})
            elif op == "delete_session":
                sid = valid_id(req.get('sid'))
                self._require_unlocked(sid)
                with self.run_lock:
                    run = self.runs.get(sid)
                    if run and run.busy:
                        raise ValueError('Сначала останови ответ в этой сессии.')
                    if run:
                        run.close()
                        self.runs.pop(sid, None)
                    linked = set(self.transcripts.linked_ids.get(sid, set()))
                    found = self.transcripts.find(sid)
                    if found is not None:
                        try:
                            self.transcripts.metadata(found)
                            linked.update(self.transcripts.linked_ids.get(sid, set()))
                        except Exception:
                            pass
                    self.transcripts.delete(sid)
                    for target_id in {sid, *linked}:
                        if self._scan_codex_native() and self.codex["has_native_session"](target_id):
                            self.codex["delete_native_session"](target_id)
                        if self._scan_agy_native() and self.agy["has_native_session"](target_id):
                            self.agy["delete_native_session"](target_id)
                try:
                    self.meta.forget(sid)
                except OSError:
                    pass
                for backend in BACKEND_LABELS:
                    self.slash_efforts.pop((sid, backend), None)
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
                self._send_usage(str(req.get("backend") or "claude"))
            elif op == "api_models":
                try:
                    self.send({'type':'api_models','request':req.get('request'),'base_url':self.api['configuration']({**(req.get('api') or {}),'model':'catalog'})['base_url'],'models':self.api['models'](req.get('api'))})
                except Exception as error:
                    self.send({'type':'api_models','request':req.get('request'),'models':[],'error':self.api['clean_error'](error,(req.get('api') or {}).get('key',''))})
            elif op == "history":
                sid = valid_id(req.get("sid"))
                if self.is_locked_for_caller(sid):
                    self.send({"type": "session_locked", "sid": sid, "message": "Сессия защищена паролем. Введи пароль, чтобы открыть историю."})
                    return
                messages = None
                if self.transcripts.find(sid) is not None:
                    messages = self.transcripts.history(sid)
                elif self.meta.is_locked(sid) and self.meta.has_vault(sid):
                    keys = self._find_unlocked_keys(sid)
                    if keys is not None:
                        payload = self.meta.open_vault(sid, keys[0], keys[1])
                        if payload is not None:
                            if payload.get("kind") == "codex" and isinstance(payload.get("messages"), list):
                                messages = list(payload["messages"])
                            elif payload.get("kind") == "agy":
                                # Parse AGY JSONL lines in memory
                                agy_rows = []
                                for line in str(payload.get("jsonl") or "").splitlines():
                                    try:
                                        r = json.loads(line)
                                        if isinstance(r, dict):
                                            agy_rows.append(r)
                                    except ValueError:
                                        pass
                                messages = []
                                pending_tids = collections.deque()
                                for r in agy_rows:
                                    k_type = r.get("type")
                                    s_idx = r.get("step_index", len(messages))
                                    if k_type == "USER_INPUT":
                                        pending_tids.clear()
                                        t_txt = self.agy["_strip_tags"](r.get("content", ""))
                                        if t_txt:
                                            messages.append({"role": "user", "text": t_txt, "uuid": f"agy-u-{s_idx}", "backend": "agy"})
                                    elif k_type == "PLANNER_RESPONSE":
                                        pending_tids.clear()
                                        t_txt = str(r.get("content") or "").strip()
                                        if t_txt:
                                            messages.append({"role": "assistant", "text": t_txt, "uuid": f"agy-a-{s_idx}", "backend": "agy"})
                                        for idx, tc in enumerate(r.get("tool_calls") or []):
                                            if isinstance(tc, dict):
                                                tid = f"agy-t-{s_idx}-{idx}"
                                                pending_tids.append(tid)
                                                messages.append({"role": "tool", "tool": str(tc.get("name") or "Действие"), "input": tc.get("args") if isinstance(tc.get("args"), dict) else {}, "tool_id": tid, "backend": "agy"})
                                    elif k_type == "GENERIC" and pending_tids:
                                        tid = pending_tids.popleft()
                                        messages.append({"role": "tool_result", "text": str(r.get("content") or "")[:24000], "failed": r.get("status") == "ERROR", "tool_id": tid, "backend": "agy"})
                                messages = history_window(messages)
                            else:
                                parsed_rows = []
                                for line in str(payload.get("jsonl") or "").splitlines():
                                    try:
                                        r = json.loads(line)
                                        if isinstance(r, dict):
                                            parsed_rows.append(r)
                                    except ValueError:
                                        pass
                                messages = self.transcripts.history_from_rows(parsed_rows)
                if messages is None:
                    if self._scan_codex_native() and self.codex["has_native_session"](sid):
                        messages = self.codex["native_history"](sid)
                    elif self._scan_agy_native() and self.agy["has_native_session"](sid):
                        messages = self.agy["native_history"](sid)
                    else:
                        messages = self.transcripts.history(sid)
                messages = self.meta.enrich_turns(sid, messages)
                self.send({"type": "history", "sid": sid, "messages": messages})
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
                self._require_unlocked(sid)
                backend=req.get('backend','claude')
                if backend not in ('claude','codex','agy','api'):raise ValueError('Неизвестный источник модели.')
                text = req.get("text")
                images = req.get("images", [])
                files = req.get("files", [])
                if not isinstance(text, str) or (not text.strip() and not images and not files) or len(text.encode("utf-8")) > 1024 * 1024:
                    raise ValueError("Сообщение пустое или слишком длинное.")
                if not images and not files and self.quick_command(sid, text, backend=backend, settings=req):
                    return
                api=self.api['configuration'](req.get('api')) if backend=='api' else None
                if backend=='claude' and not self.binary:raise ValueError('На сервере нет Claude Code. Выбери другой агент или установи Claude Code.')
                if backend=='codex' and not self.codex['codex_binary']():raise ValueError('На сервере не установлен Codex CLI (`codex`). Установи его или выбери другой агент.')
                if backend=='agy' and not self.agy['agy_binary']():raise ValueError('На сервере не установлен Antigravity CLI (`agy`). Установи его или выбери другой агент.')
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
                effort = "default" if backend == "agy" else validate_effort(req.get("effort", "default"))
                original_effort = effort
                pending_effort = self.slash_efforts.get((sid, backend))
                if pending_effort and original_effort == pending_effort[0]:
                    effort = pending_effort[1]
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
                    if run and run.busy and not run.closed and getattr(run,'backend','claude')!=backend:
                        raise ValueError('Дождись завершения ответа перед переключением между агентами.')
                    steer = bool(run and run.busy and run.turns > 0 and not run.closed and run.process.poll() is None and backend in ('claude', 'codex'))
                    if run and run.busy and not run.closed and run.process.poll() is None:
                        if pending_effort and effort != run.effort:
                            raise ValueError("Дождись завершения ответа, чтобы применить новую глубину рассуждений.")
                        # Mid-answer: the message joins the queue/steers; model and mode switch live.
                        if model != run.model:
                            run.control("set_model", model=None if model == "default" else model); run.model = model
                        if mode != run.mode:
                            run.control("set_permission_mode", mode=mode); run.mode = mode
                    elif run and (
                        run.closed
                        or run.process.poll() is not None
                        or getattr(run,'backend','claude')!=backend
                        or (backend=='api' and not run.same_api(api))
                        or (backend=='claude' and (run.auth_failed or run.auth_stamp != credential_stamp()))
                        or (backend=='codex' and (run.auth_failed or run.auth_stamp != self.codex['credential_stamp']()))
                        or (backend=='agy' and (run.auth_failed or run.auth_stamp != self.agy['credential_stamp']()))
                        or (run.cwd, run.model, run.mode, run.effort, getattr(run,"desktop_enabled",False), getattr(run,"desktop_context",""), getattr(run,"agent_role",""), getattr(run,"custom_prompt",""), getattr(run,"cluster_context",{})) != (cwd, model, mode, effort, desktop_enabled, self.desktop_context, agent_role, custom_prompt, cluster_context)
                    ):
                        run.close()
                        self.runs.pop(sid, None)
                        run = None
                    if run is None:
                        if self.meta.is_locked(sid) and self.meta.has_vault(sid) and self.transcripts.find(sid) is None:
                            keys = self._find_unlocked_keys(sid)
                            if keys is not None:
                                self._active_vault_keys[sid.lower()] = keys
                                self._restore_session_from_vault(sid, keys, keep_vault=True)
                        if len(self.runs) >= 24:
                            idle = next((key for key, value in self.runs.items() if not value.busy), None)
                            if idle:
                                self.runs.pop(idle).close()
                            else:
                                raise ValueError("Уже работают 24 разговора. Дождись завершения одного из них.")
                        existing = self.transcripts.find(sid)
                        if existing and self.transcripts.metadata(existing)["cwd"] != cwd:
                            raise ValueError("Открой исходную рабочую папку этой сессии.")
                        native_codex = not existing and self._scan_codex_native() and self.codex["has_native_session"](sid)
                        native_agy = not existing and not native_codex and self._scan_agy_native() and self.agy["has_native_session"](sid)
                        if native_codex:
                            ncwd = self.codex["native_cwd"](sid)
                            if ncwd and Path(ncwd).is_dir() and ncwd != cwd:
                                raise ValueError("Открой исходную рабочую папку этой сессии.")
                        elif native_agy:
                            ncwd = self.agy["native_cwd"](sid)
                            if ncwd and Path(ncwd).is_dir() and ncwd != cwd:
                                raise ValueError("Открой исходную рабочую папку этой сессии.")
                        native_backend = 'codex' if native_codex else 'agy' if native_agy else None
                        if native_backend and backend != native_backend:
                            adapter = self.codex if native_codex else self.agy
                            existing = self.transcripts.import_native(sid, cwd, native_backend, adapter['native_history'](sid))
                        resume_flag = bool(existing or native_backend)
                        if backend == 'api':
                            run = self.api['ApiRun'](self, sid, cwd, model, mode, resume_flag, effort, api)
                        elif backend == 'codex':
                            run = self.codex['CodexRun'](self, sid, cwd, model, mode, resume_flag, effort)
                        elif backend == 'agy':
                            run = self.agy['AgyRun'](self, sid, cwd, model, mode, resume_flag, effort)
                        else:
                            run = Run(self, sid, cwd, model, mode, bool(existing), effort)
                        self.runs[sid] = run
                    self.last_backend=backend
                    run.actor = self.actor
                    claimed = not steer and not run.busy
                    run.last_activity = time.monotonic()
                self.meta.record_turn(
                    sid,
                    actor=self.actor,
                    backend=backend,
                    model=model,
                    mode=mode,
                    effort=effort,
                    steer=steer,
                    agent_name=cluster_context.get("name", ""),
                    text_head=visible_text(text) if isinstance(text, str) else "",
                )
                if decoded or uploaded:
                    saved = store_files(sid, decoded) if decoded else []
                    saved.extend(uploaded)
                    text += "\n\n<relay-files>\n" + json.dumps(saved, ensure_ascii=False) + "\n</relay-files>"
                run.prompt(text, images, steer=steer)
                if pending_effort and original_effort != pending_effort[0]:
                    if self.slash_efforts.get((sid, backend)) == pending_effort:
                        self.slash_efforts.pop((sid, backend), None)
            elif op == "cluster_reply":
                if self.cluster:self.cluster.result(valid_id(req.get("id")),valid_id(req.get("sid")),valid_id(req.get("run")),req.get("result"))
            elif op == "permission":
                sid = valid_id(req.get("sid"))
                self._require_unlocked(sid)
                run = self.runs.get(sid)
                if run:
                    run.permit(str(req.get("id", "")), bool(req.get("allow")), req.get("answers"))
            elif op == "control":
                sid = valid_id(req.get("sid"))
                self._require_unlocked(sid)
                run = self.runs.get(sid)
                target = req.get("backend")
                if target is not None and getattr(run, "backend", "claude") != target:
                    return
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
                target_backend = req.get("backend") or "claude"
                if target_backend == "claude":
                    if self.catalog:
                        self.send(self.catalog)
                    else:
                        self.probe_catalog()
                elif target_backend in ("codex", "agy"):
                    if self.backend_catalogs.get(target_backend):
                        self.send(self.backend_catalogs[target_backend])
                    else:
                        self.probe_backend_catalog(target_backend)
            elif op == "stop":
                sid = valid_id(req.get("sid"))
                self._require_unlocked(sid)
                run = self.runs.get(sid)
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
            if op in ('auth_start','auth_input','auth_cancel','auth_browser_input','auth_browser_ack','browser_open'):
                self.send({'type':'auth','id':req.get('id'),'state':'input_error' if op in ('auth_input','auth_browser_input') else 'failed','message':str(error)[:500]})
                return
            sid = req.get("sid", "")
            run = self.runs.get(sid)
            if op == "prompt" and run and claimed and run.turns == 0:
                run.busy = False
                self.run_state(run)
            self.send({"type": "error", "sid": sid, "op": op, "message": str(error)[:1800]})

    def close(self):
        self.auth.cancel(closing=True)
        for sid, run in list(self.runs.items()):
            run.close()
            try:
                self._sync_materialized_vault(sid, keep_file=False)
            except Exception:
                pass
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
BROADCAST = {"claude", "turn_summary", "permission", "permission_cancelled", "run_closed", "run_state", "sessions", "session_deleted", "catalog", "claude_update", "agent_install", "backends_status"}
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
        self.unlocked = set()
        self.unlocked_keys = {}
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
            buffered = bytearray()
            while True:
                try:
                    chunk = self.conn.recv(64 * 1024)
                except socket.timeout:
                    if not self.running and time.time() > settle:
                        break
                    continue
                if not chunk:
                    break
                buffered.extend(chunk)
                frames = buffered.split(b"\n")
                buffered = frames.pop()
                if len(buffered) > MAX_REQUEST:
                    break
                for line in frames:
                    if len(line) > MAX_REQUEST:
                        continue
                    try:
                        event = json.loads(line)
                    except ValueError:
                        continue
                    if not isinstance(event, dict):
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
                        self.running.discard(sid)
                        self.pending.pop(sid, None)
                    self.host.peers_changed()
                    if kind in ("claude", "turn_summary", "permission", "permission_cancelled", "run_closed", "run_state", "notice") or (kind == "error" and sid):
                        self.host.route(event)
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
        self.bridge.host_ref = self
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
        sid = event.get("sid")
        locked_stream = (
            kind in ("claude", "turn_summary", "permission", "permission_cancelled")
            and isinstance(sid, str)
            and self.bridge.meta.is_locked(sid)
        )
        sid_low = sid.lower() if isinstance(sid, str) else ""
        for client in clients:
            if locked_stream and not getattr(client, "peer", False) and sid_low not in client.unlocked:
                continue
            client.send(event)

    def replay(self, client):
        """A returning app learns what is still running and what is waiting for an answer."""
        for peer in list(self.peers):
            for sid in list(peer.running):
                client.send({"type": "run_state", "sid": sid, "busy": True, "queued": 0})
                if getattr(client, "peer", False) or not self.bridge.is_locked_for_caller(sid, client):
                    for event in list(peer.pending.get(sid, {}).values()):
                        client.send(event)
        for run in list(self.bridge.runs.values()):
            if run.busy and not run.closed:
                client.send({"type": "run_state", "sid": run.sid, "busy": True, "queued": max(0, run.turns - 1),
                             "background": list(run.background), "backend": getattr(run, "backend", "claude")})
                if getattr(client, "peer", False) or not self.bridge.is_locked_for_caller(run.sid, client):
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
                    with self.bridge.run_lock:
                        for sid, run in list(self.bridge.runs.items()):
                            if getattr(run, "backend", "claude") == "codex" and not run.busy:
                                run.close()
                                self.bridge.runs.pop(sid, None)
                    continue
                peer = self.peer_for(req.get("sid")) if req.get("op") in ("permission", "stop", "control", "prompt") else None
                if peer is not None and req.get("op") == "prompt" and not (req.get("files") or req.get("images")):
                    # Local statistics belong to the new bridge even when the turn is leased to a peer.
                    command = str(req.get("text") or "").strip().lower()
                    if command in ("/usage", "/limits", "/quota", "/cost", "/tokens", "/status", "/info", "/help", "/commands") or re.fullmatch(r"/reset-(?:limits|usage)(?:\s+confirm)?", command):
                        threading.Thread(target=self.dispatch, args=(client, req), daemon=True).start()
                        continue
                if peer is not None:
                    if self.bridge.is_locked_for_caller(req.get("sid"), client):
                        client.send({'type':'error','op':req.get('op'),'sid':req.get('sid'),'message':'Сессия защищена паролем. Сначала разблокируй её.'})
                        continue
                    if req.get('op')=='prompt' and req.get('backend') in ('api','codex','agy'):
                        client.send({'type':'error','op':'prompt','sid':req.get('sid'),'message':'Этот разговор ещё работает в предыдущем процессе хоста. Дождись завершения ответа.'})
                        continue
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
            if getattr(self.bridge,'auth',None):self.bridge.auth.cancel_owner(client)
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
                    self.bridge.reap_idle_runs()
                    if self.idle():
                        break
                    # Claude Code updates itself only between answers, never under a running session.
                    if time.time() >= next_update and not self.bridge.running() and self.bridge.last_backend=='claude' and self.bridge.binary:
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

