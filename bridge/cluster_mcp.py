"""Leader-only MCP tools carried over Relay's existing authenticated SSH channel."""
import json
import os
from pathlib import Path
import secrets
import socket
import sys
import tempfile
import threading
import uuid

TOOLS = [
    {"name":"list_workers","description":"List the workers configured for THIS cluster, their hosts, directories and current availability. Only these workers may be assigned tasks.","inputSchema":{"type":"object","properties":{},"additionalProperties":False}},
    {"name":"dispatch_task","description":"Assign a concrete task to one configured worker. Each worker keeps its own session within this cluster; follow-up tasks resume its context. Include scope, constraints, expected deliverable and checks. Returns a job ID immediately; the worker runs under Relay's concurrency limit. Use depends_on to sequence tasks that edit or review the same files. You remain responsible for the final result.","inputSchema":{"type":"object","properties":{"worker":{"type":"string"},"title":{"type":"string"},"task":{"type":"string"},"depends_on":{"type":"array","items":{"type":"string"}}},"required":["worker","task"],"additionalProperties":False}},
    {"name":"task_status","description":"Get a job's actual state and, when complete, its worker's result. Use only job IDs returned by this cluster.","inputSchema":{"type":"object","properties":{"job":{"type":"string"}},"required":["job"],"additionalProperties":False}},
    {"name":"wait_for_task","description":"Wait up to 20 seconds for a worker's result without stopping it. If unfinished, returns its current state. Repeat or do other useful work. Do not claim completion until the job state is done and you have checked the result.","inputSchema":{"type":"object","properties":{"job":{"type":"string"},"timeout_seconds":{"type":"integer","minimum":1,"maximum":20}},"required":["job"],"additionalProperties":False}},
    {"name":"cancel_task","description":"Cancel one queued or running job in this cluster; returns its actual state.","inputSchema":{"type":"object","properties":{"job":{"type":"string"}},"required":["job"],"additionalProperties":False}},
    {"name":"cluster_status","description":"Get the state of this cluster's tasks and scheduling limits.","inputSchema":{"type":"object","properties":{},"additionalProperties":False}},
]
NAMES={t["name"] for t in TOOLS}
LEADER_INSTRUCTIONS = """You are the lead Claude session of a Relay agent cluster. You direct the configured worker Claude sessions using relay_cluster MCP tools. Start by listing workers. Break the user's request into concrete scoped assignments, specify constraints, expected outputs and verification. Dispatch independent work in parallel; use depends_on for shared files or reviews. Workers operate on their own configured hosts/directories: do not assume files or commits are already synchronized. Have workers report actual files, commits, artifacts and checks. Use task_status or wait_for_task to collect their results. Review findings, request corrections by dispatching follow-up tasks when needed, and synthesize the final answer to the user. Do not ask the user to copy tasks between sessions. Relay enforces scheduling limits and may pause or cancel tasks; respect those states. Never report a queued, interrupted or failed job as successful. If workers are unavailable, explain what is blocked instead of inventing progress. Follow the user's custom cluster instructions and existing project rules."""

class RelayCluster:
    def __init__(self,send):
        self.send=send;self.closed=False;self.lock=threading.RLock();self.tokens={};self.pending={}
        self.temp=tempfile.TemporaryDirectory(prefix="relay-cluster-");os.chmod(self.temp.name,0o700)
        self.path=str(Path(self.temp.name)/"rpc.sock")
        self.socket=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);self.socket.bind(self.path)
        os.chmod(self.path,0o600);self.socket.listen(16);self.socket.settimeout(1)
        threading.Thread(target=self.accept,daemon=True).start()

    def config(self,sid,run):
        self.cancel(sid,revoke=True)
        token=secrets.token_urlsafe(32)
        with self.lock:
            self.tokens={k:v for k,v in self.tokens.items() if v[0]!=sid};self.tokens[token]=(sid,run)
        script=Path(self.temp.name)/"mcp.py"
        if not script.exists():script.write_text(SOURCE,encoding="utf-8");script.chmod(0o600)
        return {"type":"stdio","command":sys.executable,"args":[str(script)],
                "env":{"RELAY_CLUSTER_SOCKET":self.path,"RELAY_CLUSTER_TOKEN":token}}

    def accept(self):
        while not self.closed:
            try:
                client,_=self.socket.accept();threading.Thread(target=self.handle,args=(client,),daemon=True).start()
            except socket.timeout:continue
            except OSError:break

    def handle(self,client):
        ident=None
        try:
            client.settimeout(45)
            with client.makefile("rb") as f:line=f.readline(262145)
            if len(line)>262144:raise ValueError("Cluster request too large")
            request=json.loads(line)
            with self.lock:scope=self.tokens.get(request.get("token"))
            if scope is None:raise ValueError("Cluster connection expired")
            sid,run=scope;tool=request.get("tool");args=request.get("input")
            if tool not in NAMES or not isinstance(args,dict):raise ValueError("Unknown cluster action")
            with self.lock:
                if len(self.pending)>=64:raise ValueError("Too many pending cluster requests")
                ident=str(uuid.uuid4());item={"sid":sid,"run":run,"event":threading.Event(),"result":None};self.pending[ident]=item
            self.send({"type":"cluster_request","id":ident,"sid":sid,"run":run,"tool":tool,"input":args})
            wait=max(1,min(20,int(args.get("timeout_seconds") or 20)))+10 if tool=="wait_for_task" else 30
            if not item["event"].wait(wait):raise ValueError("Relay is not responding; do not repeat dispatch until checking cluster_status")
            result=item["result"]
        except Exception as e:result={"ok":False,"message":str(e)[:500]}
        finally:
            if ident:
                with self.lock:self.pending.pop(ident,None)
        try:client.sendall((json.dumps(result,ensure_ascii=False)+"\n").encode())
        except OSError:pass
        client.close()

    def result(self,ident,sid,run,result):
        with self.lock:
            item=self.pending.get(ident)
            if item and item["sid"]==sid and item["run"]==run:
                item["result"]=result;item["event"].set()

    def cancel(self,sid=None,revoke=False):
        with self.lock:
            for ident,item in self.pending.items():
                if sid is None or item["sid"]==sid:
                    item["result"]={"ok":False,"message":"Cluster tool request cancelled"};item["event"].set()
                    self.send({"type":"cluster_cancelled","id":ident,"sid":item["sid"]})
            if revoke:self.tokens={k:v for k,v in self.tokens.items() if sid is not None and v[0]!=sid}

    def close(self):
        self.closed=True;self.cancel(revoke=True);self.socket.close();self.temp.cleanup()

def mcp_main():
    for line in sys.stdin:
        ident=None
        try:
            if len(line)>262144:raise ValueError("Request too large")
            request=json.loads(line);ident=request.get("id");method=request.get("method")
            if ident is None:continue
            if method=="initialize":result={"protocolVersion":request.get("params",{}).get("protocolVersion","2024-11-05"),"capabilities":{"tools":{}},"serverInfo":{"name":"Relay cluster","version":"1"}}
            elif method=="tools/list":result={"tools":TOOLS}
            elif method=="ping":result={}
            elif method=="tools/call":
                params=request.get("params",{});tool=params.get("name")
                if tool not in NAMES:raise ValueError("Unknown cluster action")
                message={"token":os.environ["RELAY_CLUSTER_TOKEN"],"tool":tool,"input":params.get("arguments",{})}
                with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as sock:
                    sock.settimeout(45);sock.connect(os.environ["RELAY_CLUSTER_SOCKET"])
                    sock.sendall((json.dumps(message,ensure_ascii=False)+"\n").encode())
                    with sock.makefile("rb") as f:body=json.loads(f.readline(1048577))
                result={"content":[{"type":"text","text":json.dumps(body,ensure_ascii=False)}],"isError":body.get("ok") is False}
            else:raise ValueError("Method not found")
            print(json.dumps({"jsonrpc":"2.0","id":ident,"result":result},ensure_ascii=False),flush=True)
        except Exception as e:
            if ident is not None:print(json.dumps({"jsonrpc":"2.0","id":ident,"error":{"code":-32000,"message":str(e)[:500]}}),flush=True)

if __name__=="__main__":mcp_main()
