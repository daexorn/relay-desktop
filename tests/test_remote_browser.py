import importlib.util
from pathlib import Path
import unittest

SPEC=importlib.util.spec_from_file_location('relay_remote_browser',Path(__file__).parents[1]/'bridge/remote_browser.py')
browser=importlib.util.module_from_spec(SPEC);SPEC.loader.exec_module(browser)

class CDP:
    def __init__(self):self.calls=[];self.reply={'currentIndex':0,'entries':[]}
    def fire(self,method,params=None,session=None,**kwargs):self.calls.append((method,params,session))
    def call(self,method,params=None,session=None,**kwargs):
        self.fire(method,params,session)
        return self.reply

class BrowserTests(unittest.TestCase):
    def setUp(self):
        self.events=[];self.browser=browser.RemoteBrowser('fixture',self.events.append)
        self.cdp=CDP();self.browser.cdp=self.cdp;self.browser.session='page'

    def test_backpressure_preserves_latest_static_frame_without_unbounded_queue(self):
        def frame(number):self.browser.frame({'sessionId':number,'data':str(number)},'page')
        frame(101);frame(102);frame(103)
        self.assertEqual(len(self.events),1);self.assertEqual(self.browser.waiting[0]['sessionId'],103)
        self.assertIn(('Page.screencastFrameAck',{'sessionId':102},'page'),self.cdp.calls)
        self.browser.ack(999);self.assertEqual(len(self.events),1)
        self.browser.ack(1);event=self.browser.events.get_nowait();self.browser.frame(event['params'],event['sessionId'])
        self.assertEqual(len(self.events),2);self.assertEqual(self.events[-1]['data'],'103');self.assertEqual(self.events[-1]['frame'],2)
        self.browser.ack(2);self.assertIsNone(self.browser.outstanding)

    def test_frames_from_old_targets_and_oversize_frames_are_never_sent(self):
        self.browser.frame({'sessionId':1,'data':'fixture'},'old-page')
        self.browser.frame({'sessionId':2,'data':'x'*(browser.MAX_FRAME+1)},'page')
        self.assertFalse(self.events);self.assertEqual(len(self.cdp.calls),2)

    def test_unicode_click_wheel_and_selection_use_only_fixed_cdp_methods(self):
        for value in [{'kind':'text','text':'Русский текст'}, {'kind':'down','x':30.,'y':45.,'button':'left'},
                      {'kind':'wheel','x':1,'y':2,'deltaY':80}, {'kind':'key','key':'ArrowLeft','modifiers':8}]:self.browser.input(value)
        self.assertEqual([x[0] for x in self.cdp.calls],['Input.insertText','Input.dispatchMouseEvent','Input.dispatchMouseEvent','Input.dispatchKeyEvent'])
        self.assertEqual(self.cdp.calls[0][1]['text'],'Русский текст');self.assertEqual(self.cdp.calls[-1][1]['modifiers'],8)

    def test_input_cannot_escape_page_coordinates_or_execute_arbitrary_cdp(self):
        for value in [None,{'kind':'Runtime.evaluate','expression':'bad'}, {'kind':'move','x':float('nan'),'y':1},
                      {'kind':'down','x':-1,'y':1}, {'kind':'up','x':0,'y':900}, {'kind':'text','text':'x'*4097},
                      {'kind':'key','key':'Execute'}, {'kind':'key','key':'Enter','modifiers':100}]:
            with self.assertRaises(ValueError):self.browser.input(value)
        self.assertFalse(self.cdp.calls)

    def test_selection_copy_requires_a_human_action_and_is_bounded(self):
        self.cdp.reply={'result':{'value':'Выделенный текст'}}
        self.assertFalse(self.events);self.browser.input({'kind':'copy'})
        self.assertEqual(self.events,[{'type':'auth_clipboard','id':'fixture','text':'Выделенный текст'}])
        self.assertEqual(self.cdp.calls[-1][0],'Runtime.evaluate')
        self.events.clear();self.cdp.reply={'result':{'value':'x'*16385}};self.browser.input({'kind':'copy'});self.assertFalse(self.events)

    def test_address_bar_accepts_web_addresses_and_rejects_other_protocols(self):
        self.assertEqual(browser.RemoteBrowser.address(' example.com/path '),'https://example.com/path')
        self.assertEqual(browser.RemoteBrowser.address('http://localhost:37865/callback'),'http://localhost:37865/callback')
        for value in ['',None,'file:///etc/passwd','javascript:alert(1)','data:text/html,bad',
                      'https://user:pass@example.com','https://example.com:99999','https://example.com/\nsecret']:
            with self.assertRaises(ValueError):browser.RemoteBrowser.address(value)

    def test_back_and_forward_follow_real_history_without_invalid_indexes(self):
        self.cdp.reply={'currentIndex':1,'entries':[{'id':11},{'id':22},{'id':33}]}
        self.browser.input({'kind':'back'});self.browser.input({'kind':'forward'})
        moves=[params['entryId'] for method,params,_ in self.cdp.calls if method=='Page.navigateToHistoryEntry']
        self.assertEqual(moves,[11,33]);self.cdp.calls.clear()
        self.cdp.reply={'currentIndex':0,'entries':[{'id':11}]}
        self.browser.input({'kind':'back'});self.browser.input({'kind':'forward'})
        self.assertEqual([x[0] for x in self.cdp.calls],['Page.getNavigationHistory']*2)

    def test_navigation_and_tabs_are_bound_to_the_standalone_browser(self):
        with self.assertRaises(ValueError):self.browser.input({'kind':'navigate','url':'example.com'})
        self.browser.navigation=True;self.browser.input({'kind':'navigate','url':'example.com'})
        self.assertEqual(self.cdp.calls[-1],('Page.navigate',{'url':'https://example.com'},'page'))
        self.browser.targets['owned']='page';self.browser.target='owned'
        with self.assertRaises(ValueError):self.browser.input({'kind':'switch_tab','target':'foreign'})
        self.browser.input({'kind':'close_tab','target':'owned'})
        self.assertEqual(self.cdp.calls[-1],('Page.navigate',{'url':'about:blank'},'page'))
        self.browser.targets['second']='page2';self.browser.input({'kind':'close_tab','target':'owned'})
        self.assertEqual(self.cdp.calls[-1],('Target.closeTarget',{'targetId':'owned'},None))

    def test_login_does_not_transmit_oauth_urls_or_tabs_as_navigation_metadata(self):
        self.browser.target='page';self.browser.targets['page']='session';self.browser.pages['page']={'url':'https://claude.ai/oauth/authorize?code=fixture'}
        self.browser.navigation_state();self.assertFalse(self.events)
        self.browser.navigation=True;self.browser.navigation_state()
        self.assertEqual(self.events[-1]['tabs'][0]['id'],'page');self.assertEqual(self.events[-1]['type'],'browser_navigation')
