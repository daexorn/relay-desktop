"""Shared task board for a team and its AI agents. Python stdlib only.

One JSON file per board under RELAY_BOARD_DIR (default /opt/relay-board), guarded by a lock
file, plus an append-only activity log: who did what, when and from which host.

    python3 board.py mcp            stdio MCP server for Claude sessions started by Relay
    python3 board.py <command> ...  CLI for agents and people in a terminal (see --help)

The Relay bridge imports the same code to serve the board tab.
"""
import fcntl
import json
import os
from pathlib import Path
import socket
import sys
import time

COLUMNS = [
    ("backlog", "Бэклог"),
    ("todo", "К работе"),
    ("doing", "В работе"),
    ("review", "Проверка"),
    ("done", "Готово"),
]
STATUSES = [c[0] for c in COLUMNS]
LINKS = ("blocks", "relates", "duplicates")
FIELDS = ("title", "description", "category", "priority", "assignee", "estimate", "progress", "tags")
MAX_TEXT = 20000


def root():
    return Path(os.environ.get("RELAY_BOARD_DIR", "/opt/relay-board"))


def now():
    return round(time.time(), 3)


class Board:
    def __init__(self, name="sbrp", base=None):
        self.dir = Path(base) if base else root()
        self.name = "".join(c for c in name if c.isalnum() or c in "-_") or "sbrp"
        self.path = self.dir / f"{self.name}.json"
        self.log = self.dir / f"{self.name}.activity.jsonl"

    def _lock(self):
        self.dir.mkdir(parents=True, exist_ok=True)
        handle = open(self.dir / f"{self.name}.lock", "a")
        fcntl.flock(handle, fcntl.LOCK_EX)
        return handle

    def _read(self):
        try:
            data = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            data = {}
        data.setdefault("title", "SB Roleplay")
        data.setdefault("rev", 0)
        data.setdefault("next", 1)
        data.setdefault("categories", [])
        data.setdefault("tasks", [])
        data["columns"] = [{"id": i, "name": n} for i, n in COLUMNS]
        return data

    def _write(self, data):
        temp = self.path.with_suffix(".tmp")
        temp.write_text(json.dumps(data, ensure_ascii=False, indent=1), encoding="utf-8")
        os.chmod(temp, 0o664)
        temp.replace(self.path)

    def state(self, since=None, activity=300):
        data = self._read()
        if since is not None and since == data["rev"]:
            return {"rev": data["rev"], "unchanged": True}
        data["activity"] = self.activity(activity)
        data["progress"] = progress(data)
        return data

    def activity(self, limit=300, task=None):
        try:
            lines = self.log.read_text(encoding="utf-8").splitlines()
        except OSError:
            return []
        rows = []
        for line in reversed(lines):
            try:
                row = json.loads(line)
            except ValueError:
                continue
            if task is None or row.get("task") == task:
                rows.append(row)
            if len(rows) >= limit:
                break
        return rows

    def apply(self, ops, actor, host, via):
        """Applies a list of operations atomically and returns the ids they touched."""
        if isinstance(ops, dict):
            ops = [ops]
        handle = self._lock()
        try:
            data = self._read()
            events, touched = [], []
            for op in ops:
                ident, text = _apply(data, op, actor)
                touched.append(ident)
                events.append({"ts": now(), "actor": actor, "host": host, "via": via,
                               "op": op.get("op"), "task": ident, "text": text})
            data["rev"] += 1
            self._write(data)
            with open(self.log, "a", encoding="utf-8") as log:
                for event in events:
                    log.write(json.dumps(event, ensure_ascii=False) + "\n")
            try:
                os.chmod(self.log, 0o664)
            except OSError:
                pass
            return {"ok": True, "rev": data["rev"], "ids": touched}
        finally:
            handle.close()


def _task(data, ident):
    for task in data["tasks"]:
        if task["id"] == ident:
            return task
    raise ValueError(f"Нет задачи {ident}")


def _clean(field, value):
    if field in ("title", "description", "category", "assignee", "priority"):
        value = "" if value is None else str(value)[:MAX_TEXT]
        if field == "title" and not value.strip():
            raise ValueError("Пустое название")
        if field == "priority" and value not in ("", "low", "normal", "high", "critical"):
            raise ValueError("priority: low, normal, high, critical")
        return value
    if field == "estimate":
        return max(0.0, min(100.0, float(value or 0)))
    if field == "progress":
        return max(0, min(100, int(value or 0)))
    if field == "tags":
        return [str(t)[:40] for t in (value or [])][:20]
    raise ValueError(field)


def _order(data, status):
    orders = [t.get("order", 0) for t in data["tasks"] if t["status"] == status]
    return max(orders, default=0) + 1


def _apply(data, op, actor):
    kind = op.get("op")
    stamp = now()
    if kind == "create":
        ident = f"T-{data['next']}"
        data["next"] += 1
        status = op.get("status", "backlog")
        if status not in STATUSES:
            raise ValueError("status")
        task = {"id": ident, "status": status, "order": _order(data, status), "links": [],
                "comments": [], "created_at": stamp, "created_by": actor, "updated_at": stamp}
        for field in FIELDS:
            task[field] = _clean(field, op.get(field, [] if field == "tags" else 0 if field in ("estimate", "progress") else ""))
        if not task["estimate"]:
            task["estimate"] = 1.0
        if status == "done":
            task.update(done_at=op.get("done_at", stamp), done_by=op.get("done_by", actor), progress=100)
        data["tasks"].append(task)
        return ident, task["title"]
    if kind == "category":
        ident = str(op.get("id") or op.get("name", "")).strip()[:40]
        if not ident:
            raise ValueError("category id")
        existing = next((c for c in data["categories"] if c["id"] == ident), None)
        if op.get("delete"):
            data["categories"] = [c for c in data["categories"] if c["id"] != ident]
            return ident, "категория удалена"
        if existing is None:
            existing = {"id": ident, "name": ident, "color": "#8e8e96", "order": len(data["categories"])}
            data["categories"].append(existing)
        for field in ("name", "color", "description", "order"):
            if field in op:
                existing[field] = op[field]
        return ident, existing["name"]
    task = _task(data, str(op.get("id", "")))
    task["updated_at"] = stamp
    if kind == "update":
        changed = []
        for field in FIELDS:
            if field in op:
                task[field] = _clean(field, op[field])
                changed.append(field)
        return task["id"], "изменено: " + ", ".join(changed)
    if kind in ("move", "claim", "done"):
        status = {"claim": "doing", "done": "done"}.get(kind, op.get("status"))
        if status not in STATUSES:
            raise ValueError("status")
        if kind == "claim":
            task["assignee"] = op.get("assignee") or actor
        before = op.get("before")
        if before:
            target = _task(data, before)
            task["order"] = target.get("order", 0) - 0.5
            peers = sorted((t for t in data["tasks"] if t["status"] == status and t is not task), key=lambda t: t.get("order", 0))
            peers.insert(next((i for i, t in enumerate(peers) if t is target), len(peers)), task)
            for i, t in enumerate(peers):
                t["order"] = i + 1
        elif task["status"] != status:
            task["order"] = _order(data, status)
        previous = task["status"]
        task["status"] = status
        if status == "done" and previous != "done":
            task.update(done_at=stamp, done_by=actor, progress=100)
        elif status != "done":
            task.pop("done_at", None)
            task.pop("done_by", None)
        summary = str(op.get("summary") or "").strip()[:MAX_TEXT]
        if summary:
            task["comments"].append({"ts": stamp, "author": actor, "text": summary})
        name = dict(COLUMNS)[status]
        return task["id"], f"{dict(COLUMNS)[previous]} → {name}" + (f": {summary[:200]}" if summary else "")
    if kind == "comment":
        text = str(op.get("text") or "").strip()[:MAX_TEXT]
        if not text:
            raise ValueError("Пустой комментарий")
        task["comments"].append({"ts": stamp, "author": actor, "text": text})
        return task["id"], text[:200]
    if kind in ("link", "unlink"):
        other = _task(data, str(op.get("to", "")))
        link = op.get("kind", "relates")
        if link not in LINKS or other is task:
            raise ValueError("link kind: blocks, relates, duplicates")
        task["links"] = [l for l in task["links"] if l["to"] != other["id"]]
        if kind == "link":
            task["links"].append({"to": other["id"], "kind": link})
        return task["id"], f"{'связь' if kind == 'link' else 'без связи'} {link} {other['id']}"
    if kind == "delete":
        data["tasks"] = [t for t in data["tasks"] if t is not task]
        for t in data["tasks"]:
            t["links"] = [l for l in t["links"] if l["to"] != task["id"]]
        return task["id"], "удалена: " + task["title"]
    raise ValueError(f"Неизвестная операция {kind}")


def progress(data):
    def summary(tasks):
        total = sum(t.get("estimate") or 1 for t in tasks)
        done = sum(t.get("estimate") or 1 for t in tasks if t["status"] == "done")
        # Unfinished work counts by its own progress, so a half-done epic moves the bar.
        partial = sum((t.get("estimate") or 1) * (t.get("progress") or 0) / 100 for t in tasks if t["status"] != "done")
        return {"tasks": len(tasks), "done": sum(1 for t in tasks if t["status"] == "done"),
                "percent": round(100 * (done + partial) / total, 1) if total else 0.0,
                "points": total, "done_points": done}
    result = {"all": summary(data["tasks"]), "categories": {}}
    for category in data["categories"]:
        result["categories"][category["id"]] = summary([t for t in data["tasks"] if t.get("category") == category["id"]])
    return result


# --- agents -------------------------------------------------------------------------------

def actor_from_env():
    return os.environ.get("RELAY_BOARD_ACTOR") or f"agent@{socket.gethostname()}"


def brief(task):
    who = f" · {task['assignee']}" if task.get("assignee") else ""
    links = "".join(f" · {l['kind']} {l['to']}" for l in task.get("links", []))
    return f"{task['id']} [{task['status']}] [{task.get('category') or '-'}] {task['title']}{who}{links}"


TOOLS = [
    {"name": "board_overview", "description": "Team task board for this project: progress overall and per category, counts per column, and what is in progress right now. Call this before starting work to see the roadmap and pick up a task.", "inputSchema": {"type": "object", "properties": {}}},
    {"name": "board_list", "description": "List board tasks, optionally filtered. Statuses: backlog, todo, doing, review, done.", "inputSchema": {"type": "object", "properties": {"status": {"type": "string"}, "category": {"type": "string"}, "assignee": {"type": "string"}, "query": {"type": "string"}}}},
    {"name": "board_get", "description": "Full task with description, links, comments and its activity history.", "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]}},
    {"name": "board_create", "description": "Add a task. Use for new work found while coding, follow-ups and bugs. estimate is relative size in points (1 small … 8 large).", "inputSchema": {"type": "object", "properties": {"title": {"type": "string"}, "description": {"type": "string"}, "category": {"type": "string"}, "status": {"type": "string"}, "priority": {"type": "string", "enum": ["low", "normal", "high", "critical"]}, "estimate": {"type": "number"}, "assignee": {"type": "string"}}, "required": ["title"]}},
    {"name": "board_update", "description": "Change task fields: title, description, category, priority, assignee, estimate, progress (0-100 for partial work).", "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "title": {"type": "string"}, "description": {"type": "string"}, "category": {"type": "string"}, "priority": {"type": "string"}, "assignee": {"type": "string"}, "estimate": {"type": "number"}, "progress": {"type": "integer"}}, "required": ["id"]}},
    {"name": "board_claim", "description": "Take a task: assigns it to you and moves it to 'doing'. Do this when you start working on it so others do not duplicate the work.", "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]}},
    {"name": "board_move", "description": "Move a task to another column (backlog, todo, doing, review, done) with an optional summary of what changed.", "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "status": {"type": "string"}, "summary": {"type": "string"}}, "required": ["id", "status"]}},
    {"name": "board_done", "description": "Finish a task with a summary of what was done (versions, files, what was verified and how). Use 'review' via board_move instead if a person must check it first.", "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "summary": {"type": "string"}}, "required": ["id", "summary"]}},
    {"name": "board_comment", "description": "Add a note to a task: findings, blockers, questions.", "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "text": {"type": "string"}}, "required": ["id", "text"]}},
    {"name": "board_link", "description": "Link two tasks: kind blocks (id blocks to), relates or duplicates.", "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "to": {"type": "string"}, "kind": {"type": "string", "enum": list(LINKS)}}, "required": ["id", "to"]}},
]


def call(board, name, args, actor, host, via):
    if name == "board_overview":
        data = board.state()
        p = data["progress"]
        lines = [f"{data['title']}: {p['all']['percent']}% · задач {p['all']['done']}/{p['all']['tasks']} готово"]
        for c in data["categories"]:
            s = p["categories"].get(c["id"], {})
            lines.append(f"  {c['id']} ({c['name']}): {s.get('percent', 0)}% · {s.get('done', 0)}/{s.get('tasks', 0)}")
        for status, title in COLUMNS:
            lines.append(f"{title}: {sum(1 for t in data['tasks'] if t['status'] == status)}")
        doing = [brief(t) for t in data["tasks"] if t["status"] in ("doing", "review")]
        if doing:
            lines.append("Сейчас в работе:")
            lines += ["  " + d for d in doing]
        todo = sorted((t for t in data["tasks"] if t["status"] == "todo"), key=lambda t: t.get("order", 0))[:15]
        if todo:
            lines.append("Следующие к работе:")
            lines += ["  " + brief(t) for t in todo]
        return "\n".join(lines)
    if name == "board_list":
        data = board.state()
        query = (args.get("query") or "").lower()
        rows = [t for t in data["tasks"]
                if (not args.get("status") or t["status"] == args["status"])
                and (not args.get("category") or t.get("category") == args["category"])
                and (not args.get("assignee") or args["assignee"].lower() in (t.get("assignee") or "").lower())
                and (not query or query in (t["title"] + " " + t.get("description", "")).lower())]
        rows.sort(key=lambda t: (STATUSES.index(t["status"]), t.get("order", 0)))
        return "\n".join(brief(t) for t in rows[:200]) or "Ничего не найдено"
    if name == "board_get":
        data = board.state()
        task = _task(data, args["id"])
        return json.dumps({**task, "activity": board.activity(50, task["id"])}, ensure_ascii=False, indent=1)
    op = {"board_create": "create", "board_update": "update", "board_claim": "claim", "board_move": "move",
          "board_done": "done", "board_comment": "comment", "board_link": "link"}.get(name)
    if not op:
        raise ValueError(f"Unknown tool {name}")
    result = board.apply([{**args, "op": op}], actor, host, via)
    return f"Готово: {', '.join(result['ids'])}"


def mcp_main():
    board = Board(os.environ.get("RELAY_BOARD_NAME", "sbrp"))
    actor, host = actor_from_env(), socket.gethostname()
    for line in sys.stdin:
        ident = None
        try:
            req = json.loads(line)
            ident, method = req.get("id"), req.get("method")
            if ident is None:
                continue
            if method == "initialize":
                result = {"protocolVersion": req.get("params", {}).get("protocolVersion", "2024-11-05"),
                          "capabilities": {"tools": {}}, "serverInfo": {"name": "Relay board", "version": "1"}}
            elif method == "ping":
                result = {}
            elif method == "tools/list":
                result = {"tools": TOOLS}
            elif method == "tools/call":
                params = req.get("params", {})
                try:
                    text, error = call(board, params.get("name"), params.get("arguments") or {}, actor, host, "agent"), False
                except (ValueError, KeyError, TypeError) as e:
                    text, error = f"Ошибка: {e}", True
                result = {"content": [{"type": "text", "text": text}], "isError": error}
            else:
                print(json.dumps({"jsonrpc": "2.0", "id": ident, "error": {"code": -32601, "message": "Method not found"}}), flush=True)
                continue
            print(json.dumps({"jsonrpc": "2.0", "id": ident, "result": result}, ensure_ascii=False), flush=True)
        except Exception as error:
            if ident is not None:
                print(json.dumps({"jsonrpc": "2.0", "id": ident, "error": {"code": -32000, "message": str(error)[:500]}}), flush=True)


def cli(argv):
    usage = """relay-board <command> [args]
  overview                         progress and what is in progress
  list [status] [--category C] [--query Q]
  get ID
  add "title" [--category C] [--status S] [--estimate N] [--description D]
  claim ID | move ID STATUS [summary] | done ID "summary"
  comment ID "text" | link ID TO [blocks|relates|duplicates]
Env: RELAY_BOARD_ACTOR (your name), RELAY_BOARD_NAME (default sbrp), RELAY_BOARD_DIR."""
    if not argv or argv[0] in ("-h", "--help"):
        print(usage)
        return 0
    flags, rest, i = {}, [], 0
    while i < len(argv):
        if argv[i].startswith("--") and i + 1 < len(argv):
            flags[argv[i][2:]] = argv[i + 1]
            i += 2
        else:
            rest.append(argv[i])
            i += 1
    command, rest = rest[0], rest[1:]
    names = {"overview": "board_overview", "list": "board_list", "get": "board_get", "add": "board_create",
             "claim": "board_claim", "move": "board_move", "done": "board_done", "comment": "board_comment", "link": "board_link"}
    args = dict(flags)
    if command == "list" and rest:
        args["status"] = rest[0]
    elif command == "add":
        args["title"] = rest[0]
    elif command in ("get", "claim"):
        args["id"] = rest[0]
    elif command == "move":
        args.update(id=rest[0], status=rest[1], summary=" ".join(rest[2:]))
    elif command in ("done",):
        args.update(id=rest[0], summary=" ".join(rest[1:]))
    elif command == "comment":
        args.update(id=rest[0], text=" ".join(rest[1:]))
    elif command == "link":
        args.update(id=rest[0], to=rest[1], kind=rest[2] if len(rest) > 2 else "relates")
    if "estimate" in args:
        args["estimate"] = float(args["estimate"])
    board = Board(os.environ.get("RELAY_BOARD_NAME", "sbrp"))
    print(call(board, names[command], args, actor_from_env(), socket.gethostname(), "cli"))
    return 0


if __name__ == "__main__":
    if sys.argv[1:2] == ["mcp"]:
        mcp_main()
    else:
        try:
            sys.exit(cli(sys.argv[1:]))
        except (ValueError, KeyError, IndexError) as error:
            print(f"Ошибка: {error}", file=sys.stderr)
            sys.exit(1)
