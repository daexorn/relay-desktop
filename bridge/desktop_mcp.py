"""Session-scoped MCP adapter. Desktop approval travels over the existing SSH channel."""
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import socket
import stat
import sys
import tempfile
import threading
import time
import uuid

TOOLS = [
    {"name":"download_file", "description":"Save a file from this server onto the user's computer. In Relay Auto mode this saves directly; otherwise Relay asks the user for approval and a destination. Use for generated documents, archives, Minecraft mods etc. Never claim delivery before this tool succeeds.", "inputSchema":{"type":"object","properties":{"server_path":{"type":"string"},"destination":{"type":"string","description":"Destination on the user computer: absolute path or relative to its working directory. In Auto mode, omitted destination uses the Downloads folder."}},"required":["server_path"],"additionalProperties":False}},
    {"name":"run_local_command", "description":"Run a command on the user's computer, NOT this server, with the user's local permissions. The initial working directory is the user's home directory; commands can access other locations. In Relay Auto mode commands execute automatically; otherwise the user approves each invocation. The working directory is a starting location, not a filesystem sandbox. Use for local installation or tests requested by the user. Output and exit code are returned. Windows uses PowerShell; Linux uses /bin/sh. No automatic elevation.", "inputSchema":{"type":"object","properties":{"command":{"type":"string"},"timeout_seconds":{"type":"integer","minimum":1,"maximum":120}},"required":["command"],"additionalProperties":False}}
,
    {"name":"minecraft_test", "description":"Run an automated in-game test on the user's computer: Relay starts its own hidden Minecraft (Fabric) copy with the given mods, executes the scenario and returns report.json, steps.log, the game log and PNG screenshots saved on THIS server (read them with your file tools). The user's normal game profile is not touched and the window never takes focus. Depending on the user's Relay setting the test runs immediately or the user is asked first; if they skip it, continue without claiming in-game verification. Scenario: one step per line, '#' comments. Steps: wait <sec>; wait_screen <ScreenClassOrTitle|none> [timeout]; wait_world [timeout]; test_world (create and enter a flat creative world); join <host:port> (offline-mode server); command </cmd>; chat <text>; key <key.inventory|e|escape|...>; click <button text|key:translation.key>; screen (log current screen and its buttons); screenshot <name>; log <text>; quit. Finish with quit. A failing step stops the run with a 'failure' screenshot.", "inputSchema":{"type":"object","properties":{"purpose":{"type":"string","description":"One short sentence shown to the user: what this test checks."},"scenario":{"type":"string"},"mods":{"type":"array","items":{"type":"string"},"description":"Absolute server paths of mod jars to load (the mod under test and its dependencies such as fabric-api)."},"minecraft_version":{"type":"string","description":"Default 1.21.8 (the only version the Relay test bot supports now)."},"loader_version":{"type":"string","description":"Fabric loader; default latest stable."},"timeout_seconds":{"type":"integer","minimum":60,"maximum":900}},"required":["purpose","scenario","mods"],"additionalProperties":False}}
]
# Only a paired phone advertises screenshot return paths for local commands.
PHONE_COMMAND = dict(TOOLS[1], inputSchema={
    **TOOLS[1]["inputSchema"],
    "properties": {**TOOLS[1]["inputSchema"]["properties"], "preview_files": {
        "type":"array","maxItems":4,"items":{"type":"string"},
        "description":"Image files produced by this command, relative to the PC workspace, to show on the phone."
    }}
})
MAX_FILE = 512*1024*1024
MAX_UPLOAD = 64*1024*1024
TEST_ROOT = Path.home()/".cache/relay-desktop/gametest"

class RelayDesktop:
    def __init__(self, send):
        self.send=send
        self.temp=tempfile.TemporaryDirectory(prefix="relay-desktop-")
        os.chmod(self.temp.name,0o700)
        self.path=str(Path(self.temp.name)/"rpc.sock")
        self.socket=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
        self.socket.bind(self.path);os.chmod(self.path,0o600);self.socket.listen(8)
        self.socket.settimeout(1)
        self.closed=False;self.lock=threading.RLock();self.tokens={};self.pending={}
        self.phone_history = {}
        self.can_save_phone_result = lambda sid: False
        self.phone_socket = Path.home() / ".cache/relay-desktop/phone/worker.sock"
        threading.Thread(target=self.accept,daemon=True).start()

    def config(self,sid,tools=("download_file","run_local_command"),phone=False):
        self.cancel(sid,revoke=True)
        token=secrets.token_urlsafe(32)
        with self.lock:
            if phone: tools = tuple(t for t in tools if t in ("minecraft_test", "run_local_command"))
            self.tokens[token] = (sid, tuple(tools), phone)
        script=Path(self.temp.name)/"mcp.py"
        if not script.exists():
            # Source is supplied by the desktop, never fetched from the network.
            script.write_text(SOURCE,encoding="utf-8");script.chmod(0o600)
        return json.dumps({"mcpServers":{"relay_desktop":{"type":"stdio","command":sys.executable,"args":[str(script)],"env":{"RELAY_DESKTOP_SOCKET":self.path,"RELAY_DESKTOP_TOKEN":token,"RELAY_DESKTOP_TOOLS":",".join(tools),"RELAY_PAIRED_PHONE":"1" if phone else "0"}}}})

    def accept(self):
        while not self.closed:
            try:
                client,_=self.socket.accept()
                threading.Thread(target=self.handle,args=(client,),daemon=True).start()
            except socket.timeout:continue
            except OSError:break

    def handle(self,client):
        try:
            client.settimeout(2000)
            with client.makefile("rb") as stream:
                line=stream.readline(65537)
            if len(line)>65536:raise ValueError("Request too large")
            request=json.loads(line)
            with self.lock: grant = self.tokens.get(request.get("token"))
            if not grant:raise ValueError("Desktop connection expired")
            sid, allowed, phone = grant
            tool=request.get("tool");args=request.get("input")
            if tool not in allowed or (phone and tool not in ("minecraft_test", "run_local_command")) or not isinstance(args,dict):raise ValueError("Unknown desktop action")
            field={"download_file":"server_path","run_local_command":"command","minecraft_test":"scenario"}[tool]
            text=args.get(field)
            if not isinstance(text,str) or not text.strip() or len(text)>(65536 if tool=="minecraft_test" else 16000) or "\x00" in text:raise ValueError("Invalid action")
            if phone and tool == "run_local_command":
                paths = args.get("preview_files", [])
                if not isinstance(paths, list) or len(paths)>4 or not all(
                    isinstance(p,str) and 0<len(p)<256 and not Path(p).is_absolute() and ".." not in Path(p).parts
                    and Path(p).suffix.lower() in (".jpg",".jpeg",".png",".webp") for p in paths
                ):
                    raise ValueError("Ожидалось до четырёх путей к изображениям в рабочей папке ПК")
            wait=600
            if tool=="minecraft_test":
                args,wait=self.test_input(args)
            elif tool=="download_file":
                path=Path(text).expanduser().resolve()
                info=path.stat()
                if not stat.S_ISREG(info.st_mode) or info.st_size>MAX_FILE:raise ValueError("Only regular files up to 512 MiB can be downloaded")
                destination=args.get("destination")
                if destination is not None and (not isinstance(destination,str) or not destination.strip() or len(destination)>4096 or "\x00" in destination):raise ValueError("Invalid destination")
                args={"server_path":str(path),"name":path.name,"size":info.st_size}
                if destination is not None:args["destination"]=destination
            ident=str(uuid.uuid4());item={"sid":sid,"tool":tool,"input":args,"event":threading.Event(),"result":None,"streaming":False,"streamed":set(),"uploads":{},"uploaded":0}
            with self.lock:
                if self.closed or self.tokens.get(request.get("token"))!=grant:raise ValueError("Desktop connection expired")
                if self.closed or len(self.pending)>=8:raise ValueError("Desktop is unavailable or busy")
                self.pending[ident]=item
            event = {"type":"desktop_request","id":ident,"sid":sid,"tool":tool,"input":args}
            try:
                if phone:
                    try:
                        self.run_phone(event, item, wait)
                    except (OSError, ValueError) as error:
                        if not item["event"].is_set(): self.result(ident, {"ok":False,"message":str(error)[:500]})
                else:
                    self.send(event)
                if not item["event"].wait(wait):raise ValueError("Desktop approval timed out")
                result=item["result"]
                if (tool=="minecraft_test" or (phone and args.get("preview_files"))) and isinstance(result,dict):
                    folder=TEST_ROOT/ident
                    result["files"]=sorted(str(p) for p in folder.glob("*") if p.is_file() and not p.name.endswith(".part"))
                    result["folder"]=str(folder)
                if phone:
                    self.phone_update(ident, sid, stage="done", payload="", result=result)
                    self.save_phone_result(ident, sid)
            finally:
                with self.lock:self.pending.pop(ident,None)
                if not phone:self.send({"type":"desktop_cancel","id":ident,"sid":sid})
            client.sendall((json.dumps(result,ensure_ascii=False)+"\n").encode())
        except Exception as error:
            try:client.sendall((json.dumps({"ok":False,"message":str(error)[:500]})+"\n").encode())
            except OSError:pass
        finally:client.close()

    def phone_update(self, ident, sid, **fields):
        with self.lock:
            row = self.phone_history.setdefault(ident, {"sid":sid, "id":ident})
            row.update(fields)
            # Active jobs stay visible while the user returns to the conversation.
            if len(self.phone_history) > 100:
                oldest = next((k for k,v in self.phone_history.items() if v.get("stage") == "done"), None)
                if oldest: self.phone_history.pop(oldest)

    def save_phone_result(self, ident, sid):
        if not self.can_save_phone_result(sid): return
        # Result files live with the other test artifacts. Never serialize the
        # challenge, signature or approval payload into the session index.
        folder = TEST_ROOT / "sessions" / str(uuid.UUID(sid))
        folder.mkdir(parents=True, exist_ok=True, mode=0o700)
        with self.lock:
            row = self.phone_history.get(ident)
            if not row: return
            summary = {key:row[key] for key in ("id","sid","stage","tool","result")}
        target = folder / (ident + ".json")
        with tempfile.NamedTemporaryFile("w", dir=folder, delete=False) as temp:
            json.dump(summary, temp, ensure_ascii=False)
        os.replace(temp.name, target)
        entries = sorted(folder.glob("*.json"), key=lambda p:p.name)
        if len(entries)>30:
            for p in sorted(entries, key=lambda f:f.stat().st_mtime)[:-30]: p.unlink(missing_ok=True)

    def phone_state(self, sid):
        with self.lock:
            rows = [dict(row) for row in reversed(list(self.phone_history.values())) if row["sid"] == sid][:10]
        if len(rows)<10 and self.can_save_phone_result(sid):
            folder = TEST_ROOT / "sessions" / str(uuid.UUID(sid))
            known = {row["id"] for row in rows}
            for p in sorted(folder.glob("*.json"), key=lambda f:f.stat().st_mtime, reverse=True):
                if p.stem in known: continue
                try:
                    row = json.loads(p.read_bytes())
                    if row.get("sid") == sid and row.get("stage") == "done": rows.append(row)
                except (ValueError,OSError): continue
                if len(rows)>=10: break
        online = False
        if self.phone_socket.is_socket():
            import fcntl
            try:
                with self.phone_socket.with_name("worker.lock").open() as guard:
                    fcntl.flock(guard, fcntl.LOCK_SH | fcntl.LOCK_NB)
            except BlockingIOError: online = True
            except OSError: pass
        return {"type":"phone_tests", "sid":sid, "online":online, "jobs":rows}

    def phone_decide(self, sid, ident, signature):
        with self.lock:
            item = self.pending.get(ident)
            if not item or item["sid"] != sid or "decision" not in item:
                raise ValueError("Запрос теста устарел")
            if signature and not re.fullmatch(r"[A-Za-z0-9+/]{43}=", signature):
                raise ValueError("Некорректная подпись телефона")
            if not item["decision"].is_set():
                item["signature"] = signature
                item["decision"].set()

    def run_phone(self, event, item, wait):
        ident, sid = event["id"], event["sid"]
        self.phone_update(ident, sid, stage="connecting", tool=event["tool"])
        for mod in event["input"].get("mods", []):
            digest = hashlib.sha256()
            with open(mod["server_path"], "rb") as jar:
                for block in iter(lambda: jar.read(1024*1024), b""):
                    digest.update(block)
            mod["sha256"] = digest.hexdigest()
        with socket.socket(socket.AF_UNIX) as conn:
            conn.settimeout(12)
            try:
                conn.connect(str(self.phone_socket))
            except OSError as error:
                raise ValueError("Фоновый Relay на ПК сейчас недоступен") from error
            with conn.makefile("rb") as stream:
                def read():
                    line = stream.readline(400001)
                    if not line or len(line) > 400000: raise ValueError("Связь с ПК прервана")
                    return json.loads(line)
                def write(frame):
                    conn.sendall((json.dumps(frame, separators=(",",":"))+"\n").encode())
                greeting = read()
                if greeting.get("op") != "phone_challenge":
                    raise ValueError("На ПК уже выполняется другой тест")
                payload = json.dumps(dict(event, nonce=greeting["nonce"]), ensure_ascii=False, separators=(",",":"))
                with self.lock:
                    item["decision"] = threading.Event()
                    self.phone_update(ident, sid, stage="approval", payload=payload)
                if not item["decision"].wait(240) or not item.get("signature") or item["event"].is_set():
                    raise ValueError("Телефон не подтвердил запуск")
                conn.settimeout(min(wait, 1800))
                write({"type":"phone_approved","payload":payload,"signature":item["signature"]})
                self.phone_update(ident, sid, stage="running", payload="")
                with self.lock:
                    item["connection"] = conn
                while not item["event"].is_set():
                    frame = read()
                    op = frame.get("op")
                    if op == "phone_error": raise ValueError(frame.get("message") or "ПК отклонил запуск")
                    if frame.get("id") != ident: continue
                    if op == "desktop_download":
                        self.download(ident, frame.get("index"), write)
                    elif op == "desktop_upload":
                        self.upload(ident, frame.get("name"), frame.get("sequence"), frame.get("data"))
                    elif op == "desktop_upload_end":
                        self.upload_end(ident, frame.get("name"))
                    elif op == "phone_progress":
                        self.phone_update(ident, sid, message=str(frame.get("message") or "")[:512])
                    elif op == "desktop_result":
                        self.result(ident, frame.get("result") or {"ok":False})

    def test_input(self,args):
        mods=args.get("mods")
        if not isinstance(mods,list) or not 1<=len(mods)<=16:raise ValueError("mods: 1-16 jar paths")
        files=[]
        for raw in mods:
            if not isinstance(raw,str) or "\x00" in raw:raise ValueError("Invalid mod path")
            path=Path(raw).expanduser().resolve();info=path.stat()
            if not stat.S_ISREG(info.st_mode) or path.suffix!=".jar" or info.st_size>256*1024*1024:raise ValueError(f"Not a mod jar: {raw}")
            files.append({"server_path":str(path),"name":path.name,"size":info.st_size})
        if len({f["name"] for f in files})!=len(files):raise ValueError("Mod file names must be unique")
        version=str(args.get("minecraft_version") or "1.21.8")
        loader=str(args.get("loader_version") or "")
        if not re.fullmatch(r"[\w.+-]{1,32}",version) or (loader and not re.fullmatch(r"[\w.+-]{1,32}",loader)):raise ValueError("Invalid version")
        timeout=max(60,min(900,int(args.get("timeout_seconds") or 300)))
        purpose=str(args.get("purpose") or "")[:300]
        clean={"purpose":purpose,"scenario":args["scenario"],"mods":files,"minecraft_version":version,"loader_version":loader,"timeout_seconds":timeout}
        # Approval may wait for the user; the game download, transfer and run add their own time.
        return clean,300+timeout+600

    def upload(self,ident,name,sequence,data):
        with self.lock:
            item=self.pending.get(ident)
            if not item or item["event"].is_set():return
            if item["tool"]!="minecraft_test" and name not in {Path(p).name for p in item["input"].get("preview_files", [])}:return
            if not isinstance(name,str) or not re.fullmatch(r"[\w][\w.-]{0,79}",name):return
            state=item["uploads"].setdefault(name,{"sequence":0})
            if sequence!=state["sequence"] or len(item["uploads"])>64:return
            body=base64.b64decode(data or "")
            item["uploaded"]+=len(body)
            if item["uploaded"]>MAX_UPLOAD:return
            folder=TEST_ROOT/ident;folder.mkdir(parents=True,exist_ok=True,mode=0o700)
            with open(folder/(name+".part"),"ab" if sequence else "wb") as f:f.write(body)
            state["sequence"]+=1

    def upload_end(self,ident,name):
        with self.lock:
            item=self.pending.get(ident)
            if not item or name not in item["uploads"]:return
            part=TEST_ROOT/ident/(name+".part")
            if part.exists():part.replace(TEST_ROOT/ident/name)

    def result(self,ident,result):
        with self.lock:
            item=self.pending.get(ident)
            if item:
                item["result"]=result;item["event"].set()

    def download(self,ident,index=None,send=None):
        send = send or self.send
        with self.lock:
            item=self.pending.get(ident)
            if not item or item["event"].is_set():return
            if item["tool"]=="minecraft_test":
                if not isinstance(index,int) or not 0<=index<len(item["input"]["mods"]) or index in item["streamed"]:return
                item["streamed"].add(index);source_path=item["input"]["mods"][index]["server_path"]
            elif item["tool"]=="download_file" and not item["streaming"]:
                item["streaming"]=True;source_path=item["input"]["server_path"]
            else:return
        try:
            digest=hashlib.sha256();total=0;sequence=0
            descriptor=os.open(source_path,os.O_RDONLY | os.O_NONBLOCK | getattr(os,"O_NOFOLLOW",0))
            with os.fdopen(descriptor,"rb") as source:
                info=os.fstat(source.fileno())
                if not stat.S_ISREG(info.st_mode) or info.st_size>MAX_FILE:raise ValueError("File is not regular or is too large")
                while True:
                    if item["event"].is_set() or self.closed:return
                    body=source.read(128*1024)
                    if not body:break
                    total+=len(body)
                    if total>MAX_FILE:raise ValueError("File grew beyond 512 MiB")
                    digest.update(body)
                    send({"type":"desktop_chunk","id":ident,"sequence":sequence,"data":base64.b64encode(body).decode()});sequence+=1
            send({"type":"desktop_end","id":ident,"size":total,"sha256":digest.hexdigest()})
        except OSError as error:
            send({"type":"desktop_end","id":ident,"error":str(error)[:300]})
        except ValueError as error:
            send({"type":"desktop_end","id":ident,"error":str(error)})

    def cancel_phone(self, sid, ident):
        with self.lock:
            item = self.pending.get(ident)
            if not item or item["sid"] != sid: return
            self.result(ident, {"ok": False, "message": "Остановлено с телефона"})
            if item.get("decision"): item["decision"].set()
            if item.get("connection"):
                try: item["connection"].shutdown(socket.SHUT_RDWR)
                except OSError: pass

    def cancel(self,sid=None,revoke=False):
        with self.lock:
            for item in self.pending.values():
                if sid is None or item["sid"]==sid:
                    item["result"]={"ok":False,"message":"Desktop action cancelled"};item["event"].set()
                    if item.get("decision"): item["decision"].set()
                    if item.get("connection"):
                        try: item["connection"].shutdown(socket.SHUT_RDWR)
                        except OSError: pass
            if revoke:self.tokens={k:v for k,v in self.tokens.items() if sid is not None and v[0]!=sid}

    def close(self):
        self.closed=True;self.cancel(revoke=True);self.socket.close();self.temp.cleanup()


def mcp_main():
    for line in sys.stdin:
        ident=None
        try:
            if len(line)>65536:raise ValueError("Request too large")
            req=json.loads(line);ident=req.get("id");method=req.get("method")
            if ident is None:continue
            if method=="initialize":
                result={"protocolVersion":req.get("params",{}).get("protocolVersion","2024-11-05"),"capabilities":{"tools":{}},"serverInfo":{"name":"Relay desktop","version":"0.4.1"}}
            elif method=="ping":result={}
            elif method=="tools/list":
                allowed=os.environ.get("RELAY_DESKTOP_TOOLS","download_file,run_local_command").split(",")
                result={"tools":[PHONE_COMMAND if t["name"]=="run_local_command" and os.environ.get("RELAY_PAIRED_PHONE")=="1" else t for t in TOOLS if t["name"] in allowed]}
            elif method=="tools/call":
                params=req.get("params",{})
                with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as client:
                    client.settimeout(2000);client.connect(os.environ["RELAY_DESKTOP_SOCKET"])
                    client.sendall((json.dumps({"token":os.environ["RELAY_DESKTOP_TOKEN"],"tool":params.get("name"),"input":params.get("arguments",{})})+"\n").encode())
                    with client.makefile("rb") as stream:body=json.loads(stream.readline(262145))
                # A failed test is a result to read, not a tool error.
                result={"content":[{"type":"text","text":json.dumps(body,ensure_ascii=False)}],"isError":not body.get("ok",False) and "report" not in body}
            else:
                print(json.dumps({"jsonrpc":"2.0","id":ident,"error":{"code":-32601,"message":"Method not found"}}),flush=True);continue
            print(json.dumps({"jsonrpc":"2.0","id":ident,"result":result},ensure_ascii=False),flush=True)
        except Exception as error:
            if ident is not None:print(json.dumps({"jsonrpc":"2.0","id":ident,"error":{"code":-32000,"message":str(error)[:500]}}),flush=True)

if __name__=="__main__":mcp_main()
