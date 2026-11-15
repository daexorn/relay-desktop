import importlib.util,json,os,queue,tempfile,threading,time,unittest,uuid
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from pathlib import Path
from unittest import mock

spec=importlib.util.spec_from_file_location('api_bridge',Path(__file__).parents[1]/'bridge/relay_bridge.py')
bridge=importlib.util.module_from_spec(spec);spec.loader.exec_module(bridge)

def text_reply(text):return [{'choices':[{'delta':{'content':text},'finish_reason':'stop'}]},{'usage':{'prompt_tokens':12,'completion_tokens':3},'choices':[]}]
def tool_reply(name,args,ident='call_fixture'):
    raw=json.dumps(args)
    return [{'choices':[{'delta':{'tool_calls':[{'index':0,'id':ident,'function':{'name':name,'arguments':raw[:len(raw)//2]}}]}}]},
            {'choices':[{'delta':{'tool_calls':[{'index':0,'function':{'arguments':raw[len(raw)//2:]}}]},'finish_reason':'tool_calls'}]}]

class ApiTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup);self.root=Path(self.temp.name)
        self.events=queue.Queue();self.requests=[];self.responses=queue.Queue();self.sid=str(uuid.uuid4())
        owner=self
        class Handler(BaseHTTPRequestHandler):
            def log_message(self,*args):pass
            def do_GET(self):
                body=json.dumps({'data':[{'id':'fixture-model'}]}).encode();self.send_response(200);self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(body)
            def do_POST(self):
                data=json.loads(self.rfile.read(int(self.headers['Content-Length'])));owner.requests.append(data)
                reply=owner.responses.get(timeout=5)
                if isinstance(reply,dict) and reply.get('status'):
                    self.send_response(reply['status']);self.end_headers();self.wfile.write(json.dumps({'error':{'message':reply['message']}}).encode());return
                self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
                try:
                    for chunk in reply:
                        self.wfile.write(('data: '+json.dumps(chunk)+'\n\n').encode());self.wfile.flush()
                    self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush()
                except (BrokenPipeError,ConnectionResetError):pass
        self.http=ThreadingHTTPServer(('127.0.0.1',0),Handler)
        threading.Thread(target=self.http.serve_forever,daemon=True).start()
        self.addCleanup(self.http.server_close);self.addCleanup(self.http.shutdown)
        self.api={'base_url':f'http://127.0.0.1:{self.http.server_port}/v1','model':'fixture-model','key':'fixture-only-secret','max_steps':5}
        self.host=bridge.Bridge(send=self.events.put,transcripts=bridge.Transcripts(self.root/'transcripts'),binary='/not-a-real-claude')
        self.host.board_file=None
        self.addCleanup(self.cleanup)
    def cleanup(self):
        self.host.close()
        for run in self.host.runs.values():
            if getattr(run,'backend','')=='api':run.worker.join(3)
    def prompt(self,text='hello',mode='auto',**options):
        self.host.dispatch({'op':'prompt','backend':'api','api':self.api,'sid':self.sid,'cwd':str(self.root),'model':'fixture-model','mode':mode,'text':text,**options})
    def wait(self,kind):
        end=time.monotonic()+7
        while time.monotonic()<end:
            try:event=self.events.get(timeout=.1)
            except queue.Empty:continue
            if event.get('type')==kind:return event
        self.fail('Missing event '+kind)
    def result(self):
        end=time.monotonic()+8
        while time.monotonic()<end:
            event=self.wait('claude')
            if event['data'].get('type')=='result':return event['data']
        self.fail('Missing result')
    def test_streaming_and_same_session_history_without_claude(self):
        self.responses.put(text_reply('Первый ответ'))
        with mock.patch.object(bridge.subprocess,'Popen',side_effect=AssertionError('Claude must not start')):
            self.prompt();result=self.result()
        self.assertFalse(result['is_error']);self.assertEqual(result['usage']['input_tokens'],12)
        self.assertEqual([m['text'] for m in self.host.transcripts.history(self.sid)],['hello','Первый ответ'])
        self.responses.put(text_reply('Продолжение'));self.prompt('second');self.result()
        content=json.dumps(self.requests[-1],ensure_ascii=False)
        self.assertIn('Первый ответ',content);self.assertIn('second',content)
        path=self.host.transcripts.path(self.sid)
        self.assertNotIn(self.api['key'],path.read_text())
        self.assertEqual({row['sessionId'] for row in bridge.records(path)},{self.sid})
        self.assertEqual(self.host.transcripts.metadata(path)['backend'],'api')
        self.host.run_state(self.host.runs[self.sid])
        self.assertEqual(self.wait('run_state')['backend'],'api')
    def test_tools_execute_and_the_next_request_contains_their_results(self):
        self.responses.put(tool_reply('Write',{'file_path':'answer.txt','content':'fixture output'}))
        self.responses.put(text_reply('Сохранено'))
        self.prompt();self.assertFalse(self.result()['is_error'])
        self.assertEqual((self.root/'answer.txt').read_text(),'fixture output')
        messages=self.requests[-1]['messages'];self.assertTrue(any(m.get('role')=='tool' and 'Сохранено' in m.get('content','') for m in messages))
        history=self.host.transcripts.history(self.sid)
        self.assertTrue(any(m['role']=='tool' and m['tool']=='Write' for m in history))
        self.assertTrue(any(m['role']=='tool_result' for m in history))
    def test_manual_denial_prevents_writing(self):
        self.responses.put(tool_reply('Write',{'file_path':'denied.txt','content':'must not exist'}));self.responses.put(text_reply('Не меняю'))
        self.prompt(mode='manual');permission=self.wait('permission')
        self.assertFalse((self.root/'denied.txt').exists())
        self.host.dispatch({'op':'permission','sid':self.sid,'id':permission['id'],'allow':False});self.result()
        self.assertFalse((self.root/'denied.txt').exists())
        self.assertIn('отклонил',json.dumps(self.requests[-1],ensure_ascii=False))
    def test_plan_mode_blocks_shell_and_write(self):
        self.responses.put(tool_reply('Bash',{'command':'touch plan-must-not-write'}));self.responses.put(text_reply('План'))
        self.prompt(mode='plan');self.result()
        self.assertFalse((self.root/'plan-must-not-write').exists())
        self.assertIn('В режиме плана',json.dumps(self.requests[-1],ensure_ascii=False))
    def test_error_never_exposes_key_or_repeats_the_request(self):
        self.responses.put({'status':401,'message':'Rejected '+self.api['key']})
        self.prompt();result=self.result()
        self.assertTrue(result['is_error']);self.assertNotIn(self.api['key'],json.dumps(result));self.assertEqual(len(self.requests),1)
    def test_history_import_uses_the_latest_native_branch(self):
        p=self.host.transcripts.root/'project';p.mkdir(parents=True)
        rows=[{'type':'user','uuid':'u','parentUuid':None,'cwd':str(self.root),'message':{'role':'user','content':'original question'}},
              {'type':'assistant','uuid':'abandoned','parentUuid':'u','message':{'role':'assistant','content':'discarded answer'}},
              {'type':'assistant','uuid':'latest','parentUuid':'u','message':{'role':'assistant','content':'actual native answer'}}]
        (p/(self.sid+'.jsonl')).write_text('\n'.join(json.dumps(r) for r in rows)+'\n')
        self.responses.put(text_reply('API continuation'));self.prompt();self.result()
        body=json.dumps(self.requests[0]);self.assertIn('actual native answer',body);self.assertNotIn('discarded answer',body)
        history=self.host.transcripts.history(self.sid);self.assertEqual(history[-1]['text'],'API continuation')
        self.assertTrue(any(m['text']=='actual native answer' for m in history))
    def test_stop_shell_preserves_tool_pair_and_terminates_the_child(self):
        self.responses.put(tool_reply('Bash',{'command':'sleep 20'}))
        self.prompt();end=time.monotonic()+5
        while time.monotonic()<end and not getattr(self.host.runs.get(self.sid),'child',None):time.sleep(.02)
        run=self.host.runs[self.sid];self.assertIsNotNone(run.child);child=run.child
        self.host.dispatch({'op':'stop','sid':self.sid});result=self.result()
        self.assertTrue(result['is_error']);self.assertIsNotNone(child.poll())
        history=self.host.transcripts.history(self.sid)
        uses=[m['tool_id'] for m in history if m['role']=='tool'];results=[m['tool_id'] for m in history if m['role']=='tool_result']
        self.assertEqual(uses,results);self.assertEqual(len(self.requests),1)
    def test_api_catalog_is_separate_and_contains_no_secret(self):
        self.host.dispatch({'op':'api_models','api':self.api,'request':'catalog-test'})
        event=self.wait('api_models');self.assertEqual(event['models'][0]['value'],'fixture-model')
        self.assertEqual(event['request'],'catalog-test');self.assertNotIn(self.api['key'],json.dumps(event))
    def test_existing_board_mcp_runs_without_claude(self):
        board=self.root/'board.py';board.write_text((Path(__file__).parents[1]/'bridge/board.py').read_text())
        self.host.board_file=board
        self.responses.put(tool_reply('mcp__relay_board__board_overview',{}));self.responses.put(text_reply('Доска проверена'))
        with mock.patch.dict(os.environ,{'RELAY_BOARD_DIR':str(self.root/'board-data')}):
            self.prompt();result=self.result()
        self.assertFalse(result['is_error'],result)
        self.assertTrue(any(t['function']['name']=='mcp__relay_board__board_overview' for t in self.requests[0]['tools']))
        self.assertTrue(any(m.get('role')=='tool' for m in self.requests[-1]['messages']))
    def test_nonzero_shell_exit_is_marked_as_a_failed_action(self):
        self.responses.put(tool_reply('Bash',{'command':'printf failure; exit 7'}));self.responses.put(text_reply('Команда завершилась ошибкой'))
        self.prompt();self.result()
        result=next(m for m in self.host.transcripts.history(self.sid) if m['role']=='tool_result')
        self.assertTrue(result['failed']);self.assertIn('Exit code: 7',result['text'])
    def test_images_are_preserved_for_api_and_claude_history(self):
        self.responses.put(text_reply('Image received'))
        image={'type':'base64','media_type':'image/png','data':'aW1n'}
        self.host.dispatch({'op':'prompt','backend':'api','api':self.api,'sid':self.sid,'cwd':str(self.root),'model':'fixture-model','mode':'auto','text':'image','images':[image]})
        self.result()
        self.assertTrue(any(isinstance(m.get('content'),list) and any(b.get('type')=='image_url' for b in m['content']) for m in self.requests[0]['messages']))
        self.assertEqual(self.host.transcripts.history(self.sid)[0]['images'][0]['data'],'aW1n')
    def test_native_run_cannot_take_over_busy_api_session(self):
        self.responses.put(tool_reply('Write',{'file_path':'later.txt','content':'x'}));self.responses.put(text_reply('done'))
        self.prompt(mode='manual');permission=self.wait('permission');run=self.host.runs[self.sid]
        self.host.dispatch({'op':'prompt','sid':self.sid,'cwd':str(self.root),'text':'switch'})
        self.assertIn('переключением',self.wait('error')['message']);self.assertIs(self.host.runs[self.sid],run)
        self.host.dispatch({'op':'permission','sid':self.sid,'id':permission['id'],'allow':False});self.result()
    def test_api_session_with_desktop_can_change_mode_and_keep_its_history(self):
        desktop=(Path(__file__).parents[1]/'bridge/desktop_mcp.py').read_text()
        with mock.patch.object(bridge,'DESKTOP_SOURCE',desktop,create=True):
            self.responses.put(text_reply('First connected-PC response'))
            self.prompt(desktop_enabled=True,desktop_context={'local':True});self.assertFalse(self.result()['is_error'])
            first=self.host.runs[self.sid]
            self.responses.put(text_reply('Second connected-PC response'))
            self.prompt('continue',mode='manual',desktop_enabled=True,desktop_context={'local':True})
            self.assertFalse(self.result()['is_error']);first.worker.join(3)
        self.assertTrue(first.closed);self.assertFalse(first.worker.is_alive());self.assertIsNot(self.host.runs[self.sid],first)
        self.assertEqual([m['text'] for m in self.host.transcripts.history(self.sid) if m['role'] in ('user','assistant')],
                         ['hello','First connected-PC response','continue','Second connected-PC response'])
        self.assertTrue(any(t['function']['name']=='mcp__relay_desktop__run_local_command' for t in self.requests[-1]['tools']))
    def test_bridge_close_with_connected_api_desktop_finishes_cleanly(self):
        desktop=(Path(__file__).parents[1]/'bridge/desktop_mcp.py').read_text()
        with mock.patch.object(bridge,'DESKTOP_SOURCE',desktop,create=True):
            self.responses.put(text_reply('Connected-PC fixture'))
            self.prompt(desktop_enabled=True,desktop_context={'local':True});self.assertFalse(self.result()['is_error'])
        run=self.host.runs[self.sid];self.host.close();run.worker.join(3)
        self.assertTrue(run.closed);self.assertFalse(run.worker.is_alive());self.assertTrue(self.host.desktop.closed)

if __name__=='__main__':unittest.main()
