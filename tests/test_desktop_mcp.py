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

SOURCE=Path(__file__).parents[1]/'bridge/desktop_mcp.py'
spec=importlib.util.spec_from_file_location('desktop_mcp',SOURCE)
mcp=importlib.util.module_from_spec(spec);spec.loader.exec_module(mcp);mcp.SOURCE=SOURCE.read_text()

class DesktopTests(unittest.TestCase):
    def setUp(self):self.events=queue.Queue();self.service=mcp.RelayDesktop(self.events.put);self.config=json.loads(self.service.config('test-session'));self.token=self.config['mcpServers']['relay_desktop']['env']['RELAY_DESKTOP_TOKEN']
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
    def test_cancel_resolves_waiting_tool(self):
        result=self.call('run_local_command',{'command':'echo example'});self.events.get(timeout=2);self.service.cancel('test-session');self.assertFalse(result.get(timeout=2)['ok'])

if __name__=='__main__':unittest.main()
