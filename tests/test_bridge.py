import base64
import importlib.util
import io
import json
import socket
import stat
import threading
import os
from pathlib import Path
import queue
import tempfile
import time
import threading
import unittest
from unittest import mock
import uuid

SPEC = importlib.util.spec_from_file_location("relay_bridge", Path(__file__).parents[1] / "bridge/relay_bridge.py")
bridge = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bridge)

FAKE = '''#!/usr/bin/env python3
import json,sys
def send(x):print(json.dumps(x),flush=True)
for line in sys.stdin:
 r=json.loads(line)
 if r.get('type')=='control_request':
  send({'type':'control_response','response':{'subtype':'success','request_id':r['request_id'],'response':{}}})
 elif r.get('type')=='user':
  send({'type':'control_request','request_id':'test-permission','request':{'subtype':'can_use_tool','tool_name':('mcp__relay_desktop__run_local_command' if r.get('message',{}).get('content',[{}])[0].get('text')=='relay-pc-tool' else 'Edit'),'input':{'file_path':'/tmp/a'}}})
 elif r.get('type')=='control_response':
  send({'type':'assistant','message':{'content':[{'type':'text','text':r['response']['response']['behavior']}]}})
  send({'type':'result','is_error':False,'result':'done'})
'''


class TranscriptTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.project = self.root / '-opt-project'
        self.project.mkdir()
        self.sid = str(uuid.uuid4())
        self.path = self.project / (self.sid + '.jsonl')
        self.store = bridge.Transcripts(self.root)

    def write(self, rows):
        self.path.write_text('\n'.join(json.dumps(r) for r in rows) + '\n')

    def test_history_follows_parent_links_through_progress(self):
        self.write([
            {'type':'user','uuid':'a','parentUuid':None,'cwd':'/opt/project','message':{'role':'user','content':'Привет'}},
            {'type':'assistant','uuid':'discarded','parentUuid':'a','message':{'role':'assistant','content':'old branch'}},
            {'type':'progress','uuid':'p','parentUuid':'a'},
            {'type':'assistant','uuid':'b','parentUuid':'p','message':{'role':'assistant','content':[{'type':'text','text':'Ответ'}]}},
            {'type':'assistant','uuid':'sub','parentUuid':'b','isSidechain':True,'message':{'role':'assistant','content':'not the main conversation'}},
        ])
        self.assertEqual([m['text'] for m in self.store.history(self.sid)], ['Привет','Ответ'])

    def test_cached_metadata_refreshes_after_append(self):
        self.write([{'type':'user','uuid':'a','cwd':'/opt/project','message':{'content':'Первая задача'}}])
        self.assertEqual(self.store.sessions()[0]['title'],'Первая задача')
        with self.path.open('a') as f:f.write(json.dumps({'type':'custom-title','customTitle':'Новое название'})+'\n')
        self.assertEqual(self.store.sessions()[0]['title'],'Новое название')
        self.assertEqual(self.store.sessions()[0]['cwd'],'/opt/project')

    def test_partial_last_line_is_ignored(self):
        self.write([{'type':'user','uuid':'a','parentUuid':None,'message':{'role':'user','content':'valid'}}])
        with self.path.open('a') as f:f.write('{"type":"assistant"')
        self.assertEqual(self.store.history(self.sid)[0]['text'],'valid')

    def test_missing_and_injected_session_ids_rejected(self):
        for value in ['../../secret','x; echo no','',str(uuid.uuid4())]:
            with self.assertRaises(ValueError):self.store.path(value)

    def test_user_images_arrive_as_attachments_not_text(self):
        self.write([{'type':'user','uuid':'a','parentUuid':None,'message':{'role':'user','content':[{'type':'text','text':'смотри'},{'type':'image','source':{'media_type':'image/png','data':'aW1n'}}]}}])
        history=self.store.history(self.sid)
        self.assertEqual(history[0]['text'],'смотри')
        self.assertEqual(history[0]['images'],[{'media_type':'image/png','data':'aW1n'}])

    def test_images_beyond_the_budget_become_placeholders(self):
        with mock.patch.object(bridge,'MAX_HISTORY_IMAGES',6):
            self.write([{'type':'user','uuid':'a','parentUuid':None,'message':{'role':'user','content':[{'type':'image','source':{'data':'old-old-old'}}]}},
                        {'type':'user','uuid':'b','parentUuid':'a','message':{'role':'user','content':[{'type':'image','source':{'data':'new'}}]}}])
            history=self.store.history(self.sid)
        self.assertEqual([h['images'][0]['data'] for h in history],['','new'])

    def test_interrupts_and_meta_rows_are_hidden(self):
        self.write([{'type':'user','uuid':'a','parentUuid':None,'message':{'role':'user','content':'вопрос'}},
                    {'type':'user','uuid':'b','parentUuid':'a','message':{'role':'user','content':[{'type':'text','text':'[Request interrupted by user]'}]}},
                    {'type':'assistant','uuid':'c','parentUuid':'b','message':{'role':'assistant','content':[{'type':'text','text':'No response requested.'}]}},
                    {'type':'user','uuid':'d','parentUuid':'c','isMeta':True,'message':{'role':'user','content':[{'type':'text','text':'[Image: source: /tmp/x.png]'}]}},
                    {'type':'user','uuid':'e','parentUuid':'d','message':{'role':'user','content':'[Request interrupted by user for tool use]'}},
                    {'type':'assistant','uuid':'f','parentUuid':'e','message':{'role':'assistant','content':[{'type':'text','text':'ответ'}]}}])
        self.assertEqual([h['text'] for h in self.store.history(self.sid)],['вопрос','ответ'])

    def test_session_meta_is_shared_and_survives_bad_input(self):
        meta=bridge.SessionMeta(self.root/'shared/sessions.json')
        folder=meta.folder(name='Корабли')
        meta.session(self.sid,title='  Якорь   и цепь ',pinned=True,folder=folder,actor='champ')
        with self.assertRaises(ValueError):meta.folder(name='корабли')
        with self.assertRaises(ValueError):meta.session(self.sid,folder='f-missing')
        rows,folders=bridge.SessionMeta(self.root/'shared/sessions.json').apply([{'id':self.sid,'title':'auto','cwd':'/','modified':1}])
        self.assertEqual((rows[0]['title'],rows[0]['auto_title'],rows[0]['folder']),('Якорь и цепь','auto',folder))
        self.assertGreater(rows[0]['pinned'],0)
        self.assertEqual(folders[0]['name'],'Корабли')
        meta.folder(folder,name='Флот');meta.session(self.sid,title='',pinned=False)
        rows,folders=meta.apply([{'id':self.sid,'title':'auto','cwd':'/','modified':1}])
        self.assertEqual((rows[0]['title'],rows[0]['pinned'],rows[0]['folder'],folders[0]['name']),('auto',0.0,folder,'Флот'))
        meta.folder(folder,delete=True)
        rows,folders=meta.apply([{'id':self.sid,'title':'auto','cwd':'/','modified':1}])
        self.assertEqual((rows[0]['folder'],folders),('',[]))

    def test_existing_sessions_are_never_rewritten(self):
        self.write([{'type':'user','uuid':'a','message':{'content':'x'}}])
        original=self.path.read_bytes();self.store.sessions();self.store.history(self.sid)
        self.assertEqual(original,self.path.read_bytes())

    def test_resume_flags_are_not_session_titles(self):
        self.write([{'type':'user','uuid':'a','cwd':'/opt/project','message':{'content':'--resume'}},
                    {'type':'user','uuid':'b','message':{'content':'Сделай удобный интерфейс'}},
                    {'type':'custom-title','customTitle':'--resume'}])
        self.assertEqual(self.store.sessions()[0]['title'],'Сделай удобный интерфейс')
        self.assertEqual(bridge.visible_text('--resume'),'')

    def test_attachment_metadata_is_readable_without_cache_paths(self):
        text='Проверь файл\n<relay-files>'+json.dumps([{'name':'notes.md','path':'/private/cache/notes.md'}])+'</relay-files>'
        visible=bridge.visible_text(text)
        self.assertIn('notes.md',visible)
        self.assertNotIn('/private/cache',visible)

    def test_delete_removes_one_conversation_and_its_subagents_only(self):
        self.write([{'type':'user','uuid':'a','message':{'content':'delete me'}}])
        self.store.sessions()  # Cached metadata must not bring it back.
        other = self.project / (str(uuid.uuid4()) + '.jsonl')
        other.write_text('{"type":"user","message":{"content":"keep me"}}\n')
        children = self.path.with_suffix('') / 'subagents'; children.mkdir(parents=True)
        (children / 'agent.jsonl').write_text('subagent data')
        project_file = self.project / 'source.rs'; project_file.write_text('project file')
        before = self.path.read_bytes()
        self.store.delete(self.sid)
        self.assertIsNone(self.store.find(self.sid))
        self.assertEqual([s['id'] for s in self.store.sessions()], [other.stem])
        self.assertEqual(other.read_text(), '{"type":"user","message":{"content":"keep me"}}\n')
        self.assertEqual(project_file.read_text(), 'project file')
        archives = list((self.project / '.relay-deleted').glob('*/' + self.path.name))
        self.assertEqual(len(archives), 1)
        self.assertEqual(archives[0].read_bytes(), before)
        self.assertEqual((archives[0].parent / self.sid / 'subagents/agent.jsonl').read_text(), 'subagent data')
        self.store.delete(self.sid)  # Retry after a lost acknowledgement is safe.

    def test_invalid_delete_never_touches_an_unrelated_path(self):
        self.write([{'type':'user','message':{'content':'keep'}}])
        original = self.path.read_bytes()
        for value in ['../../secret', '', None]:
            with self.assertRaises(ValueError): self.store.delete(value)
        self.assertEqual(self.path.read_bytes(), original)

    def test_delete_rolls_back_if_subagent_move_fails(self):
        self.write([{'type':'user','message':{'content':'keep'}}])
        children = self.path.with_suffix(''); children.mkdir()
        original = self.path.read_bytes()
        rename = bridge.os.rename
        calls = 0
        def fail_second(*args, **kwargs):
            nonlocal calls
            calls += 1
            if calls == 2: raise PermissionError('fixture')
            return rename(*args, **kwargs)
        with mock.patch.object(bridge.os, 'rename', side_effect=fail_second):
            with self.assertRaises(PermissionError): self.store.delete(self.sid)
        self.assertEqual(self.path.read_bytes(), original)
        self.assertTrue(children.is_dir())


class ProtocolTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name);self.cli=self.root/'claude'
        self.cli.write_text(FAKE);self.cli.chmod(0o700)
        self.events=queue.Queue()
        self.host=bridge.Bridge(send=self.events.put,transcripts=bridge.Transcripts(self.root/'transcripts'),binary=str(self.cli))
        def cleanup_host():
            self.host.close()
            for run in self.host.runs.values():
                for stream in [run.process.stdin,run.process.stdout,run.process.stderr]:
                    stream.close()
        self.addCleanup(cleanup_host)
        self.sid=str(uuid.uuid4())

    def wait(self, kind):
        end=time.monotonic()+5
        while time.monotonic()<end:
            try:event=self.events.get(timeout=.2)
            except queue.Empty:continue
            if event.get('type')==kind:return event
        self.fail('Timed out waiting for '+kind)

    def prompt(self):
        self.host.dispatch({'op':'prompt','sid':self.sid,'cwd':str(self.root),'text':'test','mode':'manual'})

    def test_initialize_permission_and_continuation(self):
        self.prompt();event=self.wait('permission');self.assertEqual(event['tool'],'Edit')
        self.host.dispatch({'op':'permission','sid':self.sid,'id':event['id'],'allow':True})
        result=self.wait('claude');self.assertEqual(result['data']['message']['content'][0]['text'],'allow')
        self.wait('claude')
        self.assertFalse(self.host.runs[self.sid].busy)
        # Same process accepts another turn, rather than accidentally forking the conversation.
        pid=self.host.runs[self.sid].process.pid
        self.prompt();self.wait('permission');self.assertEqual(pid,self.host.runs[self.sid].process.pid)

    def test_desktop_tool_permission_is_delegated_to_native_client(self):
        source=(Path(__file__).parents[1]/'bridge/desktop_mcp.py').read_text()
        with mock.patch.object(bridge,'DESKTOP_SOURCE',source,create=True):
            self.host.dispatch({'op':'prompt','sid':self.sid,'cwd':str(self.root),'text':'relay-pc-tool','mode':'manual','desktop_enabled':True})
        event=self.wait('claude')
        self.assertEqual(event['data']['message']['content'][0]['text'],'allow')

    def test_board_ops_and_session_mcp_config(self):
        source=(Path(__file__).parents[1]/'bridge/board.py').read_text()
        with mock.patch.dict('os.environ',{'RELAY_BOARD_DIR':str(self.root/'board'),'HOME':str(self.root)}),mock.patch.object(bridge,'BOARD_SOURCE',source,create=True):
            host=bridge.Bridge(send=self.events.put,transcripts=bridge.Transcripts(self.root/'transcripts'),binary=str(self.cli))
            self.assertTrue(host.board_file.exists())
            # An older Relay must not replace a newer board.py next to the data.
            host.board_file.write_text("BOARD_VERSION = 99\n# newer\n")
            bridge.load_board();self.assertIn('newer',host.board_file.read_text())
            host.board_file.write_text("BOARD_VERSION = 1\n")
            bridge.load_board();self.assertIn('BOARD_VERSION = 3',host.board_file.read_text())
            host.dispatch({'op':'board_apply','actor':'champ','host':'pc','ops':[{'op':'create','title':'Плавильня','status':'todo'}]})
            state=self.wait('board')['state'];self.assertEqual(state['tasks'][0]['title'],'Плавильня');self.assertEqual(state['activity'][0]['actor'],'champ')
            host.dispatch({'op':'board','since':state['rev']});self.assertTrue(self.wait('board')['state']['unchanged'])
            host.dispatch({'op':'board_apply','ops':[{'op':'move','id':'T-404','status':'done'}]})
            self.assertIn('T-404',self.wait('board_error')['message'])
            with mock.patch.object(bridge.subprocess,'Popen',side_effect=OSError('stop')) as popen:
                with self.assertRaises(OSError):bridge.Run(host,self.sid,str(self.root),'default','manual',False)
            args=popen.call_args[0][0];config=json.loads(args[args.index('--mcp-config')+1])
            self.assertEqual(config['mcpServers']['relay_board']['args'],[str(host.board_file),'mcp'])
    def test_cluster_role_is_passed_to_the_system_prompt_not_user_text(self):
        self.host.dispatch({'op':'prompt','sid':self.sid,'cwd':str(self.root),'text':'test','mode':'manual','agent_role':'reviewer'})
        self.wait('permission')
        run=self.host.runs[self.sid]
        self.assertEqual(run.agent_role,'reviewer')
        args=run.process.args
        self.assertIn('review agent',args[args.index('--append-system-prompt')+1])
        self.assertIn('renders LaTeX',args[args.index('--append-system-prompt')+1])

    def test_leader_gets_cluster_tools_and_custom_prompt_but_workers_do_not(self):
        source=(Path(__file__).parents[1]/'bridge/cluster_mcp.py').read_text()
        run_id=str(uuid.uuid4())
        with mock.patch.object(bridge,'CLUSTER_SOURCE',source,create=True):
            self.host.dispatch({'op':'prompt','sid':self.sid,'cwd':str(self.root),'text':'test','mode':'manual','cluster_context':{'run':run_id,'role':'leader'},'custom_prompt':'Follow my project conventions.'})
            self.wait('permission');leader=self.host.runs[self.sid]
            self.assertIn('Follow my project conventions.',leader.process.args[leader.process.args.index('--append-system-prompt')+1])
            configs=json.loads(leader.process.args[leader.process.args.index('--mcp-config')+1])
            self.assertIn('relay_cluster',configs['mcpServers'])
            sid=str(uuid.uuid4())
            self.host.dispatch({'op':'prompt','sid':sid,'cwd':str(self.root),'text':'test','mode':'manual','cluster_context':{'run':run_id,'role':'worker'}})
            self.wait('permission');worker=self.host.runs[sid]
            if '--mcp-config' in worker.process.args:
                self.assertNotIn('relay_cluster',json.loads(worker.process.args[worker.process.args.index('--mcp-config')+1])['mcpServers'])
            self.assertIn('worker directed by the lead',worker.process.args[worker.process.args.index('--append-system-prompt')+1])

    def test_worker_returns_questions_to_the_leader_instead_of_prompting_the_user(self):
        question=FAKE.replace("else 'Edit'", "else ('AskUserQuestion' if r.get('message',{}).get('content',[{}])[0].get('text')=='worker-question' else 'Edit')")
        self.cli.write_text(question)
        self.host.dispatch({'op':'prompt','sid':self.sid,'cwd':str(self.root),'text':'worker-question','mode':'manual','cluster_context':{'run':str(uuid.uuid4()),'role':'worker'}})
        event=self.wait('claude')
        self.assertEqual(event['data']['message']['content'][0]['text'],'deny')
        self.assertFalse(self.host.runs[self.sid].pending)

    def test_unknown_cluster_role_does_not_start_a_process(self):
        self.host.dispatch({'op':'prompt','sid':self.sid,'cwd':str(self.root),'text':'test','agent_role':'unknown'})
        event=self.wait('error')
        self.assertIn('роль',event['message'])
        self.assertNotIn(self.sid,self.host.runs)

    def test_prompt_during_an_answer_joins_the_queue(self):
        self.prompt();self.wait('permission')
        run=self.host.runs[self.sid]
        self.prompt()
        state=self.wait('run_state')
        while state['queued']!=1:state=self.wait('run_state')
        self.assertIs(self.host.runs[self.sid],run);self.assertEqual(run.turns,2);self.assertTrue(run.busy)
        self.assertEqual(self.host.running(),{self.sid})
        self.host.dispatch({'op':'permission','sid':self.sid,'id':'test-permission','allow':True})
        self.wait('claude')
        # The first answer is done, the queued message is still being answered.
        while True:
            state=self.wait('run_state')
            if state['queued']==0:break
        self.assertTrue(run.busy);self.assertEqual(run.turns,1)

    def test_model_switches_live_during_an_answer(self):
        self.prompt();self.wait('permission')
        run=self.host.runs[self.sid]
        with mock.patch.object(run,'control') as control:
            self.host.dispatch({'op':'prompt','sid':self.sid,'cwd':str(self.root),'text':'ещё','mode':'auto','model':'opus'})
            control.assert_any_call('set_model',model='opus');control.assert_any_call('set_permission_mode',mode='auto')
            self.host.dispatch({'op':'control','sid':self.sid,'action':'model','value':'default'})
            control.assert_called_with('set_model',model=None)
        self.assertIs(self.host.runs[self.sid],run)

    def test_catalog_comes_from_initialize(self):
        self.host.learn_catalog({'models':[{'value':'claude-fable-5-2','displayName':'Fable 5.2','description':'x','supportsEffort':True}],
                                 'commands':[{'name':'compact','description':'Сжать','argumentHint':'[focus]'},{'name':'__internal'}],'claude_code_version':'2.1'})
        event=self.wait('catalog')
        self.assertEqual(event['models'],[{'value':'claude-fable-5-2','displayName':'Fable 5.2','description':'x'}])
        self.assertEqual(event['commands'],[{'name':'compact','description':'Сжать','hint':'[focus]'}])

    def test_permission_decline_is_sent_to_claude(self):
        self.prompt();event=self.wait('permission')
        self.host.dispatch({'op':'permission','sid':self.sid,'id':event['id'],'allow':False})
        result=self.wait('claude');self.assertEqual(result['data']['message']['content'][0]['text'],'deny')

    def test_shell_metacharacters_in_model_are_an_argument(self):
        args=bridge.command('/bin/claude',self.sid,True,'name; $(no)','manual')
        self.assertIn('name; $(no)',args);self.assertIn('--resume='+self.sid,args)
        self.assertNotIn('--dangerously-skip-permissions',args)

    def test_invented_permission_mode_rejected(self):
        with self.assertRaises(ValueError):bridge.command('claude',self.sid,False,'default','bypassPermissions')

    def test_native_auto_mode_does_not_skip_permissions(self):
        args=bridge.command('claude',self.sid,False,'default','auto')
        self.assertEqual(args[args.index('--permission-mode')+1],'auto')
        self.assertNotIn('--dangerously-skip-permissions',args)

    def test_effort_default_and_supported_cli_arguments(self):
        default=bridge.command('claude',self.sid,False,'default','manual')
        self.assertNotIn('--effort',default)
        for effort in ['low','medium','high','xhigh','max']:
            with self.subTest(effort=effort):
                args=bridge.command('claude',self.sid,True,'default','auto',effort)
                self.assertEqual(args[args.index('--effort')+1],effort)
                self.assertIn('--resume='+self.sid,args)
                self.assertNotIn('--dangerously-skip-permissions',args)

    def test_invalid_effort_is_rejected_before_starting_claude(self):
        for effort in ['ultra','',None,[],{'level':'high'}]:
            with self.subTest(effort=effort):
                with self.assertRaises(ValueError):
                    bridge.command('claude',self.sid,False,'default','manual',effort)
                with mock.patch.object(bridge,'Run') as constructor:
                    self.host.dispatch({'op':'prompt','sid':self.sid,'cwd':str(self.root),'text':'test','effort':effort})
                    event=self.wait('error')
                    self.assertIn('усилий',event['message'])
                    constructor.assert_not_called()
                self.assertFalse(self.host.runs)

    def test_effort_change_waits_for_idle_and_resumes_same_session(self):
        def cleanup(run):
            run.close()
            for stream in [run.process.stdin,run.process.stdout,run.process.stderr]:
                stream.close()
        request={'op':'prompt','sid':self.sid,'cwd':str(self.root),'text':'test','mode':'manual','effort':'low'}
        self.host.dispatch(request)
        permission=self.wait('permission')
        first=self.host.runs[self.sid]
        self.addCleanup(cleanup,first)
        self.host.dispatch({'op':'permission','sid':self.sid,'id':permission['id'],'allow':True})
        self.wait('claude');self.wait('claude')
        self.assertFalse(first.busy)
        # The real CLI persists the transcript before returning the turn's result.
        project=self.host.transcripts.root/'project';project.mkdir(parents=True)
        (project/(self.sid+'.jsonl')).write_text(json.dumps({'type':'user','uuid':'first','cwd':str(self.root),'message':{'content':'test'}})+'\n')
        self.host.dispatch(dict(request,effort='high'))
        self.wait('permission')
        second=self.host.runs[self.sid]
        self.addCleanup(cleanup,second)
        self.assertTrue(first.closed)
        self.assertNotEqual(first.process.pid,second.process.pid)
        self.assertEqual(second.effort,'high')
        self.assertIn('--resume='+self.sid,second.process.args)
        self.assertEqual(second.process.args[second.process.args.index('--effort')+1],'high')

    def test_files_are_saved_outside_the_project_without_overwrite(self):
        files=bridge.decode_files([{'type':'file','name':'../../notes.txt','data':base64.b64encode(b'first').decode()},
                                  {'type':'file','name':'notes.txt','data':base64.b64encode(b'second').decode()}])
        saved=bridge.store_files(self.sid,files,self.root/'uploads')
        self.assertEqual([Path(f['path']).read_bytes() for f in saved],[b'first',b'second'])
        self.assertTrue(all(Path(f['path']).is_relative_to(self.root/'uploads') for f in saved))
        self.assertNotEqual(saved[0]['path'],saved[1]['path'])

    def test_file_only_message_is_accepted(self):
        original=bridge.store_files
        with mock.patch.object(bridge,'store_files',side_effect=lambda sid,files:original(sid,files,self.root/'uploads')):
            self.host.dispatch({'op':'prompt','sid':self.sid,'cwd':str(self.root),'text':'','files':[{'type':'file','name':'data.txt','data':base64.b64encode(b'hello').decode()}],'mode':'auto'})
            self.wait('permission')
        self.assertEqual(next((self.root/'uploads').glob('*/*/*.txt')).read_bytes(),b'hello')

    def test_invalid_files_are_rejected(self):
        for files in [[{}],[{'type':'file','name':'..','data':''}],[{'type':'file','name':'x','data':'%%%'}]]:
            with self.assertRaises(ValueError):bridge.decode_files(files)

    def test_busy_session_cannot_be_deleted(self):
        self.prompt(); self.wait('permission')
        run = self.host.runs[self.sid]
        self.host.dispatch({'op':'delete_session','sid':self.sid})
        event = self.wait('error')
        self.assertEqual(event['op'], 'delete_session')
        self.assertIs(self.host.runs[self.sid], run)
        self.assertFalse(run.closed)

    def test_idle_session_closes_its_child_before_deletion(self):
        self.prompt(); permission = self.wait('permission')
        run = self.host.runs[self.sid]
        self.addCleanup(lambda: [stream.close() for stream in [run.process.stdin, run.process.stdout, run.process.stderr]])
        self.host.dispatch({'op':'permission','sid':self.sid,'id':permission['id'],'allow':True})
        self.wait('claude'); self.wait('claude')
        project = self.host.transcripts.root / 'project'; project.mkdir(parents=True)
        path = project / (self.sid + '.jsonl'); path.write_text('{"type":"user","message":{"content":"test"}}\n')
        self.host.dispatch({'op':'delete_session','sid':self.sid})
        self.assertEqual(self.wait('session_deleted')['sid'], self.sid)
        self.assertNotIn(self.sid, self.host.runs)
        self.assertTrue(run.closed)
        self.assertFalse(path.exists())

    def test_images_are_bounded(self):
        bridge.validate_images([{'type':'base64','media_type':'image/png','data':base64.b64encode(b'png bytes').decode()}])
        for value in ['bad',{},[{}],[{'type':'base64','media_type':'image/png','data':'%%%'}]]:
            with self.assertRaises(ValueError):bridge.validate_images(value)


class UsageTests(unittest.TestCase):
    def test_stream_values_are_real_fractions_without_raw_data_leaks(self):
        state=bridge.UsageState()
        self.assertFalse(state.snapshot()['available'])
        self.assertEqual(state.snapshot()['updated_at'],0)
        state.observe({'type':'rate_limit_event','rate_limit_info':{'status':'allowed_warning','rateLimitType':'five_hour','utilization':.42,'resetsAt':time.time()+3600,'secret':'do-not-forward'}})
        result=state.snapshot()
        self.assertEqual(result['windows'][0]['utilization'],42)
        self.assertEqual(result['status'],'allowed_warning')
        self.assertNotIn('do-not-forward',json.dumps(result))
        state.observe({'type':'rate_limit_event','rate_limit_info':{'status':'allowed','rateLimitType':'seven_day','utilization':True}})
        self.assertIsNone(state.snapshot()['windows'][1]['utilization'])
        state.observe({'type':'rate_limit_event','rate_limit_info':{'status':'rejected','rateLimitType':'five_hour','resetsAt':time.time()-1}})
        self.assertNotIn('five_hour',[window['id'] for window in state.snapshot()['windows']])

    def test_oauth_uses_only_fixed_endpoint_and_sanitized_windows(self):
        with tempfile.TemporaryDirectory() as folder:
            Path(folder,'.credentials.json').write_text(json.dumps({'claudeAiOauth':{'accessToken':'fixture-only-secret','expiresAt':(time.time()+3600)*1000,'subscriptionType':'max','scopes':['user:profile']}}))
            response={'five_hour':{'utilization':37.5,'resets_at':'2099-01-01T10:00:00Z'},'seven_day':{'utilization':0,'resets_at':'2099-01-07T10:00:00Z'},'accessToken':'not-for-ui'}
            state=bridge.UsageState()
            with mock.patch.dict(bridge.os.environ,{'CLAUDE_CONFIG_DIR':folder}),mock.patch.object(bridge.urllib.request,'build_opener') as factory:
                factory.return_value.open.return_value=io.BytesIO(json.dumps(response).encode())
                state.fetch_oauth()
                request=factory.return_value.open.call_args.args[0]
                self.assertEqual(request.full_url,'https://api.anthropic.com/api/oauth/usage')
                self.assertEqual(factory.return_value.open.call_args.kwargs['timeout'],6)
            result=state.snapshot()
            self.assertEqual([window['utilization'] for window in result['windows']],[37.5,0])
            self.assertIsInstance(result['windows'][0]['resets_at'],int)
            self.assertNotIn('fixture-only-secret',json.dumps(result))
            self.assertNotIn('not-for-ui',json.dumps(result))
            self.assertIsNone(bridge.UsageNoRedirect().redirect_request(None,None,302,'',{},'https://other.invalid/'))

    def test_expired_or_wrong_scope_credentials_never_send_request(self):
        with tempfile.TemporaryDirectory() as folder:
            for expires,scopes in [(0,['user:profile']),((time.time()+3600)*1000,['user:inference'])]:
                Path(folder,'.credentials.json').write_text(json.dumps({'claudeAiOauth':{'accessToken':'fixture-only-secret','expiresAt':expires,'subscriptionType':'pro','scopes':scopes}}))
                with mock.patch.dict(bridge.os.environ,{'CLAUDE_CONFIG_DIR':folder}),mock.patch.object(bridge.urllib.request,'build_opener') as factory:
                    bridge.UsageState().fetch_oauth()
                    factory.assert_not_called()

    def test_refresh_runs_in_background_and_is_cached_for_sixty_seconds(self):
        state=bridge.UsageState();done=threading.Event()
        with mock.patch.object(state,'fetch_oauth') as fetch:
            state.refresh_async(lambda result:done.set())
            self.assertTrue(done.wait(2))
            state.refresh_async(lambda result:None)
            self.assertEqual(fetch.call_count,1)


if __name__=='__main__':unittest.main()

class LegacySessionTests(unittest.TestCase):
    def test_corrupt_rows_do_not_hide_other_sessions(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);project=root/'-root';project.mkdir()
            valid=str(uuid.uuid4());legacy=str(uuid.uuid4())
            (project/(valid+'.jsonl')).write_text(json.dumps({'type':'user','cwd':'/tmp','message':{'role':'user','content':'Keep this session'},'uuid':'u'})+'\n')
            (project/(legacy+'.jsonl')).write_bytes(b'[]\nnull\n{"message":null}\n\xff\n'+json.dumps({'type':'user','cwd':'/tmp','message':{'content':'Legacy title'}}).encode()+b'\n{"partial":')
            store=bridge.Transcripts(root)
            self.assertEqual({row['id'] for row in store.sessions()},{valid,legacy})
            self.assertEqual(store.history(legacy)[0]['text'],'Legacy title')

class FileTransferTests(unittest.TestCase):
    def test_real_bytes_are_streamed_with_a_checksum_and_no_cli_prompt(self):
        with tempfile.TemporaryDirectory() as folder:
            path=Path(folder)/'mod with spaces.jar';body=bytes(range(256))*1600;path.write_bytes(body)
            events=[];transfer=bridge.FileTransfers(events.append);ident=str(uuid.uuid4())
            transfer.info(ident,str(path),folder)
            self.assertEqual(events[0]['type'],'artifact_info');self.assertEqual(events[0]['size'],len(body))
            self.assertEqual(len(events),1)
            transfer.stream(ident)
            chunks=[e for e in events if e['type']=='desktop_chunk']
            self.assertEqual([e['sequence'] for e in chunks],list(range(len(chunks))))
            self.assertEqual(b''.join(base64.b64decode(e['data']) for e in chunks),body)
            import hashlib
            self.assertEqual(events[-1]['sha256'],hashlib.sha256(body).hexdigest());self.assertFalse(transfer.items)
    def test_missing_file_returns_an_explicit_error(self):
        with tempfile.TemporaryDirectory() as folder:
            events=[];transfer=bridge.FileTransfers(events.append)
            transfer.info(str(uuid.uuid4()),str(Path(folder)/'absent.jar'),folder)
            self.assertIn('не найден',events[0]['error']);self.assertFalse(transfer.items)
    def test_bare_filename_uses_unique_build_output_and_cancellation_closes_it(self):
        with tempfile.TemporaryDirectory() as folder:
            output=Path(folder)/'build/libs';output.mkdir(parents=True);(output/'mod.jar').write_bytes(b'jar')
            events=[];transfer=bridge.FileTransfers(events.append);ident=str(uuid.uuid4())
            transfer.info(ident,'mod.jar',folder);self.assertEqual(events[0]['path'],str(output/'mod.jar'))
            source=transfer.items[ident]['file'];transfer.cancel(ident);self.assertTrue(source.closed);self.assertFalse(transfer.items)
    def test_directory_is_not_offered_as_a_download(self):
        with tempfile.TemporaryDirectory() as folder:
            events=[];transfer=bridge.FileTransfers(events.append)
            transfer.info(str(uuid.uuid4()),folder,folder);self.assertIn('error',events[0]);self.assertFalse(transfer.items)


class HostTests(unittest.TestCase):
    """Claude keeps working when the app disconnects; the next app sees it running."""
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name);cli=self.root/'claude'
        cli.write_text(FAKE);cli.chmod(0o700)
        transcripts=bridge.Transcripts(self.root/'transcripts')
        self.host=bridge.Host(lambda send:bridge.Bridge(send=send,transcripts=transcripts,binary=str(cli)))
        self.host.bridge.catalog={'type':'catalog','models':[],'commands':[]}
        self.path=str(self.root/'host.sock')
        threading.Thread(target=self.host.serve,args=(self.path,),daemon=True).start()
        end=time.monotonic()+5
        while not os.path.exists(self.path) and time.monotonic()<end:time.sleep(.02)
        self.addCleanup(self.host.bridge.close)
        self.sid=str(uuid.uuid4())

    def app(self):
        conn=socket.socket(socket.AF_UNIX);conn.connect(self.path);conn.settimeout(5)
        return conn,conn.makefile('rb')

    def next(self,stream,kind):
        while True:
            event=json.loads(stream.readline())
            if event.get('type')==kind:return event

    def test_run_survives_the_app_and_is_replayed(self):
        conn,stream=self.app()
        self.assertEqual(self.next(stream,'hello')['protocol'],4)
        self.assertTrue(stat.S_IMODE(os.stat(self.path).st_mode)&0o077==0)
        conn.sendall((json.dumps({'op':'prompt','sid':self.sid,'cwd':str(self.root),'text':'test','mode':'manual'})+'\n').encode())
        permission=self.next(stream,'permission')
        conn.close()
        time.sleep(.3)
        run=self.host.bridge.runs[self.sid]
        self.assertTrue(run.busy);self.assertIsNone(run.process.poll())
        other,events=self.app()
        self.assertTrue(self.next(events,'run_state')['busy'])
        self.assertEqual(self.next(events,'permission')['id'],permission['id'])
        other.sendall((json.dumps({'op':'sessions'})+'\n').encode())
        other.sendall((json.dumps({'op':'permission','sid':self.sid,'id':permission['id'],'allow':True})+'\n').encode())
        self.assertEqual(self.next(events,'claude')['data']['message']['content'][0]['text'],'allow')
        self.assertFalse(self.next(events,'run_state')['busy'])
        other.close()

    def test_pc_tools_go_only_to_the_app_that_asked(self):
        first,a=self.app();second,b=self.app()
        self.next(a,'hello');self.next(b,'hello')
        first.sendall((json.dumps({'op':'prompt','sid':self.sid,'cwd':str(self.root),'text':'test','mode':'manual'})+'\n').encode())
        self.next(a,'permission');self.next(b,'permission')
        owner=self.host.owners[self.sid]
        seen=[]
        with mock.patch.object(owner,'send',side_effect=seen.append):
            self.host.route({'type':'desktop_request','id':'x','sid':self.sid,'tool':'run_local_command','input':{}})
        self.assertEqual([e['id'] for e in seen],['x'])
        first.close();second.close()

