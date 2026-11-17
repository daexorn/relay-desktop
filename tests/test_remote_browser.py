import importlib.util
import io
from pathlib import Path
import unittest
from unittest import mock

SPEC=importlib.util.spec_from_file_location('relay_remote_browser',Path(__file__).parents[1]/'bridge/remote_browser.py')
browser=importlib.util.module_from_spec(SPEC);SPEC.loader.exec_module(browser)

class BrowserTests(unittest.TestCase):
    def setUp(self):
        self.events=[];self.browser=browser.RemoteBrowser('fixture',self.events.append)
        self.calls=[];self.browser.xdo=lambda *args:self.calls.append(args) or ''
        self.browser.paste=mock.Mock()

    def test_wrong_or_stale_ack_cannot_release_an_outstanding_frame(self):
        self.browser.outstanding=21;self.browser.ack(20);self.assertEqual(self.browser.outstanding,21)
        self.browser.ack(21);self.assertIsNone(self.browser.outstanding)
        self.browser.ack(21);self.assertIsNone(self.browser.outstanding)

    def test_transport_reader_rejects_truncation(self):
        self.assertEqual(browser.read_exact(io.BytesIO(b'1234'),4),b'1234')
        with self.assertRaises(OSError):browser.read_exact(io.BytesIO(b'12'),4)

    def test_native_launch_needs_no_browser_debugger_or_disabled_sandbox(self):
        self.browser.prepare=lambda:('/usr/bin/chromium',Path('/private/profile'))
        process=mock.Mock();process.poll.return_value=None;self.browser.spawn=mock.Mock(return_value=process)
        with mock.patch.object(browser.threading,'Thread'):
            self.browser.open('https://example.com/')
        argv=self.browser.spawn.call_args.args[0]
        self.assertFalse(any('remote-debugging' in value or 'headless' in value or 'automation' in value or value=='--no-sandbox' for value in argv))
        self.assertTrue(self.events[-1]['native']);self.assertNotIn('url',self.events[-1])

    def test_unicode_click_and_shift_selection_are_real_keyboard_and_pointer_input(self):
        self.browser.input({'kind':'text','text':'Русский текст'});self.browser.paste.assert_called_once_with('Русский текст')
        self.browser.input({'kind':'down','x':30.,'y':45.,'button':'left'})
        self.browser.input({'kind':'key','key':'ArrowLeft','modifiers':8})
        self.assertIn(('mousedown','1'),self.calls);self.assertIn(('keydown','Shift_L'),self.calls);self.assertIn(('keydown','Left'),self.calls)
        self.browser.input({'kind':'key','key':'ArrowLeft','modifiers':0,'pressed':False})
        self.assertIn(('keyup','Shift_L'),self.calls)
        self.browser.input({'kind':'modifiers','modifiers':2});self.browser.input({'kind':'modifiers','modifiers':0})
        self.assertIn(('keydown','Control_L'),self.calls);self.assertIn(('keyup','Control_L'),self.calls)

    def test_bad_inputs_cannot_become_commands_or_escape_the_private_screen(self):
        for value in [None,{'kind':'Runtime.evaluate'}, {'kind':'move','x':float('nan'),'y':1},
                      {'kind':'down','x':-1,'y':1},{'kind':'up','x':0,'y':browser.HEIGHT+1},
                      {'kind':'text','text':'x'*4097},{'kind':'key','key':'ctrl+alt+F1'},
                      {'kind':'key','key':'Enter','modifiers':100}]:
            with self.assertRaises(ValueError):self.browser.input(value)
        self.assertFalse(self.calls);self.browser.paste.assert_not_called()

    def test_address_bar_accepts_web_addresses_and_rejects_other_protocols(self):
        self.assertEqual(browser.RemoteBrowser.address(' example.com/path '),'https://example.com/path')
        self.assertEqual(browser.RemoteBrowser.address('http://localhost:37865/callback'),'http://localhost:37865/callback')
        for value in ['',None,'file:///etc/passwd','javascript:alert(1)','data:text/html,bad',
                      'https://user:pass@example.com','https://example.com:99999','https://example.com/\nsecret']:
            with self.assertRaises(ValueError):browser.RemoteBrowser.address(value)

    def test_human_navigation_and_native_tab_shortcuts(self):
        with self.assertRaises(ValueError):self.browser.input({'kind':'navigate','url':'example.com'})
        self.browser.navigation=True;self.browser.input({'kind':'navigate','url':'example.com'})
        self.assertIn(('key','--clearmodifiers','ctrl+l'),self.calls)
        self.browser.paste.assert_called_with('https://example.com')
        for kind in ('back','forward','reload','new_tab'):self.browser.input({'kind':kind})
        self.browser.input({'kind':'switch_tab','index':0});self.browser.input({'kind':'close_tab','index':1})
        for combo in ('alt+Left','alt+Right','ctrl+r','ctrl+t','ctrl+1','ctrl+2','ctrl+w'):
            self.assertIn(('key','--clearmodifiers',combo),self.calls)
        with self.assertRaises(ValueError):self.browser.input({'kind':'switch_tab','index':9})

    def test_copy_is_explicit_and_clears_the_old_clipboard_before_password_fields_refuse_copy(self):
        self.browser.selection=mock.Mock(return_value='Выделено');self.assertFalse(self.events)
        self.browser.input({'kind':'copy'});self.assertEqual(self.events,[{'type':'auth_clipboard','id':'fixture','text':'Выделено'}])
        calls=[];self.browser.selection=browser.RemoteBrowser.selection.__get__(self.browser)
        self.browser.set_clipboard=lambda text:calls.append(('clipboard',text))
        self.browser.shortcut=lambda key:calls.append(('key',key))
        self.browser.spawn=mock.Mock(side_effect=OSError('fixture'))
        with self.assertRaises(OSError):self.browser.selection()
        self.assertEqual(calls,[('clipboard',''),('key','ctrl+c')])
