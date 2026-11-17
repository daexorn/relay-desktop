"""Run the unmodified Claude CLI login. Never read/copy its credential store."""
import errno
import fcntl
import json
import os
import pty
import re
import select
import signal
import struct
import subprocess
import termios
import threading
import time
import tempfile
import urllib.parse
import uuid

AUTH_HOSTS = {"claude.ai", "claude.com", "console.anthropic.com", "platform.claude.com"}
ANSI = re.compile(r"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\))")


def login_url(text):
    for value in re.findall(r"https://[^\s<>\"'\x00-\x1f\x7f]+", text + "\n" + ANSI.sub("", text)):
        value = value.rstrip(").,;")
        try:
            url = urllib.parse.urlsplit(value)
            if (len(value) <= 12000 and url.hostname in AUTH_HOSTS and not url.username and not url.password
                    and url.port in (None, 443) and url.path.rstrip("/") in ("/oauth/authorize", "/cai/oauth/authorize")):
                return value
        except ValueError:
            pass
    return None


class AuthManager:
    def __init__(self, binary, timeout=600, browser_factory=None):
        self.binary = binary
        self.browser_factory = browser_factory
        self.timeout = timeout
        self.lock = threading.RLock()
        self.current = None

    def start(self, ident, send, owner=None, standalone=False, url='https://claude.ai/'):
        if not isinstance(ident, str) or str(uuid.UUID(ident)) != ident:
            raise ValueError("Некорректный запрос входа.")
        if not self.binary and not standalone:
            raise ValueError("На сервере не установлен Claude Code.")
        if self.browser_factory is None:raise ValueError('Обнови серверного помощника Relay для трансляции браузера.')
        info=self.browser_factory.info()
        if not info['ready']:raise ValueError('На сервере не подготовлен браузер: '+', '.join(info['missing']))
        if standalone:
            url=self.browser_factory.address(url)
        with self.lock:
            previous=self.current
        if previous is not None and previous['owner'] is owner and previous['cancel'].is_set():
            previous['thread'].join(9)
        with self.lock:
            if self.current is not None:
                raise ValueError("На этом сервере уже открыт браузер. Заверши или закрой его.")
            flow = {"id": ident, "owner": owner, "send": send, "cancel": threading.Event(), "process": None, "fd": None, "browser":None, 'standalone':standalone, 'url':url}
            self.current = flow
        self.event(flow, "starting", "Открываем браузер на сервере…" if standalone else "Готовим вход на сервере…")
        thread = threading.Thread(target=self.run_browser if standalone else self.run, args=(flow,), daemon=True)
        flow["thread"] = thread
        thread.start()

    def run_browser(self, flow):
        try:
            flow['browser']=self.browser_factory(flow['id'],flow['send'],navigation=True)
            flow['browser'].open(flow['url'])
            self.event(flow,'browser','Страницы открываются на сервере. Управляй браузером здесь.',remote=True)
            while not flow['cancel'].wait(.25):
                if not flow['browser'].alive():
                    self.event(flow,'failed','Браузер сервера закрылся. Открой его ещё раз.')
                    return
            self.event(flow,'cancelled','Браузер закрыт.')
        except Exception:
            self.event(flow,'failed','Не удалось открыть браузер на сервере.')
        finally:
            if flow['browser'] is not None:
                try:flow['browser'].close()
                except Exception:pass
            with self.lock:
                if self.current is flow:self.current=None

    @staticmethod
    def event(flow, state, message, **fields):
        flow["send"]({"type": "auth", "id": flow["id"], "state": state, "message": message, **fields})

    def run(self, flow):
        process = None
        master = slave = None
        runtime = None
        try:
            runtime=tempfile.TemporaryDirectory(prefix='relay-login-')
            flow['runtime']=runtime.name
            destination=os.path.join(runtime.name,'open-url')
            launcher=os.path.join(runtime.name,'browser')
            with open(launcher,'w') as script:
                script.write('#!/usr/bin/env python3\nimport os,sys\nfd=os.open('+repr(destination)+',os.O_WRONLY|os.O_CREAT|os.O_TRUNC,0o600)\nwith os.fdopen(fd,"w") as f:f.write(sys.argv[-1])\n')
            os.chmod(launcher,0o700)
            master, slave = pty.openpty()
            attrs = termios.tcgetattr(slave)
            attrs[3] &= ~(termios.ECHO | termios.ECHONL)
            termios.tcsetattr(slave, termios.TCSANOW, attrs)
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 1600, 0, 0))
            env = dict(os.environ)
            env.pop("CLAUDECODE", None)
            env.pop("DISPLAY", None)
            env.pop("WAYLAND_DISPLAY", None)
            # The official CLI launches this server-side adapter. Its normal localhost
            # callback and the account page both stay on this server.
            env["BROWSER"] = launcher
            process = subprocess.Popen([self.binary, "auth", "login"], stdin=slave, stdout=slave,
                                       stderr=slave, env=env, start_new_session=True, close_fds=True)
            os.close(slave); slave = None
            with self.lock:
                flow.update(process=process, fd=master)
            end = time.monotonic() + self.timeout
            output = ""
            offered = None
            eof = False
            browser_failed=False
            while not flow["cancel"].is_set() and time.monotonic() < end:
                if select.select([master], [], [], 0.2)[0]:
                    try:
                        chunk = os.read(master, 8192)
                    except OSError as error:
                        if error.errno == errno.EIO:eof = True;break
                        raise
                    if not chunk:eof = True;break
                    output = (output + chunk.decode("utf-8", "replace"))[-48000:]
                url=None
                try:
                    with open(destination) as file:url=login_url(file.read(12001))
                except FileNotFoundError:pass
                url=url or login_url(output)
                if url and url != offered:
                    if flow['browser'] is None:
                        flow['browser']=self.browser_factory(flow['id'],flow['send'])
                        flow['browser'].open(url)
                    else:flow['browser'].navigate(url)
                    offered=url
                    self.event(flow, "browser", "Браузер работает на сервере. Войди на странице ниже.", remote=True)
                if flow['browser'] is not None and not flow['browser'].alive():browser_failed=True;break
                if process.poll() is not None:break
            if eof and process.poll() is None:
                try:process.wait(timeout=2)
                except subprocess.TimeoutExpired:pass
            if browser_failed:
                self.event(flow,'failed','Браузер сервера закрылся. Повтори вход.')
            elif flow["cancel"].is_set():
                self.event(flow, "cancelled", "Вход отменён.")
            elif process.poll() is None:
                self.event(flow, "failed", "Время входа истекло. Открой вход ещё раз.")
            elif process.returncode != 0:
                self.event(flow, "failed", "Claude Code не завершил вход. Попробуй ещё раз в браузере сервера.")
            else:
                result = subprocess.run([self.binary, "auth", "status"], capture_output=True, timeout=20, env=env)
                try:state = json.loads(result.stdout)
                except (ValueError, UnicodeDecodeError):state = {}
                if result.returncode == 0 and state.get("loggedIn") is True:
                    self.event(flow, "success", "Вход сохранён на сервере. Можно продолжать прежние сессии.")
                else:
                    self.event(flow, "failed", "Claude Code завершился, но не подтвердил вход. Повтори попытку.")
        except Exception:
            # No raw CLI output: it can contain codes and account information.
            self.event(flow, "failed", "Не удалось завершить вход в Claude Code на сервере.")
        finally:
            if flow['browser'] is not None:
                try:flow['browser'].close()
                except Exception:pass
            if process is not None:
                self.stop_process(process)
            with self.lock:
                flow["fd"] = None
                if self.current is flow:self.current = None
            for fd in (master, slave):
                if fd is not None:
                    try:os.close(fd)
                    except OSError:pass
            if runtime is not None:runtime.cleanup()

    @staticmethod
    def stop_process(process):
        if process.poll() is None:
            try:os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:pass
            try:process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                try:os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:pass
                process.wait(timeout=2)

    def input(self, ident, code, owner=None):
        if not isinstance(code, str) or not 1 <= len(code.strip()) <= 4096 or any(ord(c) < 32 or ord(c) == 127 for c in code.strip()):
            raise ValueError("Вставь код входа одной строкой.")
        with self.lock:
            flow = self.current
            if not flow or flow["id"] != ident or flow["owner"] is not owner or flow["fd"] is None or flow["cancel"].is_set():
                raise ValueError("Этот запрос входа уже закрыт.")
            os.write(flow["fd"], (code.strip() + "\n").encode("utf-8"))
        self.event(flow, "verifying", "Claude Code проверяет код…")

    def cancel(self, ident=None, owner=None, closing=False):
        with self.lock:
            flow = self.current
            if not flow or not closing and (flow["id"] != ident or flow["owner"] is not owner):return
            flow["cancel"].set()

    def cancel_owner(self, owner):
        with self.lock:
            if self.current and self.current["owner"] is owner:self.current["cancel"].set()

    def browser(self, ident, value, owner=None, ack=False):
        with self.lock:
            flow=self.current
            if not flow or flow['id']!=ident or flow['owner'] is not owner or flow['cancel'].is_set() or flow['browser'] is None:
                if ack:return
                raise ValueError('Этот серверный браузер уже закрыт.')
            browser=flow['browser']
        if ack:
            if type(value)!=int or value<1:raise ValueError('Некорректное подтверждение кадра.')
            browser.ack(value)
        else:browser.input(value)
