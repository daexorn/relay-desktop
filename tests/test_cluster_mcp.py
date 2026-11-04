import importlib.util
import json
import os
from pathlib import Path
import queue
import socket
import subprocess
import sys
import threading
import unittest
import uuid

SOURCE=Path(__file__).parents[1]/'bridge/cluster_mcp.py'
spec=importlib.util.spec_from_file_location('cluster_mcp',SOURCE)
cluster=importlib.util.module_from_spec(spec);spec.loader.exec_module(cluster);cluster.SOURCE=SOURCE.read_text()

class ClusterMcpTests(unittest.TestCase):
    def setUp(self):
        self.events=queue.Queue();self.service=cluster.RelayCluster(self.events.put)
        self.sid=str(uuid.uuid4());self.run=str(uuid.uuid4())
        self.config=self.service.config(self.sid,self.run)
        self.token=self.config['env']['RELAY_CLUSTER_TOKEN']
        self.addCleanup(self.service.close)

    def call(self,tool,args=None,token=None):
        result=queue.Queue()
        def invoke():
            with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as client:
                client.settimeout(3);client.connect(self.service.path)
                client.sendall((json.dumps({'token':token or self.token,'tool':tool,'input':args or {}})+'\n').encode())
                with client.makefile('rb') as f:result.put(json.loads(f.readline()))
        thread=threading.Thread(target=invoke,daemon=True);thread.start()
        return result

    def test_stdio_lists_actual_leader_tools(self):
        data=''.join(json.dumps(r)+'\n' for r in [{'id':1,'method':'initialize','params':{'protocolVersion':'2025-06-18'}},{'method':'notifications/initialized'},{'id':2,'method':'tools/list'}])
        out=subprocess.run([sys.executable,str(SOURCE)],input=data,text=True,capture_output=True,check=True,timeout=3)
        frames=[json.loads(s) for s in out.stdout.splitlines()]
        self.assertEqual(frames[0]['result']['protocolVersion'],'2025-06-18')
        self.assertEqual({t['name'] for t in frames[1]['result']['tools']},cluster.NAMES)

    def test_dispatch_and_result_travel_over_the_existing_connection_with_bound_scope(self):
        result=self.call('dispatch_task',{'worker':'ui','task':'Implement the UI'})
        event=self.events.get(timeout=2)
        self.assertEqual((event['sid'],event['run']),(self.sid,self.run))
        self.assertEqual(event['type'],'cluster_request')
        self.service.result(event['id'],'wrong-session',self.run,{'ok':True,'job':'wrong'})
        self.assertFalse(self.service.pending[event['id']]['event'].is_set())
        self.service.result(event['id'],self.sid,self.run,{'ok':True,'job':'real-job','state':'queued'})
        self.assertEqual(result.get(timeout=2)['job'],'real-job')

    def test_unknown_tokens_and_tools_never_reach_the_coordinator(self):
        self.assertFalse(self.call('dispatch_task',{'worker':'a','task':'b'},'invalid').get(timeout=2)['ok'])
        self.assertFalse(self.call('shell_command',{}).get(timeout=2)['ok'])
        self.assertTrue(self.events.empty())

    def test_stopping_a_turn_cancels_waits_but_preserves_the_session_tools(self):
        result=self.call('wait_for_task',{'job':'job'})
        self.events.get(timeout=2);self.service.cancel(self.sid)
        self.assertFalse(result.get(timeout=2)['ok'])
        self.assertIn(self.token,self.service.tokens)
        self.service.cancel(self.sid,revoke=True)
        self.assertNotIn(self.token,self.service.tokens)

    def test_rotating_a_session_does_not_revoke_another_leader(self):
        other=self.service.config('other-session','other-run')['env']['RELAY_CLUSTER_TOKEN']
        self.service.config(self.sid,self.run)
        self.assertNotIn(self.token,self.service.tokens);self.assertIn(other,self.service.tokens)

    def test_stdio_wait_returns_the_workers_real_result(self):
        def respond():
            event=self.events.get(timeout=3)
            self.service.result(event['id'],event['sid'],event['run'],{'ok':True,'job':'job-1','state':'done','finished':True,'result':'Changed files, checks passed'})
        thread=threading.Thread(target=respond,daemon=True);thread.start()
        data=json.dumps({'jsonrpc':'2.0','id':3,'method':'tools/call','params':{'name':'wait_for_task','arguments':{'job':'job-1'}}})+'\n'
        out=subprocess.run([sys.executable,str(SOURCE)],input=data,text=True,capture_output=True,check=True,env=dict(os.environ,**self.config['env']),timeout=5)
        frame=json.loads(out.stdout);body=json.loads(frame['result']['content'][0]['text'])
        self.assertEqual(body['result'],'Changed files, checks passed');self.assertFalse(frame['result']['isError'])
