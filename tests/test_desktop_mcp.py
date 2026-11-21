import base64
import hashlib
import importlib.util
import json
import queue
import socket
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path
from unittest import mock

SOURCE=Path(__file__).parents[1]/'bridge/desktop_mcp.py'
spec=importlib.util.spec_from_file_location('desktop_mcp',SOURCE)
mcp=importlib.util.module_from_spec(spec);spec.loader.exec_module(mcp);mcp.SOURCE=SOURCE.read_text()

class DesktopTests(unittest.TestCase):
    def setUp(self):
        self.events = queue.Queue()
        self.service = mcp.RelayDesktop(self.events.put)
        self.config = json.loads(self.service.config("test-session", tuple(t["name"] for t in mcp.TOOLS)))
        self.token = self.config["mcpServers"]["relay_desktop"]["env"]["RELAY_DESKTOP_TOKEN"]
    def tearDown(self):self.service.close()
    def call(self,tool,args,token=None):
        result=queue.Queue()
        def worker():
            with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as client:
                client.settimeout(3);client.connect(self.service.path)
                client.sendall((json.dumps({'token':token or self.token,'tool':tool,'input':args})+'\n').encode())
                with client.makefile('rb') as f:result.put(json.loads(f.readline()))
        threading.Thread(target=worker,daemon=True).start();return result
    def test_stdio_initialize_and_tools_list(self):
        data=''.join(json.dumps(r)+'\n' for r in [{'jsonrpc':'2.0','id':1,'method':'initialize','params':{'protocolVersion':'2025-06-18'}},{'jsonrpc':'2.0','method':'notifications/initialized'},{'jsonrpc':'2.0','id':2,'method':'tools/list'}])
        output=subprocess.run([sys.executable,str(SOURCE)],input=data,text=True,capture_output=True,timeout=3,check=True)
        frames=[json.loads(line) for line in output.stdout.splitlines()];self.assertEqual(len(frames),2)
        self.assertEqual(frames[0]['result']['protocolVersion'],'2025-06-18');self.assertEqual({t['name'] for t in frames[1]['result']['tools']},{'download_file','run_local_command'})
    def test_unknown_token_cannot_request_local_execution(self):
        result=self.call('run_local_command',{'command':'echo example'},token='invalid').get(timeout=3)
        self.assertFalse(result['ok']);self.assertTrue(self.events.empty())
    def test_command_waits_for_desktop_confirmation_and_returns_denial(self):
        result=self.call('run_local_command',{'command':'echo example'})
        event=self.events.get(timeout=2);self.assertEqual(event['tool'],'run_local_command');self.assertTrue(result.empty())
        self.service.result(event['id'],{'ok':False,'message':'Denied'})
        self.assertEqual(result.get(timeout=2)['message'],'Denied')
    def test_download_requires_approval_then_streams_bytes_and_checksum(self):
        with tempfile.TemporaryDirectory() as folder:
            path=Path(folder)/'example.jar';body=bytes(range(256))*1800;path.write_bytes(body)
            result=self.call('download_file',{'server_path':str(path)})
            request=self.events.get(timeout=2);self.assertEqual(request['input']['size'],len(body));self.assertTrue(self.events.empty())
            self.service.download(request['id']);chunks=[];sequence=0
            while True:
                event=self.events.get(timeout=2)
                if event['type']=='desktop_end':break
                self.assertEqual(event['sequence'],sequence);sequence+=1;chunks.append(base64.b64decode(event['data']))
            self.assertEqual(b''.join(chunks),body);self.assertEqual(event['sha256'],hashlib.sha256(body).hexdigest())
            self.service.result(request['id'],{'ok':True});self.assertTrue(result.get(timeout=2)['ok'])
    def test_tools_list_follows_session_tools(self):
        data=json.dumps({'jsonrpc':'2.0','id':1,'method':'tools/list'})+'\n'
        env=dict(__import__('os').environ,RELAY_DESKTOP_TOOLS='minecraft_test')
        output=subprocess.run([sys.executable,str(SOURCE)],input=data,text=True,capture_output=True,timeout=3,check=True,env=env)
        self.assertEqual([t['name'] for t in json.loads(output.stdout)['result']['tools']],['minecraft_test'])
    def test_minecraft_test_streams_mods_by_index_and_collects_uploads(self):
        with tempfile.TemporaryDirectory() as folder,tempfile.TemporaryDirectory() as home:
            mcp.TEST_ROOT=Path(home)
            first=Path(folder)/'mod.jar';first.write_bytes(b'one');second=Path(folder)/'fabric-api.jar';second.write_bytes(b'two'*1000)
            result=self.call('minecraft_test',{'purpose':'GUI opens','scenario':'test_world\nquit','mods':[str(first),str(second)]})
            request=self.events.get(timeout=2);self.assertEqual([m['name'] for m in request['input']['mods']],['mod.jar','fabric-api.jar'])
            self.assertEqual(request['input']['minecraft_version'],'1.21.8');self.assertEqual(request['input']['timeout_seconds'],300)
            self.service.download(request['id'],1);chunks=[]
            while True:
                event=self.events.get(timeout=2)
                if event['type']=='desktop_end':break
                chunks.append(base64.b64decode(event['data']))
            self.assertEqual(b''.join(chunks),b'two'*1000)
            self.service.download(request['id'],1);self.assertTrue(self.events.empty())
            png=b'\x89PNG'+bytes(300000)
            for i in range(0,len(png),131072):self.service.upload(request['id'],'01-title.png',i//131072,base64.b64encode(png[i:i+131072]).decode())
            self.service.upload_end(request['id'],'01-title.png')
            self.service.upload(request['id'],'../escape.png',0,base64.b64encode(b'x').decode())
            self.service.result(request['id'],{'ok':False,'report':{'ok':False}})
            body=result.get(timeout=2);saved=Path(body['files'][0])
            self.assertEqual(saved.name,'01-title.png');self.assertEqual(saved.read_bytes(),png);self.assertEqual(len(body['files']),1)
            self.assertFalse((Path(home)/'escape.png').exists())
    def test_minecraft_test_rejects_non_jar_mods(self):
        with tempfile.TemporaryDirectory() as folder:
            path=Path(folder)/'notes.txt';path.write_text('x')
            result=self.call('minecraft_test',{'purpose':'x','scenario':'quit','mods':[str(path)]}).get(timeout=3)
            self.assertFalse(result['ok']);self.assertIn('Not a mod jar',result['message']);self.assertTrue(self.events.empty())
    def test_download_destination_is_forwarded_to_desktop(self):
        with tempfile.TemporaryDirectory() as folder:
            path=Path(folder)/'example.jar';path.write_bytes(b'jar')
            destination='C:/Users/Player/Minecraft/mods/example.jar'
            result=self.call('download_file',{'server_path':str(path),'destination':destination})
            event=self.events.get(timeout=2);self.assertEqual(event['input']['destination'],destination)
            self.service.result(event['id'],{'ok':False,'message':'test finished'});self.assertFalse(result.get(timeout=2)['ok'])
    def test_cancel_resolves_waiting_tool(self):
        result=self.call('run_local_command',{'command':'echo example'});self.events.get(timeout=2);self.service.cancel('test-session');self.assertFalse(result.get(timeout=2)['ok'])
    def test_stop_keeps_the_connection_for_the_next_action(self):
        result=self.call('run_local_command',{'command':'first fixture'})
        self.events.get(timeout=2);self.service.cancel('test-session')
        self.assertFalse(result.get(timeout=2)['ok']);self.events.get(timeout=2)
        next_result=self.call('run_local_command',{'command':'second fixture'})
        event=self.events.get(timeout=2);self.assertEqual(event['type'],'desktop_request')
        self.service.result(event['id'],{'ok':True,'message':'fixture response'})
        self.assertTrue(next_result.get(timeout=2)['ok'])
    def test_revoke_cancels_waiter_and_rejects_old_token(self):
        result=self.call('run_local_command',{'command':'fixture only'})
        self.events.get(timeout=2);self.service.cancel('test-session',revoke=True)
        self.assertFalse(result.get(timeout=2)['ok']);self.events.get(timeout=2)
        rejected=self.call('run_local_command',{'command':'must never be forwarded'}).get(timeout=2)
        self.assertFalse(rejected['ok']);self.assertIn('expired',rejected['message']);self.assertTrue(self.events.empty())
    def test_revoking_one_session_preserves_the_other_connection(self):
        other=json.loads(self.service.config('other-session'))['mcpServers']['relay_desktop']['env']['RELAY_DESKTOP_TOKEN']
        self.service.cancel('test-session',revoke=True)
        result=self.call('run_local_command',{'command':'other fixture'},token=other)
        event=self.events.get(timeout=2);self.assertEqual(event['sid'],'other-session')
        self.service.result(event['id'],{'ok':True});self.assertTrue(result.get(timeout=2)['ok'])
    def test_revoke_all_rejects_both_sessions(self):
        other=json.loads(self.service.config('other-session'))['mcpServers']['relay_desktop']['env']['RELAY_DESKTOP_TOKEN']
        self.service.cancel(revoke=True)
        for token in (self.token,other):
            result=self.call('run_local_command',{'command':'fixture only'},token=token).get(timeout=2)
            self.assertFalse(result['ok']);self.assertIn('expired',result['message'])
        self.assertTrue(self.events.empty())
    def test_revoke_during_validation_does_not_forward_a_late_request(self):
        entered=threading.Event();release=threading.Event()
        def validate(args):
            entered.set();release.wait(2);return args,1
        with mock.patch.object(self.service,'test_input',side_effect=validate):
            result=self.call('minecraft_test',{'scenario':'fixture only'})
            self.assertTrue(entered.wait(2))
            try:self.service.cancel('test-session',revoke=True)
            finally:release.set()
            rejected=result.get(timeout=2)
        self.assertFalse(rejected['ok']);self.assertIn('expired',rejected['message']);self.assertTrue(self.events.empty())

if __name__=='__main__':unittest.main()
