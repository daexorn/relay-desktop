"""A normal server Chromium displayed to a human over the existing SSH connection."""
import base64
import fcntl
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import pwd
import select
import shutil
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse

WIDTH, HEIGHT = 1280, 960
MAX_FRAME = 2 * 1024 * 1024
SETUP_LOCK = threading.Lock()

def browser_info():
    binary = next((shutil.which(x) for x in ('chromium', 'chromium-browser', 'google-chrome') if shutil.which(x)), None)
    missing = []
    if not binary:missing.append('Chromium')
    for name in ('Xvfb','openbox','xdotool','xclip'):
        if not shutil.which(name):missing.append(name)
    if not importlib.util.find_spec('PIL'):missing.append('python3-pil')
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
        for command in (['apt-get','update'],['apt-get','install','-y','--no-install-recommends','chromium','chromium-sandbox','xvfb','xauth','fonts-liberation','openbox','xdotool','xclip','python3-pil']):
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


# A separate unprivileged process reads only its private X11 screen. It captures a
# frame on demand, so there is no pipe backlog or capture while a frame awaits ACK.
CAPTURE_SOURCE = '''import io,os,struct,sys
from PIL import ImageGrab
while sys.stdin.buffer.read(1):
 image=ImageGrab.grab(xdisplay=os.environ['DISPLAY']).convert('RGB')
 output=io.BytesIO();image.save(output,format='JPEG',quality=72)
 body=output.getvalue();sys.stdout.buffer.write(struct.pack('!I',len(body))+body);sys.stdout.buffer.flush()
'''


def stop_process(process):
    if process is None:return
    if process.poll() is None:
        try:os.killpg(process.pid,signal.SIGTERM)
        except ProcessLookupError:pass
        try:process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            try:os.killpg(process.pid,signal.SIGKILL)
            except ProcessLookupError:pass
            process.wait(timeout=2)


def read_exact(stream,size):
    body=bytearray()
    while len(body)<size:
        chunk=stream.read(size-len(body))
        if not chunk:raise OSError('Серверный экран закрылся.')
        body.extend(chunk)
    return bytes(body)


class RemoteBrowser:
    info=staticmethod(browser_info)
    def __init__(self,ident,send,profile=None,navigation=False):
        self.ident=ident;self.send=send;self.profile_override=profile;self.navigation=navigation
        self.process=None;self.display=None;self.wm=None;self.capture=None;self.clipboard=None
        self.runtime=None;self.profile_lock=None;self.thread=None;self.env=None;self.prefix=[]
        self.lock=threading.RLock();self.input_lock=threading.RLock();self.stop=threading.Event()
        self.sequence=0;self.outstanding=None;self.last_digest=None;self.dead=False;self.modifiers=0;self.last_pasted_digest=None;self.last_synced_digest=None;self._last_clip_check=0.0

    @staticmethod
    def address(value):
        if not isinstance(value,str) or not value.strip() or len(value)>8192 or any(ord(c)<32 or ord(c)==127 for c in value):raise ValueError('Введи адрес сайта.')
        value=value.strip()
        if '://' not in value:value='https://'+value
        try:
            parsed=urllib.parse.urlsplit(value)
            if parsed.scheme not in ('http','https') or not parsed.hostname or parsed.username or parsed.password:raise ValueError()
            parsed.port
        except ValueError:raise ValueError('Поддерживаются адреса http:// и https:// без пароля в адресе.')
        return value

    def spawn(self,args,**kwargs):
        return subprocess.Popen(self.prefix+args,env=self.env,start_new_session=True,stderr=kwargs.pop('stderr',subprocess.DEVNULL),**kwargs)

    def prepare(self):
        info=browser_info()
        if not info['ready']:raise ValueError('На сервере не подготовлен браузер: '+', '.join(info['missing']))
        uid,gid=os.geteuid(),os.getegid();home=Path.home()
        if uid==0:
            user=pwd.getpwnam('relay-browser');uid,gid=user.pw_uid,user.pw_gid;home=Path(user.pw_dir)
            self.prefix=[shutil.which('setpriv'),'--reuid='+str(uid),'--regid='+str(gid),'--init-groups','--']
        config=str(Path(os.environ.get('CLAUDE_CONFIG_DIR') or str(Path.home()/'.claude')).expanduser())
        key=hashlib.sha256(config.encode()).hexdigest()
        profile=Path(self.profile_override) if self.profile_override else (home/'profiles'/key if self.prefix else home/'.local/share/relay-desktop/browser'/key)
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
        def write_authority(number):
            fields=(b'',str(number).encode(),b'MIT-MAGIC-COOKIE-1',cookie)
            authority.write_bytes(struct.pack('!H',65535)+b''.join(struct.pack('!H',len(x))+x for x in fields));authority.chmod(0o600)
            if os.geteuid()==0:os.chown(authority,uid,gid)
        write_authority('')
        self.env={'PATH':'/usr/local/bin:/usr/bin:/bin','HOME':str(home),'LANG':'C.UTF-8','XAUTHORITY':str(authority),'XDG_RUNTIME_DIR':str(runtime)}
        self.display=self.spawn([shutil.which('Xvfb'),'-displayfd','1','-screen','0',str(WIDTH)+'x'+str(HEIGHT)+'x24','-nolisten','tcp','-auth',str(authority)],stdout=subprocess.PIPE)
        if not select.select([self.display.stdout],[],[],10)[0]:raise OSError('Не удалось открыть экран сервера.')
        number=self.display.stdout.readline().strip().decode()
        if not number.isdigit():raise OSError('Не удалось подготовить экран сервера.')
        write_authority(number);self.env['DISPLAY']=':'+number
        wm_config=runtime/'openbox.xml'
        wm_config.write_text('<openbox_config xmlns="http://openbox.org/3.4/rc"><focus><focusNew>yes</focusNew></focus><applications><application class="Chromium"><maximized>yes</maximized></application></applications></openbox_config>')
        wm_config.chmod(0o644)
        self.wm=self.spawn([shutil.which('openbox'),'--config-file',str(wm_config)],stdout=subprocess.DEVNULL)
        self.capture=self.spawn([sys.executable,'-u','-c',CAPTURE_SOURCE],stdin=subprocess.PIPE,stdout=subprocess.PIPE)
        return info['browser'],profile

    def open(self,url):
        try:
            binary,profile=self.prepare()
            self.process=self.spawn([binary,'--force-dark-mode','--no-first-run','--no-default-browser-check','--disable-dev-shm-usage',
                                     '--start-maximized','--window-size='+str(WIDTH)+','+str(HEIGHT),'--user-data-dir='+str(profile),'--new-window',url],stdout=subprocess.DEVNULL)
            end=time.monotonic()+10
            while time.monotonic()<end:
                if self.process.poll() is not None:raise OSError('Не удалось запустить Chromium на сервере.')
                try:
                    self.xdo('search','--onlyvisible','--class','chromium');break
                except OSError:time.sleep(.15)
            self.send({'type':'auth_browser','id':self.ident,'connected':True,'native':True,'origin':urllib.parse.urlsplit(url).hostname or ''})
            self.thread=threading.Thread(target=self.run,daemon=True);self.thread.start()
        except Exception:self.close();raise

    def alive(self):
        return not self.dead and not self.stop.is_set() and self.process is not None and self.process.poll() is None and self.capture is not None and self.capture.poll() is None

    def run(self):
        try:
            while not self.stop.wait(.125) and self.alive():
                now=time.monotonic()
                if now-self._last_clip_check>=.35:
                    self._last_clip_check=now
                    try:self.sync_browser_clipboard()
                    except (OSError,subprocess.SubprocessError):pass
                with self.lock:
                    if self.outstanding is not None:continue
                self.capture.stdin.write(b'g');self.capture.stdin.flush()
                size=struct.unpack('!I',read_exact(self.capture.stdout,4))[0]
                if not 0<size<=MAX_FRAME*3//4:raise ValueError('Слишком большой кадр экрана.')
                body=read_exact(self.capture.stdout,size);digest=hashlib.sha256(body).digest()
                if digest==self.last_digest:continue
                self.last_digest=digest
                with self.lock:
                    self.sequence+=1;self.outstanding=self.sequence
                    self.send({'type':'auth_frame','id':self.ident,'frame':self.sequence,'width':WIDTH,'height':HEIGHT,'data':base64.b64encode(body).decode('ascii')})
        except (OSError,ValueError,struct.error):pass
        finally:
            self.dead=True
            if not self.stop.is_set():self.send({'type':'auth_browser','id':self.ident,'connected':False,'native':True})

    def ack(self,sequence):
        with self.lock:
            if sequence==self.outstanding:self.outstanding=None

    def xdo(self,*args):
        result=subprocess.run(self.prefix+[shutil.which('xdotool'),*args],env=self.env,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=5)
        if result.returncode:raise OSError('Не удалось передать ввод в серверный браузер.')
        return result.stdout.decode('utf-8','replace').strip()

    def set_modifiers(self,value):
        if type(value)!=int or not 0<=value<=15:raise ValueError('Некорректные модификаторы.')
        for flag,key in ((1,'Alt_L'),(2,'Control_L'),(4,'Super_L'),(8,'Shift_L')):
            if bool(value&flag)!=bool(self.modifiers&flag):self.xdo('keydown' if value&flag else 'keyup',key)
        self.modifiers=value

    def shortcut(self,keys):
        self.set_modifiers(0);self.xdo('key','--clearmodifiers',keys)

    def set_clipboard(self,text):
        with self.lock:
            stop_process(self.clipboard)
            self.clipboard=None
            self.last_pasted_digest=hashlib.sha256(text.encode('utf-8')).digest() if text else None
        process=self.spawn([shutil.which('xclip'),'-selection','clipboard','-in','-quiet'],stdin=subprocess.PIPE,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
        with self.lock:
            self.clipboard=process
        ready=threading.Event()
        def drain():
            try:
                while True:
                    line=process.stderr.readline(4096)
                    if not line:break
                    if b'Waiting for selection requests' in line:ready.set()
            finally:process.stderr.close()
        threading.Thread(target=drain,daemon=True).start()
        process.stdin.write(text.encode('utf-8'));process.stdin.close()
        # Forking and connecting to X11 can take longer under load. A fixed sleep
        # allowed Ctrl+V to arrive before xclip owned the selection.
        if not ready.wait(3) or process.poll() is not None:
            stop_process(process);raise OSError('Не удалось подготовить буфер серверного браузера.')

    def read_clipboard(self,timeout=2.5):
        process=self.spawn([shutil.which('xclip'),'-selection','clipboard','-out'],stdout=subprocess.PIPE)
        body=bytearray();end=time.monotonic()+timeout;complete=False
        try:
            while time.monotonic()<end and len(body)<=65536:
                if not select.select([process.stdout],[],[],.1)[0]:continue
                chunk=os.read(process.stdout.fileno(),min(4096,65537-len(body)))
                if not chunk:complete=True;break
                body.extend(chunk)
        finally:
            stop_process(process);process.stdout.close()
        text=body.decode('utf-8','replace')
        return text if complete and len(body)<=65536 and len(text)<=16384 else ''

    def _browser_owns_clipboard(self):
        with self.lock:
            owner=self.clipboard
        return owner is None or owner.poll() is not None

    def sync_browser_clipboard(self):
        # In X11, our own `xclip -in` exits immediately when Chromium claims CLIPBOARD
        # (page "Copy" button, right-click Copy, Ctrl+C/Ctrl+X). While our xclip still
        # owns it, the clipboard holds text pasted FROM the PC — never echo it back.
        if not self._browser_owns_clipboard():
            return ''
        text=self.read_clipboard(timeout=1.5)
        if not text:
            return ''
        digest=hashlib.sha256(text.encode('utf-8')).digest()
        with self.lock:
            if digest==self.last_synced_digest or digest==self.last_pasted_digest:
                return ''
            self.last_synced_digest=digest
        self.send({'type':'auth_clipboard','id':self.ident,'text':text})
        return text

    def paste(self,text):
        self.set_clipboard(text);self.shortcut('ctrl+v');time.sleep(.04)

    def selection(self):
        # Preserve text Chromium itself already copied (e.g. a page's "Copy code" button)
        # if Ctrl+C finds no active selection, while still clearing before Ctrl+C so a
        # password field refusing Copy can never return an earlier pasted secret.
        prior=''
        if self.env is not None and self._browser_owns_clipboard():
            try:
                cand=self.read_clipboard(timeout=1.0)
                if cand:
                    digest=hashlib.sha256(cand.encode('utf-8')).digest()
                    if digest!=self.last_pasted_digest:
                        prior=cand
            except (OSError,subprocess.SubprocessError):
                prior=''
        self.set_clipboard('');self.shortcut('ctrl+c');time.sleep(.14)
        text=self.read_clipboard(timeout=2.5)
        if not text and self.env is not None:
            time.sleep(.14)
            text=self.read_clipboard(timeout=1.5)
        result=text or prior
        if result:
            with self.lock:
                self.last_synced_digest=hashlib.sha256(result.encode('utf-8')).digest()
        return result

    def navigate(self,url):
        with self.input_lock:
            self.shortcut('ctrl+l');self.paste(url);time.sleep(.15);self.xdo('key','Return')

    def input(self,value):
        if not isinstance(value,dict):raise ValueError('Некорректный ввод браузера.')
        with self.input_lock:self.handle_input(value)

    def handle_input(self,value):
        kind=value.get('kind')
        if kind in ('move','down','up','wheel'):
            coordinates=[]
            for field,limit in (('x',WIDTH),('y',HEIGHT)):
                number=value.get(field)
                if not isinstance(number,(int,float)) or isinstance(number,bool) or not math.isfinite(number) or not 0<=number<=limit:raise ValueError('Координаты вне окна браузера.')
                coordinates.append(str(min(limit-1,int(number))))
            button=value.get('button','none')
            if button not in ('none','left','right','middle'):raise ValueError('Неизвестная кнопка мыши.')
            wheels=[]
            if kind=='wheel':
                for field,negative,positive in (('deltaY','4','5'),('deltaX','6','7')):
                    delta=value.get(field,0)
                    if not isinstance(delta,(int,float)) or isinstance(delta,bool) or not math.isfinite(delta):raise ValueError('Некорректная прокрутка.')
                    if delta:wheels.append((negative if delta<0 else positive,str(min(20,max(1,round(abs(delta)/80))))))
            self.xdo('mousemove',*coordinates)
            if kind in ('down','up') and button!='none':self.xdo('mousedown' if kind=='down' else 'mouseup',{'left':'1','middle':'2','right':'3'}[button])
            for button,count in wheels:self.xdo('click','--repeat',count,'--delay','15',button)
        elif kind=='text':
            text=value.get('text')
            if not isinstance(text,str) or not 1<=len(text)<=4096 or '\0' in text:raise ValueError('Слишком длинный текст ввода.')
            self.paste(text)
        elif kind=='key':
            key=value.get('key');keys={'Enter':'Return','Backspace':'BackSpace','PageUp':'Prior','PageDown':'Next','ArrowLeft':'Left','ArrowRight':'Right','ArrowUp':'Up','ArrowDown':'Down','Home':'Home','End':'End','Tab':'Tab','Escape':'Escape','Delete':'Delete'}
            if key in ('a','c','v','x','z','y','l','t','w','r'):keys[key]=key
            if key not in keys:raise ValueError('Неизвестная клавиша.')
            self.set_modifiers(value.get('modifiers',0));self.xdo('keydown' if value.get('pressed',True) else 'keyup',keys[key])
        elif kind=='modifiers':self.set_modifiers(value.get('modifiers',0))
        elif kind=='copy':self.send({'type':'auth_clipboard','id':self.ident,'text':self.selection()})
        elif kind in ('back','forward','reload','new_tab'):
            self.shortcut({'back':'alt+Left','forward':'alt+Right','reload':'ctrl+r','new_tab':'ctrl+t'}[kind])
        elif kind in ('switch_tab','close_tab'):
            index=value.get('index')
            if index is not None:
                if type(index)!=int or not 0<=index<=8:raise ValueError('Некорректный номер вкладки.')
                self.shortcut('ctrl+'+str(index+1))
            if kind=='close_tab':self.shortcut('ctrl+w')
        elif self.navigation and kind=='navigate':self.navigate(self.address(value.get('url')))
        else:raise ValueError('Неизвестное действие браузера.')

    def close(self):
        if self.stop.is_set():return
        self.stop.set()
        for process in (self.process,self.clipboard,self.capture,self.wm,self.display):
            try:stop_process(process)
            except (OSError,subprocess.SubprocessError):pass
        if self.thread and self.thread is not threading.current_thread():self.thread.join(2)
        for stream in (getattr(self.capture,'stdin',None),getattr(self.capture,'stdout',None),getattr(self.display,'stdout',None)):
            if stream:
                try:stream.close()
                except OSError:pass
        if self.runtime:self.runtime.cleanup()
        if self.profile_lock:self.profile_lock.close()


def probe_browser(send,ident):
    """Account-free roundtrip against a private local test page, using normal screen input."""
    import http.server
    class Page(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            self.send_response(200);self.send_header('Content-Type','text/html; charset=utf-8');self.end_headers()
            page='two' if self.path.startswith('/two') else 'one'
            script="let page="+json.dumps(page)+";function report(value){fetch('/event',{method:'POST',body:JSON.stringify({page,value})})}addEventListener('pageshow',()=>report(''));document.addEventListener('visibilitychange',()=>{if(!document.hidden)report('')})"
            html="<!doctype html><meta charset=utf-8><title>Relay "+page+"</title><style>body{background:#171a24;color:#e2e6f3;font:24px sans-serif;padding:40px}input{position:fixed;box-sizing:border-box;width:calc(100% - 60px);left:30px;right:30px;top:170px;height:400px;font:26px sans-serif;background:#1b242e;color:#e6edf3;border:1px solid #32414d;border-radius:12px;padding:20px}</style><h1>Relay — браузер на сервере</h1><p>"+socket.gethostname()+"</p><input autofocus oninput=\"report(this.value)\"><script>"+script+"</script>"
            self.wfile.write(html.encode())
        def do_POST(self):
            try:state.update(json.loads(self.rfile.read(min(16384,int(self.headers.get('Content-Length','0'))))))
            except (ValueError,TypeError):pass
            self.send_response(204);self.end_headers()
        def log_message(self,*args):pass
    state={};server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Page)
    threading.Thread(target=server.serve_forever,daemon=True).start()
    home=Path(pwd.getpwnam('relay-browser').pw_dir) if os.geteuid()==0 else Path.home()
    profile=home/'profiles'/('relay-check-'+hashlib.sha256(str(ident).encode()).hexdigest())
    frames=[];browser=None
    def receive(event):
        if event.get('type')=='auth_frame':frames[:]=[event];browser.ack(event['frame'])
    browser=RemoteBrowser(ident,receive,profile=profile,navigation=True)
    url='http://127.0.0.1:'+str(server.server_port)
    def wait_state(key,value):
        end=time.monotonic()+12
        while time.monotonic()<end:
            if state.get(key)==value:return
            time.sleep(.1)
        if frames:send(frames[-1])
        raise ValueError('Не прошла проверка браузера: '+key+'; expected='+str(value)+'; actual='+str(state.get(key)))
    try:
        browser.open(url+'/one');wait_state('page','one')
        for kind in ('down','up'):browser.input({'kind':kind,'button':'left','x':400,'y':400})
        browser.input({'kind':'text','text':'СЕРВЕР 123'});wait_state('value','СЕРВЕР 123')
        for _ in range(3):
            for pressed in (True,False):browser.input({'kind':'key','key':'ArrowLeft','modifiers':8,'pressed':pressed})
        browser.input({'kind':'text','text':'456'});wait_state('value','СЕРВЕР 456')
        time.sleep(.3)
        if not frames:raise ValueError('Нет кадра серверного экрана.')
        send(frames[-1])
        report={'type':'browser_probe','id':ident,'ok':True,'server':socket.gethostname(),'browser_uid':os.stat('/proc/'+str(browser.process.pid)).st_uid,'result':'OK СЕРВЕР 456','native':True}
        browser.input({'kind':'navigate','url':url+'/two'});wait_state('page','two')
        browser.input({'kind':'back'});wait_state('page','one')
        browser.input({'kind':'forward'});wait_state('page','two')
        title=browser.xdo('getactivewindow','getwindowname')
        browser.input({'kind':'new_tab'});end=time.monotonic()+10
        while time.monotonic()<end and browser.xdo('getactivewindow','getwindowname')==title:time.sleep(.1)
        time.sleep(.25)
        browser.input({'kind':'navigate','url':url+'/one'});wait_state('page','one')
        browser.input({'kind':'switch_tab','index':0});wait_state('page','two')
        browser.input({'kind':'close_tab','index':1});time.sleep(.2)
        report.update(navigation_history=True,tabs=True)
        try:
            browser.input({'kind':'navigate','url':'https://api.ipify.org?format=json'});time.sleep(3)
            browser.shortcut('ctrl+a');text=browser.selection()
            import ipaddress
            report['browser_egress_ip']=str(ipaddress.ip_address(json.loads(text)['ip']))
        except (OSError,ValueError,KeyError,subprocess.SubprocessError):pass
        send(report)
    finally:
        browser.close();server.shutdown();server.server_close();shutil.rmtree(profile,ignore_errors=True)
