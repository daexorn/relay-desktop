"""A server Chromium, controlled by a human over the existing private SSH connection."""
import base64
import collections
import errno
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import pwd
import queue
import select
import shutil
import signal
import struct
import subprocess
import tempfile
import threading
import time
import urllib.parse
import socket

WIDTH, HEIGHT = 1200, 800
MAX_FRAME = 2 * 1024 * 1024
SETUP_LOCK=threading.Lock()


def browser_info():
    binary = next((shutil.which(x) for x in ('chromium', 'chromium-browser', 'google-chrome') if shutil.which(x)), None)
    missing = []
    if not binary:missing.append('Chromium')
    if not shutil.which('Xvfb'):missing.append('Xvfb')
    if os.geteuid() == 0:
        try:
            user = pwd.getpwnam('relay-browser')
            if not user.pw_uid or user.pw_dir != '/var/lib/relay-browser':missing.append('пользователь relay-browser')
        except KeyError:missing.append('пользователь relay-browser')
        if not shutil.which('setpriv'):missing.append('setpriv')
    return {'ready':not missing, 'missing':missing, 'can_install':os.geteuid() == 0 and bool(shutil.which('apt-get')), 'browser':binary}


def install_browser(send,ident):
    if not browser_info()['can_install']:raise ValueError('Подготовить браузер может администратор сервера Debian/Ubuntu.')
    if not SETUP_LOCK.acquire(blocking=False):raise ValueError('Браузер сервера уже подготавливается.')
    try:
        send({'type':'browser_setup','id':ident,'running':True,'message':'Устанавливаем Chromium и отдельный экран на сервере…'})
        env={**os.environ,'DEBIAN_FRONTEND':'noninteractive','NEEDRESTART_MODE':'l'}
        for command in (['apt-get','update'],['apt-get','install','-y','--no-install-recommends','chromium','chromium-sandbox','xvfb','xauth','fonts-liberation']):
            subprocess.run(command,env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=600,check=True)
        try:user=pwd.getpwnam('relay-browser')
        except KeyError:
            subprocess.run(['useradd','--system','--create-home','--home-dir','/var/lib/relay-browser','--shell','/usr/sbin/nologin','relay-browser'],check=True)
            user=pwd.getpwnam('relay-browser')
        if not user.pw_uid or user.pw_dir!='/var/lib/relay-browser':raise ValueError('Пользователь relay-browser уже существует с другими настройками.')
        home=Path(user.pw_dir);home.mkdir(mode=0o700,exist_ok=True);os.chown(home,user.pw_uid,user.pw_gid);home.chmod(0o700)
        send({'type':'browser_setup','id':ident,'running':False,'ok':True,'info':browser_info(),'message':'Браузер сервера готов.'})
    except (OSError,subprocess.SubprocessError,ValueError):
        send({'type':'browser_setup','id':ident,'running':False,'ok':False,'info':browser_info(),'message':'Не удалось подготовить браузер. Проверь доступ к пакетам и права администратора.'})
    finally:SETUP_LOCK.release()


def probe_browser(send,ident):
    """Fixed, account-free diagnostic on a separate browser profile; no arbitrary JS RPC."""
    home=Path(pwd.getpwnam('relay-browser').pw_dir) if os.geteuid()==0 else Path.home()
    profile=home/'profiles'/('relay-check-'+hashlib.sha256(str(ident).encode()).hexdigest())
    frames=[];browser=None
    def receive(event):
        if event.get('type')=='auth_frame':frames[:]=[event];browser.ack(event['frame'])
    browser=RemoteBrowser(ident,receive,profile=profile,navigation=True)
    try:
        html="<!doctype html><meta charset=utf-8><style>body{font:24px sans-serif;background:#171a24;color:#e2e6f3;padding:40px}input,button{font:24px sans-serif;margin:10px;padding:10px}</style><h1>Relay — браузер на сервере</h1><p>"+socket.gethostname()+"</p><input id=name><button id=go onclick=\"document.querySelector('#result').textContent='OK '+document.querySelector('#name').value\">Проверить</button><p id=result>Ожидаю клика</p>"
        browser.open('data:text/html;charset=utf-8,'+urllib.parse.quote(html))
        def evaluate(expression):return browser.cdp.call('Runtime.evaluate',{'expression':expression,'returnByValue':True},browser.session)['result'].get('value')
        end=time.monotonic()+15
        while time.monotonic()<end and evaluate("!!document.querySelector('#name')") is not True:time.sleep(.1)
        def point(selector):return json.loads(evaluate("JSON.stringify((()=>{let r=document.querySelector('#"+selector+"').getBoundingClientRect();return{x:r.x+20,y:r.y+20}})())"))
        target=point('name')
        for kind in ('down','up'):browser.input({'kind':kind,'button':'left',**target})
        browser.input({'kind':'text','text':'СЕРВЕР 123'})
        for _ in range(3):
            for pressed in (True,False):browser.input({'kind':'key','key':'ArrowLeft','modifiers':8,'pressed':pressed})
        browser.input({'kind':'text','text':'456'})
        target=point('go')
        for kind in ('down','up'):browser.input({'kind':kind,'button':'left',**target})
        end=time.monotonic()+10;result=None
        while time.monotonic()<end:
            result=evaluate("document.querySelector('#result').textContent")
            if result=='OK СЕРВЕР 456' and frames:break
            time.sleep(.1)
        if result!='OK СЕРВЕР 456' or not frames:raise ValueError('Не прошла проверка кликов/выделения/Unicode браузера.')
        time.sleep(.3);send(frames[-1])
        report={'type':'browser_probe','id':ident,'ok':True,'server':socket.gethostname(),'browser_uid':os.stat('/proc/'+str(browser.cdp.pid)).st_uid,'result':result,'private_pipe':True}
        def wait_value(expression,expected):
            end=time.monotonic()+10
            while time.monotonic()<end:
                if evaluate(expression)==expected:return
                time.sleep(.1)
            raise ValueError('Не прошла проверка навигации браузера.')
        browser.navigate('data:text/html,<title>Relay navigation check</title><p>Second page</p>')
        wait_value('document.title','Relay navigation check')
        browser.input({'kind':'back'});wait_value("!!document.querySelector('#name')",True)
        browser.input({'kind':'forward'});wait_value('document.title','Relay navigation check')
        first=browser.target;browser.input({'kind':'new_tab'});second=browser.target
        if first==second or len(browser.targets)!=2:raise ValueError('Не открылась новая вкладка.')
        browser.input({'kind':'switch_tab','target':first});wait_value('document.title','Relay navigation check')
        browser.input({'kind':'close_tab','target':second});end=time.monotonic()+10
        while len(browser.targets)>1 and time.monotonic()<end:time.sleep(.1)
        if len(browser.targets)!=1:raise ValueError('Не закрылась вкладка.')
        report.update(navigation_history=True,tabs=True)
        try:
            browser.input({'kind':'navigate','url':'https://api.ipify.org?format=json'});end=time.monotonic()+15
            while time.monotonic()<end:
                text=evaluate('document.body?.innerText')
                try:
                    import ipaddress
                    report['browser_egress_ip']=str(ipaddress.ip_address(json.loads(text)['ip']));break
                except (ValueError,TypeError,KeyError):time.sleep(.3)
        except (OSError,ValueError,KeyError):pass
        send(report)
    finally:
        browser.close();shutil.rmtree(profile,ignore_errors=True)


class CDP:
    """Chromium's private FD3/FD4 pipe. No TCP listener or browser-debugging port."""
    def __init__(self, argv, env, events):
        self.events=events;self.pending={};self.lock=threading.RLock();self.write_lock=threading.Lock()
        self.counter=0;self.closed=False;self.exited=False
        def reserve(fd):
            result=fcntl.fcntl(fd,fcntl.F_DUPFD_CLOEXEC,10);os.close(fd);return result
        a,b=os.pipe();child_in,self.writer=reserve(a),reserve(b)
        a,b=os.pipe();self.reader,child_out=reserve(a),reserve(b)
        null=reserve(os.open(os.devnull,os.O_RDWR))
        actions=[(os.POSIX_SPAWN_DUP2,null,fd) for fd in (0,1,2)]
        actions += [(os.POSIX_SPAWN_DUP2,child_in,3),(os.POSIX_SPAWN_DUP2,child_out,4)]
        actions += [(os.POSIX_SPAWN_CLOSE,fd) for fd in (child_in,self.writer,self.reader,child_out,null)]
        try:self.pid=os.posix_spawn(argv[0],argv,env,file_actions=actions,setsid=True)
        except Exception:
            os.close(self.writer);os.close(self.reader);raise
        finally:
            for fd in (child_in,child_out,null):os.close(fd)
        threading.Thread(target=self.read,daemon=True).start()

    def fire(self, method, params=None, session=None, reply=None):
        with self.lock:
            if self.closed:raise OSError('Browser closed')
            self.counter+=1;ident=self.counter
            if reply is not None:self.pending[ident]=reply
        message={'id':ident,'method':method,'params':params or {}}
        if session:message['sessionId']=session
        body=json.dumps(message,separators=(',',':'),ensure_ascii=False).encode()+b'\0'
        with self.write_lock:
            while body:
                written=os.write(self.writer,body);body=body[written:]
        return ident

    def call(self, method, params=None, session=None, timeout=15):
        reply=queue.Queue(maxsize=1);ident=self.fire(method,params,session,reply)
        try:
            result=reply.get(timeout=timeout)
            if 'error' in result:raise OSError('Server browser did not accept '+method)
            return result.get('result',{})
        except queue.Empty:raise OSError('Server browser did not answer '+method)
        finally:
            with self.lock:self.pending.pop(ident,None)

    def read(self):
        buffer=bytearray()
        try:
            while not self.closed:
                body=os.read(self.reader,65536)
                if not body:break
                buffer.extend(body)
                if len(buffer)>4*1024*1024:raise ValueError('Browser message exceeded budget')
                while b'\0' in buffer:
                    end=buffer.index(0);raw=bytes(buffer[:end]);del buffer[:end+1]
                    event=json.loads(raw)
                    if 'id' in event:
                        with self.lock:reply=self.pending.get(event['id'])
                        if reply is not None:reply.put_nowait(event)
                    else:
                        try:self.events.put_nowait(event)
                        except queue.Full:
                            if event.get('method')=='Page.screencastFrame':
                                self.fire('Page.screencastFrameAck',{'sessionId':event['params']['sessionId']},event.get('sessionId'))
        except (OSError,ValueError,KeyError,queue.Full):pass
        finally:
            self.closed=True
            with self.lock:
                for reply in self.pending.values():
                    try:reply.put_nowait({'error':True})
                    except queue.Full:pass

    def poll(self):
        if self.exited:return True
        try:
            ident,_=os.waitpid(self.pid,os.WNOHANG);self.exited=bool(ident)
        except ChildProcessError:self.exited=True
        return self.exited

    def close(self):
        if not self.closed:
            try:self.fire('Browser.close')
            except OSError:pass
        end=time.monotonic()+3
        while time.monotonic()<end and not self.poll():time.sleep(.05)
        if not self.poll():
            try:os.killpg(self.pid,signal.SIGTERM)
            except ProcessLookupError:pass
            end=time.monotonic()+2
            while time.monotonic()<end and not self.poll():time.sleep(.05)
        if not self.poll():
            try:os.killpg(self.pid,signal.SIGKILL)
            except ProcessLookupError:pass
            try:os.waitpid(self.pid,0)
            except ChildProcessError:pass
        self.closed=True
        for fd in (self.writer,self.reader):
            try:os.close(fd)
            except OSError:pass


class RemoteBrowser:
    info=staticmethod(browser_info)
    def __init__(self, ident, send, profile=None, navigation=False):
        self.ident=ident;self.send=send;self.cdp=None;self.display=None;self.runtime=None;self.profile_lock=None
        self.lock=threading.RLock();self.events=queue.Queue(maxsize=16);self.stop=threading.Event()
        self.session=None;self.target=None;self.targets=collections.OrderedDict()
        self.sequence=0;self.outstanding=None;self.waiting=None;self.last_frame=0.;self.origin='';self.thread=None
        self.profile_override=profile
        self.navigation=navigation;self.pages={}

    @staticmethod
    def address(value):
        if not isinstance(value,str) or not value.strip() or len(value)>8192 or any(ord(c)<32 or ord(c)==127 for c in value):
            raise ValueError('Введи адрес сайта.')
        value=value.strip()
        if '://' not in value:value='https://'+value
        try:
            parsed=urllib.parse.urlsplit(value)
            if parsed.scheme not in ('http','https') or not parsed.hostname or parsed.username or parsed.password:raise ValueError()
            parsed.port
        except ValueError:raise ValueError('Поддерживаются адреса http:// и https:// без пароля в адресе.')
        return value

    def navigation_state(self):
        if not self.navigation:return
        page=self.pages.get(self.target,{})
        self.send({'type':'browser_navigation','id':self.ident,'url':page.get('url','')[:8192],
                   'tabs':[{'id':ident,'title':self.pages.get(ident,{}).get('title','Новая вкладка')[:120],
                            'active':ident==self.target} for ident in list(self.targets)[:16]]})

    def prepare(self):
        info=browser_info()
        if not info['ready']:raise ValueError('На сервере не подготовлен браузер: '+', '.join(info['missing']))
        uid,gid=os.geteuid(),os.getegid();prefix=[]
        if uid==0:
            user=pwd.getpwnam('relay-browser');uid,gid=user.pw_uid,user.pw_gid;home=Path(user.pw_dir)
            prefix=[shutil.which('setpriv'),'--reuid='+str(uid),'--regid='+str(gid),'--init-groups','--']
        else:home=Path.home()
        config=str(Path(os.environ.get('CLAUDE_CONFIG_DIR') or str(Path.home()/'.claude')).expanduser())
        key=hashlib.sha256(config.encode()).hexdigest()
        profile=Path(self.profile_override) if self.profile_override else home/'profiles'/key if prefix else home/'.local/share/relay-desktop/browser'/key
        for path in (profile.parent,profile):
            path.mkdir(mode=0o700,parents=True,exist_ok=True);path.chmod(0o700)
            if os.geteuid()==0:os.chown(path,uid,gid)
        self.profile_lock=(profile/'.relay.lock').open('a')
        try:fcntl.flock(self.profile_lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
        except BlockingIOError:raise ValueError('Серверный браузер этого аккаунта уже открыт в другом подключении.')
        self.runtime=tempfile.TemporaryDirectory(prefix='relay-browser-',dir=str(home))
        runtime=Path(self.runtime.name);runtime.chmod(0o700)
        if os.geteuid()==0:os.chown(runtime,uid,gid)
        authority=runtime/'Xauthority';cookie=os.urandom(16)
        def authority_record(number):
            fields=(b'',str(number).encode(),b'MIT-MAGIC-COOKIE-1',cookie)
            authority.write_bytes(struct.pack('!H',65535)+b''.join(struct.pack('!H',len(x))+x for x in fields));authority.chmod(0o600)
            if os.geteuid()==0:os.chown(authority,uid,gid)
        authority_record('')
        env={'PATH':'/usr/local/bin:/usr/bin:/bin','HOME':str(home),'LANG':'C.UTF-8','XAUTHORITY':str(authority),'XDG_RUNTIME_DIR':str(runtime)}
        self.display=subprocess.Popen(prefix+[shutil.which('Xvfb'),'-displayfd','1','-screen','0','1280x960x24','-nolisten','tcp','-auth',str(authority)],
                                      stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,env=env,start_new_session=True)
        if not select.select([self.display.stdout],[],[],10)[0]:raise OSError('Не удалось открыть экран на сервере.')
        number=self.display.stdout.readline().strip().decode()
        if not number.isdigit():raise OSError('Не удалось подготовить экран сервера.')
        authority_record(number);env['DISPLAY']=':'+number
        argv=prefix+[info['browser'],'--remote-debugging-pipe','--no-first-run','--no-default-browser-check',
                     '--disable-dev-shm-usage','--window-size=1240,940','--user-data-dir='+str(profile),'about:blank']
        self.cdp=CDP(argv,env,self.events)
        self.cdp.call('Browser.getVersion')

    def open(self, url):
        try:
            self.prepare();self.cdp.call('Target.setDiscoverTargets',{'discover':True})
            target=self.cdp.call('Target.createTarget',{'url':'about:blank'})['targetId']
            self.activate(target);self.cdp.call('Page.navigate',{'url':url},self.session)
            self.navigation_state()
            self.thread=threading.Thread(target=self.run,daemon=True);self.thread.start()
        except Exception:self.close();raise

    def navigate(self,url):self.cdp.call('Page.navigate',{'url':url},self.session)
    def alive(self):return self.cdp is not None and not self.cdp.closed and not self.stop.is_set()

    def activate(self, target):
        if self.session:
            try:self.cdp.fire('Page.stopScreencast',session=self.session)
            except OSError:pass
        self.session=self.targets.get(target) or self.cdp.call('Target.attachToTarget',{'targetId':target,'flatten':True})['sessionId']
        self.target=target;self.targets[target]=self.session
        try:
            window=self.cdp.call('Browser.getWindowForTarget',{'targetId':target})
            self.cdp.call('Browser.setWindowBounds',{'windowId':window['windowId'],'bounds':{'left':0,'top':0,'width':1240,'height':940,'windowState':'normal'}})
        except (OSError,KeyError):pass
        self.cdp.call('Page.enable',session=self.session)
        self.cdp.call('Emulation.setDeviceMetricsOverride',{'width':WIDTH,'height':HEIGHT,'deviceScaleFactor':1,'mobile':False},self.session)
        self.cdp.call('Page.startScreencast',{'format':'jpeg','quality':70,'maxWidth':WIDTH,'maxHeight':HEIGHT,'everyNthFrame':1},self.session)
        self.cdp.fire('Page.bringToFront',session=self.session)

    def run(self):
        while not self.stop.is_set() and self.cdp and not self.cdp.closed:
            try:event=self.events.get(timeout=.2)
            except queue.Empty:continue
            try:
                method=event.get('method');params=event.get('params',{});session=event.get('sessionId')
                if method in ('Page.screencastFrame','Relay.frame'):self.frame(params,session)
                elif method=='Page.frameNavigated' and session==self.session and not params.get('frame',{}).get('parentId'):
                    parsed=urllib.parse.urlsplit(params['frame'].get('url',''));self.origin=parsed.hostname or ''
                    self.send({'type':'auth_browser','id':self.ident,'origin':self.origin,'connected':True})
                    self.pages.setdefault(self.target,{})['url']=params['frame'].get('url','');self.navigation_state()
                elif method=='Target.targetCreated':
                    target=params.get('targetInfo',{})
                    if target.get('type')=='page' and target.get('openerId') in self.targets:
                        if len(self.targets)>=16:self.cdp.fire('Target.closeTarget',{'targetId':target['targetId']})
                        else:self.pages[target['targetId']]=target;self.activate(target['targetId']);self.navigation_state()
                elif method=='Target.targetInfoChanged':
                    target=params.get('targetInfo',{})
                    if target.get('targetId') in self.targets:self.pages[target['targetId']]=target;self.navigation_state()
                elif method=='Target.targetDestroyed':
                    target=params.get('targetId');self.targets.pop(target,None);self.pages.pop(target,None)
                    if target==self.target and self.targets:self.activate(next(reversed(self.targets)))
                    self.navigation_state()
            except (OSError,ValueError,KeyError):continue
        if not self.stop.is_set():self.send({'type':'auth_browser','id':self.ident,'connected':False,'message':'Браузер сервера закрылся.'})

    def frame(self, params, session):
        with self.lock:
            stamp=time.monotonic();body=params.get('data','');token=params['sessionId']
            if session!=self.session or not isinstance(body,str) or len(body)>MAX_FRAME:
                self.cdp.fire('Page.screencastFrameAck',{'sessionId':token},session);return
            if self.outstanding is not None:
                if self.waiting:
                    old,old_session=self.waiting;self.cdp.fire('Page.screencastFrameAck',{'sessionId':old['sessionId']},old_session)
                self.waiting=(params,session);return
            delay=.1-(stamp-self.last_frame)
            if delay>0:time.sleep(delay)
            self.sequence+=1;self.last_frame=time.monotonic()
            self.outstanding=(self.sequence,token,session)
            self.send({'type':'auth_frame','id':self.ident,'frame':self.sequence,'width':WIDTH,'height':HEIGHT,'data':body})

    def ack(self, sequence):
        with self.lock:
            if self.outstanding is None or sequence!=self.outstanding[0]:return
            _,token,session=self.outstanding;self.outstanding=None
            waiting=self.waiting;self.waiting=None
        self.cdp.fire('Page.screencastFrameAck',{'sessionId':token},session)
        if waiting:
            params,source=waiting
            self.events.put({'method':'Relay.frame','params':params,'sessionId':source},timeout=1)

    def input(self, value):
        if not isinstance(value,dict):raise ValueError('Некорректный ввод браузера.')
        kind=value.get('kind');params={}
        if kind in ('move','down','up','wheel'):
            for field,limit in [('x',WIDTH),('y',HEIGHT)]:
                number=value.get(field)
                if not isinstance(number,(int,float)) or isinstance(number,bool) or not math.isfinite(number) or not 0<=number<=limit:raise ValueError('Координаты вне окна браузера.')
                params[field]=number
            button=value.get('button','none')
            if button not in ('none','left','right','middle'):raise ValueError('Неизвестная кнопка мыши.')
            params.update(type={'move':'mouseMoved','down':'mousePressed','up':'mouseReleased','wheel':'mouseWheel'}[kind],button=button)
            if kind=='move':params['buttons']={'none':0,'left':1,'right':2,'middle':4}[button]
            if kind in ('down','up'):params['clickCount']=min(3,max(1,int(value.get('clicks',1))))
            if kind=='wheel':
                for field in ('deltaX','deltaY'):
                    number=value.get(field,0)
                    if not isinstance(number,(int,float)) or not math.isfinite(number):raise ValueError('Некорректная прокрутка.')
                    params[field]=max(-4000,min(4000,number))
            method='Input.dispatchMouseEvent'
        elif kind=='text':
            text=value.get('text')
            if not isinstance(text,str) or not 1<=len(text)<=4096 or '\0' in text:raise ValueError('Слишком длинный текст ввода.')
            method='Input.insertText';params={'text':text}
        elif kind=='key':
            key=value.get('key');allowed={'Enter','Tab','Escape','Backspace','Delete','ArrowLeft','ArrowRight','ArrowUp','ArrowDown','Home','End','PageUp','PageDown','Control','Shift','Alt','Meta','a','c','v','x','z','y'}
            if key not in allowed:raise ValueError('Неизвестная клавиша.')
            modifiers=value.get('modifiers',0)
            if type(modifiers)!=int or not 0<=modifiers<=15:raise ValueError('Некорректные модификаторы.')
            virtual={'Enter':13,'Tab':9,'Escape':27,'Backspace':8,'Delete':46,'ArrowLeft':37,'ArrowUp':38,'ArrowRight':39,'ArrowDown':40,'Home':36,'End':35,'PageUp':33,'PageDown':34}
            params={'type':'keyDown' if value.get('pressed',True) else 'keyUp','key':key,'code':'Key'+key.upper() if len(key)==1 else key,'modifiers':modifiers,'windowsVirtualKeyCode':virtual.get(key,ord(key.upper()) if len(key)==1 else 0)}
            method='Input.dispatchKeyEvent'
        elif kind=='copy':
            # Only an explicit human Copy action; never read a password field or page tokens.
            expression="(()=>{let e=document.activeElement;if(e&&e.type==='password')return '';if(e&&typeof e.selectionStart==='number')return e.value.substring(e.selectionStart,e.selectionEnd);return window.getSelection()?.toString()||''})()"
            result=self.cdp.call('Runtime.evaluate',{'expression':expression,'returnByValue':True},self.session)
            text=result.get('result',{}).get('value')
            if isinstance(text,str) and len(text)<=16384:self.send({'type':'auth_clipboard','id':self.ident,'text':text})
            return
        elif kind=='reload':method='Page.reload'
        elif kind in ('back','forward'):
            history=self.cdp.call('Page.getNavigationHistory',session=self.session)
            index=history.get('currentIndex',0)+(-1 if kind=='back' else 1)
            if not 0<=index<len(history.get('entries',[])):return
            method='Page.navigateToHistoryEntry';params={'entryId':history['entries'][index]['id']}
        elif self.navigation and kind=='navigate':
            self.navigate(self.address(value.get('url')));return
        elif self.navigation and kind=='new_tab':
            if len(self.targets)>=16:raise ValueError('Закрой одну из 16 открытых вкладок.')
            target=self.cdp.call('Target.createTarget',{'url':'about:blank'})['targetId']
            self.activate(target);self.navigation_state();return
        elif self.navigation and kind in ('switch_tab','close_tab'):
            target=value.get('target')
            if target not in self.targets:raise ValueError('Эта вкладка уже закрыта.')
            if kind=='switch_tab':self.activate(target);self.navigation_state()
            elif len(self.targets)>1:self.cdp.fire('Target.closeTarget',{'targetId':target})
            else:self.navigate('about:blank')
            return
        else:raise ValueError('Неизвестное действие браузера.')
        self.cdp.fire(method,params,self.session)

    def close(self):
        if self.stop.is_set():return
        self.stop.set()
        if self.cdp:self.cdp.close()
        if self.thread and self.thread is not threading.current_thread():self.thread.join(2)
        if self.display:
            try:os.killpg(self.display.pid,signal.SIGTERM)
            except ProcessLookupError:pass
            try:self.display.wait(timeout=3)
            except subprocess.TimeoutExpired:self.display.kill();self.display.wait(timeout=2)
            if self.display.stdout:self.display.stdout.close()
        if self.runtime:self.runtime.cleanup()
        if self.profile_lock:self.profile_lock.close()
