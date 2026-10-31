"""Session-scoped MCP adapter. Desktop approval travels over the existing SSH channel."""
import base64
import hashlib
import json
import os
from pathlib import Path
import secrets
import socket
import stat
import sys
import tempfile
import threading
import uuid

TOOLS = [
    {"name":"download_file", "description":"Save a file from this server onto the user's computer. In Relay Auto mode this saves directly; otherwise Relay asks the user for approval and a destination. Use for generated documents, archives, Minecraft mods etc. Never claim delivery before this tool succeeds.", "inputSchema":{"type":"object","properties":{"server_path":{"type":"string"},"destination":{"type":"string","description":"Destination on the user computer: absolute path or relative to its working directory. In Auto mode, omitted destination uses the Downloads folder."}},"required":["server_path"],"additionalProperties":False}},
    {"name":"run_local_command", "description":"Run a command on the user's computer, NOT this server, with the user's local permissions. The initial working directory is the user's home directory; commands can access other locations. In Relay Auto mode commands execute automatically; otherwise the user approves each invocation. The working directory is a starting location, not a filesystem sandbox. Use for local installation or tests requested by the user. Output and exit code are returned. Windows uses PowerShell; Linux uses /bin/sh. No automatic elevation.", "inputSchema":{"type":"object","properties":{"command":{"type":"string"},"timeout_seconds":{"type":"integer","minimum":1,"maximum":120}},"required":["command"],"additionalProperties":False}}
]
MAX_FILE = 512*1024*1024

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
        threading.Thread(target=self.accept,daemon=True).start()

    def config(self,sid):
        self.cancel(sid)
        token=secrets.token_urlsafe(32)
        with self.lock:
            self.tokens={k:v for k,v in self.tokens.items() if v!=sid};self.tokens[token]=sid
        script=Path(self.temp.name)/"mcp.py"
        if not script.exists():
            # Source is supplied by the desktop, never fetched from the network.
            script.write_text(SOURCE,encoding="utf-8");script.chmod(0o600)
        return json.dumps({"mcpServers":{"relay_desktop":{"type":"stdio","command":sys.executable,"args":[str(script)],"env":{"RELAY_DESKTOP_SOCKET":self.path,"RELAY_DESKTOP_TOKEN":token}}}})

    def accept(self):
        while not self.closed:
            try:
                client,_=self.socket.accept()
                threading.Thread(target=self.handle,args=(client,),daemon=True).start()
            except socket.timeout:continue
            except OSError:break

    def handle(self,client):
        try:
            client.settimeout(650)
            with client.makefile("rb") as stream:
                line=stream.readline(65537)
            if len(line)>65536:raise ValueError("Request too large")
            request=json.loads(line)
            with self.lock:sid=self.tokens.get(request.get("token"))
            if not sid:raise ValueError("Desktop connection expired")
            tool=request.get("tool");args=request.get("input")
            if tool not in ("download_file","run_local_command") or not isinstance(args,dict):raise ValueError("Unknown desktop action")
            field="server_path" if tool=="download_file" else "command"
            text=args.get(field)
            if not isinstance(text,str) or not text.strip() or len(text)>16000 or "\x00" in text:raise ValueError("Invalid action")
            if tool=="download_file":
                path=Path(text).expanduser().resolve()
                info=path.stat()
                if not stat.S_ISREG(info.st_mode) or info.st_size>MAX_FILE:raise ValueError("Only regular files up to 512 MiB can be downloaded")
                destination=args.get("destination")
                if destination is not None and (not isinstance(destination,str) or not destination.strip() or len(destination)>4096 or "\x00" in destination):raise ValueError("Invalid destination")
                args={"server_path":str(path),"name":path.name,"size":info.st_size}
                if destination is not None:args["destination"]=destination
            ident=str(uuid.uuid4());item={"sid":sid,"tool":tool,"input":args,"event":threading.Event(),"result":None,"streaming":False}
            with self.lock:
                if self.closed or len(self.pending)>=8:raise ValueError("Desktop is unavailable or busy")
                self.pending[ident]=item
            self.send({"type":"desktop_request","id":ident,"sid":sid,"tool":tool,"input":args})
            try:
                if not item["event"].wait(600):raise ValueError("Desktop approval timed out")
                result=item["result"]
            finally:
                with self.lock:self.pending.pop(ident,None)
                self.send({"type":"desktop_cancel","id":ident,"sid":sid})
            client.sendall((json.dumps(result,ensure_ascii=False)+"\n").encode())
        except Exception as error:
            try:client.sendall((json.dumps({"ok":False,"message":str(error)[:500]})+"\n").encode())
            except OSError:pass
        finally:client.close()

    def result(self,ident,result):
        with self.lock:
            item=self.pending.get(ident)
            if item:
                item["result"]=result;item["event"].set()

    def download(self,ident):
        with self.lock:
            item=self.pending.get(ident)
            if not item or item["tool"]!="download_file" or item["streaming"] or item["event"].is_set():return
            item["streaming"]=True
        try:
            digest=hashlib.sha256();total=0;sequence=0
            descriptor=os.open(item["input"]["server_path"],os.O_RDONLY | os.O_NONBLOCK | getattr(os,"O_NOFOLLOW",0))
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
                    self.send({"type":"desktop_chunk","id":ident,"sequence":sequence,"data":base64.b64encode(body).decode()});sequence+=1
            self.send({"type":"desktop_end","id":ident,"size":total,"sha256":digest.hexdigest()})
        except OSError as error:
            self.send({"type":"desktop_end","id":ident,"error":str(error)[:300]})
        except ValueError as error:
            self.send({"type":"desktop_end","id":ident,"error":str(error)})

    def cancel(self,sid=None):
        with self.lock:
            for item in self.pending.values():
                if sid is None or item["sid"]==sid:
                    item["result"]={"ok":False,"message":"Desktop action cancelled"};item["event"].set()

    def close(self):
        self.closed=True;self.cancel();self.socket.close();self.temp.cleanup()


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
            elif method=="tools/list":result={"tools":TOOLS}
            elif method=="tools/call":
                params=req.get("params",{})
                with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as client:
                    client.settimeout(650);client.connect(os.environ["RELAY_DESKTOP_SOCKET"])
                    client.sendall((json.dumps({"token":os.environ["RELAY_DESKTOP_TOKEN"],"tool":params.get("name"),"input":params.get("arguments",{})})+"\n").encode())
                    with client.makefile("rb") as stream:body=json.loads(stream.readline(262145))
                result={"content":[{"type":"text","text":json.dumps(body,ensure_ascii=False)}],"isError":not body.get("ok",False)}
            else:
                print(json.dumps({"jsonrpc":"2.0","id":ident,"error":{"code":-32601,"message":"Method not found"}}),flush=True);continue
            print(json.dumps({"jsonrpc":"2.0","id":ident,"result":result},ensure_ascii=False),flush=True)
        except Exception as error:
            if ident is not None:print(json.dumps({"jsonrpc":"2.0","id":ident,"error":{"code":-32000,"message":str(error)[:500]}}),flush=True)

if __name__=="__main__":mcp_main()
