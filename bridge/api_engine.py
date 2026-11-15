"""Relay's direct OpenAI-compatible agent. No Claude Code or third-party Python packages."""
import collections,glob,json,mimetypes,os,re,select,signal,socket,stat,subprocess,tempfile,threading,time,urllib.error,urllib.parse,urllib.request,uuid
from pathlib import Path
from datetime import datetime,timezone

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self,*args,**kwargs):return None

def configuration(raw):
    if not isinstance(raw,dict):raise ValueError('Настрой API этого сервера.')
    base=str(raw.get('base_url') or '').strip().rstrip('/')
    if base.endswith('/chat/completions'):base=base[:-17].rstrip('/')
    p=urllib.parse.urlsplit(base)
    if p.scheme not in ('http','https') or not p.netloc or p.username or p.password or p.query or p.fragment:
        raise ValueError('Укажи базовый URL API, например https://tooken.club/v1.')
    key=raw.get('key','')
    if not isinstance(key,str) or any(c in key for c in '\r\n\0'):raise ValueError('Некорректный API-ключ.')
    model=str(raw.get('model') or '').strip()
    if not model or len(model)>200 or any(ord(c)<32 for c in model):raise ValueError('Укажи модель API.')
    return {'base_url':base,'key':key,'model':model,'max_tokens':max(256,min(65536,int(raw.get('max_tokens') or 8192))),
            'max_steps':max(1,min(256,int(raw.get('max_steps') or 80)))}

def clean_error(error,key):
    text=str(error)
    if key:text=text.replace(key,'[ключ скрыт]')
    text=re.sub(r'(?:tc_live_|sk-ant-|sk-)[A-Za-z0-9_-]{16,}','[ключ скрыт]',text)
    return text[:1400]

def request(config,path,body=None):
    headers={'Accept':'application/json','User-Agent':'Relay-desktop','Content-Type':'application/json'}
    if config['key']:headers['Authorization']='Bearer '+config['key']
    req=urllib.request.Request(config['base_url']+path,headers=headers,data=json.dumps(body,ensure_ascii=False).encode() if body is not None else None)
    try:return urllib.request.build_opener(NoRedirect).open(req,timeout=120)
    except urllib.error.HTTPError as e:
        data=e.read(8192).decode(errors='replace')
        e.close()
        try:
            detail=json.loads(data).get('error',{});data=detail.get('message') or detail.get('code') or 'Запрос отклонён.' if isinstance(detail,dict) else str(detail)
        except (ValueError,AttributeError):data='Запрос отклонён.'
        raise ValueError(clean_error(f'API · HTTP {e.code}: {data}',config['key'])) from None
    except (OSError,urllib.error.URLError) as e:raise ValueError(clean_error('API недоступен: '+str(e),config['key'])) from None

def models(raw):
    config=configuration({**(raw or {}),'model':(raw or {}).get('model') or 'catalog'})
    with request(config,'/models') as response:data=json.load(response)
    return [{'value':m['id'],'displayName':m.get('name') or m['id'],'description':str(m.get('description') or '')[:200]} for m in data.get('data',[]) if isinstance(m,dict) and isinstance(m.get('id'),str)][:500]

def schema(name,description,properties,required=()):
    return {'type':'function','function':{'name':name,'description':description,'parameters':{'type':'object','properties':properties,'required':list(required)}}}

TEXT={'type':'string'}
TOOLS=[
 schema('Bash','Execute a shell command on this SSH server. Commands run in the session directory, with this SSH account permissions.',{'command':TEXT,'timeout':{'type':'integer','description':'Milliseconds, at most 600000'}},['command']),
 schema('Read','Read a server file (text or image), or list a directory. file_path can be absolute or relative to the session directory.',{'file_path':TEXT,'offset':{'type':'integer'},'limit':{'type':'integer'}},['file_path']),
 schema('Write','Create or replace a UTF-8 text file on this server.',{'file_path':TEXT,'content':TEXT},['file_path','content']),
 schema('Edit','Replace an exact substring in a server text file. Fails for ambiguous matches unless replace_all is true.',{'file_path':TEXT,'old_string':TEXT,'new_string':TEXT,'replace_all':{'type':'boolean'}},['file_path','old_string','new_string']),
 schema('Glob','Find server files by a glob pattern, e.g. **/*.rs. Returns at most 300 paths.',{'pattern':TEXT,'path':TEXT},['pattern']),
]

class Cancelled(Exception):pass
class ToolOutput(list):
    def __init__(self,content,failed=False):super().__init__(content);self.failed=failed

class MCP:
    """Small stdio MCP client for Relay's existing board/PC/cluster adapters."""
    def __init__(self,name,config,cancel):
        self.name,self.cancel,self.serial=name,cancel,0
        self.buffer=bytearray()
        self.process=subprocess.Popen([config['command'],*config.get('args',[])],env={**os.environ,**config.get('env',{})},stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,start_new_session=True)
        try:
            self.rpc('initialize',{'protocolVersion':'2024-11-05','capabilities':{},'clientInfo':{'name':'relay-api','version':'1'}})
            self.process.stdin.write((json.dumps({'jsonrpc':'2.0','method':'notifications/initialized'})+'\n').encode());self.process.stdin.flush()
            self.tools=self.rpc('tools/list',{}).get('tools',[])
        except Exception:
            self.close();raise
    def rpc(self,method,params):
        self.serial+=1;ident=self.serial
        self.process.stdin.write((json.dumps({'jsonrpc':'2.0','id':ident,'method':method,'params':params},ensure_ascii=False)+'\n').encode());self.process.stdin.flush()
        end=time.monotonic()+(15 if method in ('initialize','tools/list') else 2000)
        while time.monotonic()<end:
            if self.cancel.is_set():raise Cancelled()
            if b'\n' not in self.buffer:
                if not select.select([self.process.stdout],[],[],.2)[0]:continue
                raw=os.read(self.process.stdout.fileno(),65536)
                if not raw:raise ValueError('Инструмент Relay завершил работу.')
                self.buffer.extend(raw)
                if len(self.buffer)>4*1024*1024:raise ValueError('Ответ инструмента Relay слишком большой.')
                if b'\n' not in self.buffer:continue
            end_line=self.buffer.index(10);line=bytes(self.buffer[:end_line]);del self.buffer[:end_line+1]
            try:reply=json.loads(line)
            except ValueError:continue
            if reply.get('id')!=ident:continue
            if reply.get('error'):raise ValueError(str(reply['error'].get('message') or 'Ошибка инструмента Relay.'))
            return reply.get('result') or {}
        raise ValueError('Инструмент Relay не ответил вовремя.')
    def close(self):
        try:self.process.stdin.close();self.process.wait(timeout=1)
        except (OSError,subprocess.TimeoutExpired):
            self.process.terminate()
            try:self.process.wait(timeout=1)
            except subprocess.TimeoutExpired:self.process.kill();self.process.wait()
        self.process.stdout.close()

class ApiProcess:
    """Lifecycle interface shared with native runs; never starts an operating-system process."""
    def __init__(self,run):self.run=run
    def poll(self):return 0 if self.run.closed else None

class ApiRun:
    backend='api'
    def __init__(self,owner,sid,cwd,model,mode,resume,effort='default',api=None):
        self.owner,self.sid,self.cwd=owner,sid,cwd
        self.config=configuration(api);self.model=model if model and model!='default' else self.config['model']
        self.mode,self.effort=mode,effort
        self.agent_role=getattr(owner,'agent_role','');self.custom_prompt=getattr(owner,'custom_prompt','')
        self.cluster_context=getattr(owner,'cluster_context',{})
        self.desktop_enabled=getattr(owner,'desktop_requested',False);self.desktop_context=getattr(owner,'desktop_context','')
        self.busy=False;self.closed=False;self.auth_failed=False;self.auth_stamp=None;self.last_activity=time.monotonic()
        self.turns=0;self.background=[];self.pending={};self.inbox=collections.deque();self.condition=threading.Condition()
        self.cancel=threading.Event();self.process=ApiProcess(self);self.child=None;self.response=None;self.parent=None;self.partial=''
        self.mcp=[];self.mcp_tools={}
        existing=owner.transcripts.find(sid)
        self.path=existing or owner.transcripts.root/re.sub(r'[^a-zA-Z0-9-]','-',cwd)/(sid+'.jsonl')
        self.path.parent.mkdir(parents=True,exist_ok=True)
        self.worker=threading.Thread(target=self.work,daemon=True);self.worker.start()

    def same_api(self,raw):return self.config==configuration(raw)

    def chain(self):
        if not self.path.exists():return []
        nodes=collections.OrderedDict();parents={}
        # Same branch traversal as the desktop history, retaining original image/tool blocks.
        for row in self.owner.api_records(self.path):
            if row.get('isSidechain') or not row.get('uuid'):continue
            parents[row['uuid']]=row.get('parentUuid')
            if row.get('type') in ('user','assistant') and isinstance(row.get('message'),dict):nodes[row['uuid']]=row
        result=[];seen=set();key=next(reversed(nodes),None)
        self.parent=key
        while key and key not in seen and key in parents:
            seen.add(key)
            if key in nodes:result.append(nodes[key])
            key=parents[key]
        return list(reversed(result)) or list(nodes.values())

    def append(self,role,content,partial=False):
        ident=str(uuid.uuid4())
        message={'role':role,'content':content}
        if role=='assistant':message.update(id='msg_api_'+uuid.uuid4().hex,type='message',model=self.model,stop_reason='end_turn',stop_sequence=None,usage={'input_tokens':0,'output_tokens':0})
        row={'type':role,'uuid':ident,'parentUuid':self.parent,'sessionId':self.sid,'cwd':self.cwd,'timestamp':datetime.now(timezone.utc).isoformat().replace('+00:00','Z'),'isSidechain':False,'userType':'external','relayBackend':'api','message':message}
        if partial:row['relayPartial']=True
        with self.path.open('a',encoding='utf-8') as f:
            import fcntl as lock_module
            lock_module.flock(f,lock_module.LOCK_EX)
            f.write(json.dumps(row,ensure_ascii=False)+'\n');f.flush();os.fsync(f.fileno())
        self.parent=ident

    def context(self):
        result=[];deferred_images=[];pending_calls=set()
        def flush_images():
            if deferred_images:result.append({'role':'user','content':list(deferred_images)});deferred_images.clear()
        for row in self.chain():
            if row.get('isMeta'):continue
            message=row['message'];role=message.get('role',row['type']);blocks=message.get('content','')
            if isinstance(blocks,str):blocks=[{'type':'text','text':blocks}]
            if not isinstance(blocks,list):continue
            text=[];pictures=[];calls=[];tool_results=[]
            for b in blocks:
                if not isinstance(b,dict):continue
                if b.get('type')=='text':text.append(str(b.get('text') or ''))
                elif b.get('type')=='image':
                    source=b.get('source') or {}
                    if source.get('type')=='base64' and source.get('data'):pictures.append({'type':'image_url','image_url':{'url':'data:'+source.get('media_type','image/png')+';base64,'+source['data']}})
                elif b.get('type')=='tool_use':calls.append({'id':str(b.get('id') or ''),'type':'function','function':{'name':str(b.get('name') or ''),'arguments':json.dumps(b.get('input') or {},ensure_ascii=False)}})
                elif b.get('type')=='tool_result':tool_results.append(b)
            for b in tool_results:
                content=b.get('content','');parts=content if isinstance(content,list) else [{'type':'text','text':str(content)}]
                output=[]
                for part in parts:
                    if part.get('type')=='text':output.append(str(part.get('text') or ''))
                    elif part.get('type')=='image':
                        source=part.get('source') or {}
                        if source.get('data'):deferred_images.append({'type':'image_url','image_url':{'url':'data:'+source.get('media_type','image/png')+';base64,'+source['data']}})
                ident=str(b.get('tool_use_id') or '')
                if ident in pending_calls:
                    result.append({'role':'tool','tool_call_id':ident,'content':'\n'.join(output)});pending_calls.discard(ident)
                else:result.append({'role':'user','content':'Earlier tool result: '+'\n'.join(output)})
            if text or pictures or calls:
                # Resolve orphaned calls (e.g. an interrupted native session) before moving on.
                for ident in sorted(pending_calls):result.append({'role':'tool','tool_call_id':ident,'content':'The previous tool was interrupted; check actual state before retrying.'})
                pending_calls.clear();flush_images()
                entry={'role':role if role in ('user','assistant') else 'user','content':'\n'.join(text)}
                if pictures:entry['content']=([{'type':'text','text':entry['content']}] if entry['content'] else [])+pictures
                if calls and entry['role']=='assistant':entry['tool_calls']=calls;pending_calls.update(c['id'] for c in calls)
                result.append(entry)
        for ident in sorted(pending_calls):result.append({'role':'tool','tool_call_id':ident,'content':'Tool was interrupted; verify state before continuing.'})
        flush_images();return result

    def emit(self,data):self.owner.send({'type':'claude','sid':self.sid,'data':data})
    def prompt(self,text,images=None,steer=False):
        with self.condition:
            if self.closed:raise ValueError('API-сессия закрыта. Отправь сообщение ещё раз.')
            if len(self.inbox)>=12:raise ValueError('В очереди API уже 12 сообщений.')
            self.inbox.append((text,images or []));self.turns+=1;self.busy=True;self.last_activity=time.monotonic();self.condition.notify()
        self.owner.run_state(self)

    def work(self):
        while True:
            with self.condition:
                while not self.closed and not self.inbox:self.condition.wait()
                if self.closed:self.config['key']='';return
                text,images=self.inbox.popleft();self.cancel.clear()
            self.partial='';error=None;usage={};self.usage={'input_tokens':0,'output_tokens':0}
            try:
                self.chain()
                self.append('user',([{'type':'text','text':text}] if text else [])+[{'type':'image','source':image} for image in images])
                self.emit({'type':'system','subtype':'init','session_id':self.sid,'model':self.model,'relay_backend':'api'})
                usage=self.answer()
            except Exception as e:
                error='Остановлено. Уже выполненные действия не повторялись.' if isinstance(e,Cancelled) else clean_error(e,self.config['key'])
                if self.partial:
                    try:self.append('assistant',[{'type':'text','text':self.partial}],partial=True)
                    except OSError:error+=' Частичный ответ не удалось записать на диск.'
                    self.partial=''
            finally:
                for client in self.mcp:
                    try:client.close()
                    except (OSError,ValueError):pass
                self.mcp=[];self.mcp_tools={}
            with self.condition:
                self.turns=max(0,self.turns-1)
                if error:self.inbox.clear();self.turns=0
                self.busy=self.turns>0;self.pending.clear();self.last_activity=time.monotonic()
            usage=self.usage
            self.emit({'type':'result','subtype':'error_during_execution' if error else 'success','is_error':bool(error),'result':error or '', 'errors':[error] if error else [],'usage':usage,'session_id':self.sid,'relay_backend':'api'})
            self.owner.run_state(self)

    def answer(self):
        system=f"You are Relay's development agent on the user's SSH server ({os.uname().sysname}), in {self.cwd}. Use the provided tools directly. Read project AGENTS.md/instructions before changes and preserve concurrent work. Commands and file paths refer to this server. Respect the selected permission mode. Verify real files and checks, never claim execution or delivery without a successful tool result. Supply downloadable server artifacts as [name](/absolute/server/path). Do not expose credentials. Continue the supplied conversation, including work previously performed by Claude Code."
        if self.custom_prompt:system+='\n'+self.custom_prompt
        tools=list(TOOLS)
        servers={}
        if getattr(self.owner,'board_file',None) and self.cluster_context.get('role')!='worker':
            servers['relay_board']={'command':os.sys.executable,'args':[str(self.owner.board_file),'mcp'],'env':{'RELAY_BOARD_NAME':'sbrp','RELAY_BOARD_ACTOR':'Relay API'}}
            system+=' The shared relay_board is the team task board; check relevant tasks before substantial work and record actual changes/checks.'
        if self.cluster_context.get('role')=='leader':
            servers['relay_cluster']=self.owner.cluster.config(self.sid,self.cluster_context['run'])
            system+=' '+self.owner.cluster_instructions
        elif self.cluster_context.get('role')=='worker':system+=' You are an assigned cluster worker. Follow the leader instructions, preserve other workers changes and return results to the leader.'
        if self.desktop_enabled and self.owner.desktop:
            try:pc=json.loads(self.desktop_context or '{}')
            except ValueError:pc={}
            if pc.get('local'):
                servers.update(json.loads(self.owner.desktop.config(self.sid,('download_file','run_local_command')))['mcpServers'])
                system+=' relay_desktop tools operate on the connected computer, not this server. Report delivery only after download_file succeeds. Local permission rules are enforced by Relay. Connected computer context: '+self.desktop_context
        for name,config in servers.items():
            client=MCP(name,config,self.cancel);self.mcp.append(client)
            for tool in client.tools:
                qualified='mcp__'+name+'__'+tool['name'];self.mcp_tools[qualified]=(client,tool['name'])
                tools.append({'type':'function','function':{'name':qualified,'description':str(tool.get('description') or '')[:3000],'parameters':tool.get('inputSchema') or {'type':'object','properties':{}}}})
        total=self.usage
        for step in range(self.config['max_steps']):
            if self.cancel.is_set() or self.closed:raise Cancelled()
            body={'model':self.model,'messages':[{'role':'system','content':system}]+self.context(),'tools':tools,'stream':True,'stream_options':{'include_usage':True},'max_tokens':self.config['max_tokens']}
            if self.effort!='default':body['reasoning_effort']=self.effort
            content,calls,usage=self.complete(body,step)
            total['input_tokens']+=int(usage.get('prompt_tokens') or 0);total['output_tokens']+=int(usage.get('completion_tokens') or 0)
            blocks=([{'type':'text','text':content}] if content else [])
            parsed=[]
            for call in calls:
                try:args=json.loads(call['function']['arguments'] or '{}')
                except ValueError:raise ValueError('API вернул некорректные аргументы инструмента.')
                if not isinstance(args,dict):raise ValueError('API вернул некорректные аргументы инструмента.')
                ident=call.get('id') or 'tool_api_'+uuid.uuid4().hex
                parsed.append((ident,call['function']['name'],args));blocks.append({'type':'tool_use','id':ident,'name':call['function']['name'],'input':args})
            if blocks:
                self.append('assistant',blocks);self.partial='';self.emit({'type':'assistant','message':{'role':'assistant','content':blocks}})
            if not parsed:return total
            for position,(ident,name,args) in enumerate(parsed):
                failed=False
                try:
                    self.approve(ident,name,args)
                    result=self.tool(name,args)
                    failed=getattr(result,'failed',False)
                except Cancelled:
                    for remaining,_,_ in parsed[position:]:
                        block={'type':'tool_result','tool_use_id':remaining,'content':'Остановлено пользователем; проверь состояние перед повтором.','is_error':True}
                        self.append('user',[block]);self.emit({'type':'user','message':{'role':'user','content':[block]}})
                    raise
                except Exception as e:failed=True;result=[{'type':'text','text':clean_error(e,self.config['key'])}]
                block={'type':'tool_result','tool_use_id':ident,'content':result,'is_error':failed}
                self.append('user',[block]);self.emit({'type':'user','message':{'role':'user','content':[block]}})
        raise ValueError(f"Достигнут лимит {self.config['max_steps']} шагов API. История сохранена; можно продолжить следующим сообщением.")

    def complete(self,body,index):
        self.response=request(self.config,'/chat/completions',body)
        content='';calls={};usage={};finished=False;started=False
        try:
            if 'text/event-stream' not in self.response.headers.get('Content-Type',''):
                data=json.loads(self.response.read(16*1024*1024));choice=(data.get('choices') or [{}])[0];message=choice.get('message') or {}
                if data.get('error'):raise ValueError(clean_error(data['error'],self.config['key']))
                if not message.get('content') and not message.get('tool_calls'):raise ValueError('API вернул пустой ответ.')
                return str(message.get('content') or ''),message.get('tool_calls') or [],data.get('usage') or {}
            for raw in self.response:
                if self.cancel.is_set() or self.closed:raise Cancelled()
                if len(raw)>2*1024*1024:raise ValueError('Слишком большой фрагмент ответа API.')
                line=raw.decode('utf-8',errors='replace').strip()
                if not line.startswith('data:'):continue
                line=line[5:].strip()
                if line=='[DONE]':finished=True;break
                data=json.loads(line)
                if data.get('error'):raise ValueError(clean_error(data['error'],self.config['key']))
                usage=data.get('usage') or usage
                for choice in data.get('choices') or []:
                    delta=choice.get('delta') or {};text=delta.get('content') or ''
                    if isinstance(text,str) and text:
                        if not started:
                            self.emit({'type':'stream_event','event':{'type':'content_block_start','index':index,'content_block':{'type':'text','text':''}}});started=True
                        content+=text;self.partial=content
                        if len(content)>4*1024*1024:raise ValueError('Ответ API превышает 4 МБ.')
                        self.emit({'type':'stream_event','event':{'type':'content_block_delta','index':index,'delta':{'type':'text_delta','text':text}}})
                    for call in delta.get('tool_calls') or []:
                        item=calls.setdefault(call.get('index',0),{'id':'','type':'function','function':{'name':'','arguments':''}})
                        if call.get('id'):item['id']+=call['id']
                        fn=call.get('function') or {}
                        item['function']['name']+=fn.get('name') or '';item['function']['arguments']+=fn.get('arguments') or ''
                        if len(calls)>64 or sum(len(x['function']['arguments']) for x in calls.values())>2*1024*1024:raise ValueError('API вернул слишком большой набор инструментов.')
                    if choice.get('finish_reason') is not None:
                        if choice['finish_reason']=='length':raise ValueError('API достиг лимита длины ответа. Увеличь лимит токенов в настройках API и продолжи сессию.')
                        finished=True
            if not finished:raise ValueError('Поток API оборвался. Частичный ответ сохранён; запрос автоматически не повторялся.')
            if not content and not calls:raise ValueError('API вернул пустой ответ.')
            return content,[calls[k] for k in sorted(calls)],usage
        finally:
            self.response.close();self.response=None

    def approve(self,ident,name,args):
        if self.cancel.is_set() or self.closed:raise Cancelled()
        if name.startswith(('mcp__relay_desktop__','mcp__relay_board__')) and name in self.mcp_tools:return
        if name.startswith('mcp__relay_cluster__') and name in self.mcp_tools and self.mode=='auto':return
        if name not in ('Bash','Read','Write','Edit','Glob') and name not in self.mcp_tools:raise ValueError('Неизвестный инструмент: '+name)
        read=name in ('Read','Glob')
        if self.mode=='plan' and not read:raise ValueError('В режиме плана изменения и команды отключены. Переключи режим, чтобы выполнить действие.')
        if read or self.mode=='auto' or (self.mode=='acceptEdits' and name in ('Write','Edit')):return
        item={'tool_name':name,'input':args,'event':threading.Event(),'allow':False}
        self.pending[ident]=item;self.owner.send({'type':'permission','sid':self.sid,'id':ident,'tool':name,'input':args})
        try:
            end=time.monotonic()+1200
            while not item['event'].wait(.2):
                if self.cancel.is_set() or self.closed:raise Cancelled()
                if time.monotonic()>end:raise ValueError('Время ожидания подтверждения истекло.')
            if not item['allow']:raise ValueError('Пользователь отклонил действие.')
        finally:self.pending.pop(ident,None);self.owner.send({'type':'permission_cancelled','sid':self.sid,'id':ident})

    def permit(self,ident,allow,answers):
        item=self.pending.get(ident)
        if item:item['allow']=bool(allow);item['event'].set()
    def control(self,subtype,**fields):
        if subtype=='set_model':self.model=fields.get('model') or self.config['model']
        elif subtype=='set_permission_mode':self.mode=fields['mode']

    def tool(self,name,args):
        if self.cancel.is_set():raise Cancelled()
        if name in self.mcp_tools:
            client,tool=self.mcp_tools[name];result=client.rpc('tools/call',{'name':tool,'arguments':args})
            return ToolOutput(result.get('content') or [{'type':'text','text':'Выполнено.'}],bool(result.get('isError')))
        def path(raw):
            if not isinstance(raw,str) or not raw or '\0' in raw:raise ValueError('Укажи путь файла.')
            p=Path(raw).expanduser();return (p if p.is_absolute() else Path(self.cwd)/p).resolve()
        if name=='Bash':
            code,text=self.bash(args);return ToolOutput([{'type':'text','text':text}],code!=0)
        if name=='Read':
            p=path(args.get('file_path'))
            if p.is_dir():return [{'type':'text','text':'\n'.join(x.name+('/' if x.is_dir() else '') for x in sorted(p.iterdir())[:500])}]
            mime=mimetypes.guess_type(p.name)[0]
            if mime in ('image/png','image/jpeg','image/webp','image/gif') and p.stat().st_size<=4*1024*1024:
                import base64
                return [{'type':'image','source':{'type':'base64','media_type':mime,'data':base64.b64encode(p.read_bytes()).decode()}}]
            if p.stat().st_size>8*1024*1024:raise ValueError('Файл больше 8 МБ. Прочитай нужный участок командой Bash.')
            lines=p.read_text().splitlines();offset=max(0,int(args.get('offset') or 1)-1);limit=max(1,min(2000,int(args.get('limit') or 400)))
            return [{'type':'text','text':'\n'.join(f'{i+1}: {line}' for i,line in enumerate(lines[offset:offset+limit],offset))[:64000]}]
        if name in ('Write','Edit'):
            p=path(args.get('file_path'));p.parent.mkdir(parents=True,exist_ok=True)
            if name=='Write':body=args.get('content')
            else:
                old=args.get('old_string');new=args.get('new_string');body=p.read_text()
                if not isinstance(old,str) or not old or not isinstance(new,str):raise ValueError('Для Edit нужны непустой old_string и new_string.')
                count=body.count(old)
                if not count or (count>1 and not args.get('replace_all')):raise ValueError(f'Найдено совпадений: {count}. Нужна однозначная замена.')
                body=body.replace(old,new,-1 if args.get('replace_all') else 1)
            if not isinstance(body,str) or len(body.encode())>2*1024*1024:raise ValueError('Для записи нужен текст до 2 МБ.')
            mode=stat.S_IMODE(p.stat().st_mode) if p.exists() else 0o644
            fd,temp=tempfile.mkstemp(prefix='.relay-api-',dir=p.parent)
            try:
                with os.fdopen(fd,'w',encoding='utf-8') as f:f.write(body);f.flush();os.fsync(f.fileno())
                os.chmod(temp,mode);os.replace(temp,p)
            finally:
                if os.path.exists(temp):os.unlink(temp)
            return [{'type':'text','text':f'Сохранено: {p} ({len(body.encode())} байт)'}]
        if name=='Glob':
            pattern=args.get('pattern')
            if not isinstance(pattern,str) or not pattern or len(pattern)>2048:raise ValueError('Укажи шаблон поиска.')
            base=path(args.get('path') or self.cwd);found=[]
            for p in glob.iglob(str(base/pattern),recursive=True):
                if self.cancel.is_set():raise Cancelled()
                found.append(p)
                if len(found)>=300:break
            return [{'type':'text','text':'\n'.join(found)}]
        raise ValueError('Неизвестный инструмент.')

    def bash(self,args):
        command=args.get('command')
        if not isinstance(command,str) or not command.strip() or len(command)>65536 or '\0' in command:raise ValueError('Некорректная команда.')
        timeout=max(1,min(600,float(args.get('timeout') or 120000)/1000))
        with tempfile.TemporaryFile() as output:
            self.child=subprocess.Popen(['bash','-c',command],cwd=self.cwd,stdout=output,stderr=subprocess.STDOUT,start_new_session=True)
            end=time.monotonic()+timeout
            try:
                while self.child.poll() is None:
                    if self.cancel.wait(.1) or self.closed:raise Cancelled()
                    if time.monotonic()>end:raise ValueError('Время выполнения команды истекло.')
                    if os.fstat(output.fileno()).st_size>8*1024*1024:raise ValueError('Вывод команды превысил 8 МБ.')
                code=self.child.returncode;output.seek(0);data=output.read(64000).decode(errors='replace')
                return code,f'Exit code: {code}\n{data}'+('\n[Вывод сокращён до 64 КБ]' if os.fstat(output.fileno()).st_size>64000 else '')
            finally:
                if self.child.poll() is None:
                    os.killpg(self.child.pid,signal.SIGTERM)
                    try:self.child.wait(timeout=2)
                    except subprocess.TimeoutExpired:os.killpg(self.child.pid,signal.SIGKILL);self.child.wait()
                self.child=None

    def stop(self):
        self.cancel.set()
        if getattr(self.owner,'desktop',None):self.owner.desktop.cancel(self.sid)
        if getattr(self.owner,'cluster',None):self.owner.cluster.cancel(self.sid)
        for item in list(self.pending.values()):item['event'].set()
        response=self.response
        if response:
            try:response.fp.raw._sock.shutdown(socket.SHUT_RDWR)
            except (AttributeError,OSError):pass
        with self.condition:self.inbox.clear();self.condition.notify()
    def close(self):
        self.closed=True;self.stop()
        if getattr(self.owner,'desktop',None):self.owner.desktop.cancel(self.sid,revoke=True)
        if getattr(self.owner,'cluster',None):self.owner.cluster.cancel(self.sid,revoke=True)
