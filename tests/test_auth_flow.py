import importlib.util
import json
import os
from pathlib import Path
import queue
import tempfile
import time
import unittest
import uuid
from unittest import mock
from types import SimpleNamespace

SPEC=importlib.util.spec_from_file_location('relay_auth_flow',Path(__file__).parents[1]/'bridge/auth_flow.py')
auth=importlib.util.module_from_spec(SPEC);SPEC.loader.exec_module(auth)
BRIDGE_SPEC=importlib.util.spec_from_file_location('relay_login_bridge',Path(__file__).parents[1]/'bridge/relay_bridge.py')
bridge=importlib.util.module_from_spec(BRIDGE_SPEC);BRIDGE_SPEC.loader.exec_module(bridge)
URL='https://claude.com/cai/oauth/authorize?state=fixture&code=true&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback'
FAKE='''#!/usr/bin/env python3
import json,os,sys,time
if sys.argv[1:]==['auth','status']:
 print(json.dumps({'loggedIn':os.environ.get('FAKE_AUTH_STATUS','true')=='true'}));sys.exit(0)
assert sys.argv[1:]==['auth','login']
url=URL
sys.stdout.write('Opening browser…\\n\\x1b]8;;'+url+'\\x07'+url+'\\x1b]8;;\\x07\\nPaste code here if prompted > ');sys.stdout.flush()
code=sys.stdin.readline().strip()
if code!='fixture-code#fixture-state':sys.exit(2)
sys.exit(0)
'''.replace('url=URL','url='+repr(URL))


class AuthFlowTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.binary=Path(self.temp.name)/'claude';self.binary.write_text(FAKE);self.binary.chmod(0o700)
        self.events=queue.Queue();self.manager=auth.AuthManager(str(self.binary),timeout=5)
        self.ident=str(uuid.uuid4());self.owner=object();self.flow=None
        self.addCleanup(self.close)

    def close(self):
        self.manager.cancel(closing=True)
        if self.flow:self.flow['thread'].join(5)

    def start(self):
        self.manager.start(self.ident,self.events.put,self.owner);self.flow=self.manager.current

    def wait(self,state):
        end=time.monotonic()+5
        while time.monotonic()<end:
            event=self.events.get(timeout=5)
            if event['state']==state:return event
            if event['state']=='failed' and state!='failed':self.fail(event['message'])
        self.fail('No auth state '+state)

    def test_terminal_links_are_exact_with_bel_and_st_terminators(self):
        for end in ['\x07','\x1b\\']:
            self.assertEqual(auth.login_url('\x1b]8;;'+URL+end+URL+'\x1b]8;;'+end),URL)
        self.assertEqual(auth.login_url('\x1b[32m'+URL+'\x1b[0m'),URL)

    def test_only_official_authorize_urls(self):
        for value in ['https://evil.test/oauth/authorize','https://claude.ai.evil.test/oauth/authorize',
                      'https://user@claude.ai/oauth/authorize','https://claude.ai:444/oauth/authorize','http://claude.ai/oauth/authorize','file:///etc/passwd']:
            self.assertIsNone(auth.login_url(value))

    def test_real_cli_protocol_no_shell_code_echo_or_credential_changes(self):
        credentials=Path(self.temp.name)/'.credentials.json';credentials.write_bytes(b'untouched-fixture')
        with mock.patch.dict(os.environ,{'CLAUDE_CONFIG_DIR':self.temp.name}):
            self.start();event=self.wait('browser');self.assertEqual(event['url'],URL)
            self.manager.input(self.ident,'fixture-code#fixture-state',self.owner)
            event=self.wait('success');self.assertNotIn('fixture-code',json.dumps(event))
            self.flow['thread'].join(3)
        self.assertEqual(credentials.read_bytes(),b'untouched-fixture')
        self.assertIsNone(self.manager.current)

    def test_foreign_client_stale_request_and_multiline_input_rejected(self):
        self.start();self.wait('browser')
        for ident,owner,code in [(self.ident,object(),'fixture-code#fixture-state'),(str(uuid.uuid4()),self.owner,'code'),(self.ident,self.owner,'code\nother')]:
            with self.assertRaises(ValueError):self.manager.input(ident,code,owner)
        self.manager.cancel(self.ident,object());self.assertFalse(self.flow['cancel'].is_set())

    def test_cancel_kills_only_this_login_and_reaps_process(self):
        self.start();self.wait('browser');self.manager.cancel(self.ident,self.owner);self.wait('cancelled')
        self.flow['thread'].join(4);self.assertIsNotNone(self.flow['process'].poll())
        self.assertIsNone(self.manager.current)

    def test_second_login_cannot_replace_an_existing_login(self):
        self.start();self.wait('browser')
        with self.assertRaises(ValueError):self.manager.start(str(uuid.uuid4()),lambda x:None,object())
        self.assertIs(self.manager.current,self.flow)

    def test_timeout_is_not_success_and_leaves_no_process(self):
        self.manager.timeout=.2;self.start();self.wait('failed');self.flow['thread'].join(4)
        self.assertIsNotNone(self.flow['process'].poll())

    def test_status_must_confirm_login_even_after_exit_zero(self):
        with mock.patch.dict(os.environ,{'FAKE_AUTH_STATUS':'false'}):
            self.start();self.wait('browser');self.manager.input(self.ident,'fixture-code#fixture-state',self.owner)
            self.wait('failed');self.flow['thread'].join(3)

    def test_disconnecting_other_client_does_not_cancel_login(self):
        self.start();self.wait('browser');self.manager.cancel_owner(object());self.assertFalse(self.flow['cancel'].is_set())
        self.manager.cancel_owner(self.owner);self.wait('cancelled')

    def rpc(self):
        service=bridge.Bridge.__new__(bridge.Bridge);service.auth=self.manager;service.send=self.events.put;service.runs={}
        return service

    def test_actual_bridge_auth_operations_complete_the_login(self):
        service=self.rpc()
        with mock.patch.object(bridge,'CLIENT',SimpleNamespace()):
            service.dispatch({'op':'auth_start','id':self.ident});self.flow=self.manager.current
            self.assertEqual(self.wait('browser')['url'],URL)
            service.dispatch({'op':'auth_input','id':self.ident,'code':'fixture-code#fixture-state'})
            self.wait('success');self.flow['thread'].join(3)

    def test_invalid_code_reports_input_error_and_keeps_login_available(self):
        service=self.rpc()
        with mock.patch.object(bridge,'CLIENT',SimpleNamespace()):
            service.dispatch({'op':'auth_start','id':self.ident});self.flow=self.manager.current;self.wait('browser')
            service.dispatch({'op':'auth_input','id':self.ident,'code':''})
            event=self.wait('input_error');self.assertEqual(event['id'],self.ident)
            self.assertFalse(self.flow['cancel'].is_set())
            service.dispatch({'op':'auth_input','id':self.ident,'code':'fixture-code#fixture-state'});self.wait('success')
