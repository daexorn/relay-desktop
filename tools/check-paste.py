"""Exercise actual Ctrl+V into a disposable demo window. Restore the clipboard.
Never logs, stores or transmits the previous clipboard contents.
"""
from pathlib import Path
import ctypes as C
import ctypes.util
import io,json,os,subprocess,time
from PIL import Image
root=Path(__file__).resolve().parents[1]
types=subprocess.check_output(['wl-paste','--list-types'],text=True).splitlines()
text_types={'text/plain','text/plain;charset=utf-8','UTF8_STRING','STRING','TEXT','text/plain;charset=UTF-8'}
if 'image/png' in types:
 old_type='image/png'
elif types and set(types)<=text_types:
 old_type=next(t for t in types if t.startswith('text/plain'))
else:
 raise SystemExit('Clipboard has rich formats; test skipped without changing it.')
old=subprocess.check_output(['wl-paste','--no-newline','--type',old_type])
image=Image.new('RGB',(80,48),(18,212,202));png=io.BytesIO();image.save(png,format='PNG');fixture=png.getvalue()
env=dict(os.environ);env.pop('WAYLAND_DISPLAY',None);env.pop('WAYLAND_SOCKET',None)
shot=root/'previews/paste-check.png'
before={w['id'] for w in json.loads(subprocess.check_output(['niri','msg','--json','windows'],text=True))}
process=subprocess.Popen([str(root/'target/debug/relay'),'--demo','--screenshot',str(shot),'--screenshot-delay-ms','2600'],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
restored=False
try:
 x=C.CDLL(ctypes.util.find_library('X11'));xt=C.CDLL(ctypes.util.find_library('Xtst'))
 x.XOpenDisplay.argtypes=[C.c_char_p];x.XOpenDisplay.restype=C.c_void_p;d=x.XOpenDisplay(None);assert d,'X11 display unavailable'
 x.XDefaultRootWindow.argtypes=[C.c_void_p];x.XDefaultRootWindow.restype=C.c_ulong
 x.XInternAtom.argtypes=[C.c_void_p,C.c_char_p,C.c_int];x.XInternAtom.restype=C.c_ulong
 x.XQueryTree.argtypes=[C.c_void_p,C.c_ulong,C.POINTER(C.c_ulong),C.POINTER(C.c_ulong),C.POINTER(C.POINTER(C.c_ulong)),C.POINTER(C.c_uint)]
 x.XGetWindowProperty.argtypes=[C.c_void_p,C.c_ulong,C.c_ulong,C.c_long,C.c_long,C.c_int,C.c_ulong,C.POINTER(C.c_ulong),C.POINTER(C.c_int),C.POINTER(C.c_ulong),C.POINTER(C.c_ulong),C.POINTER(C.POINTER(C.c_ubyte))]
 x.XFree.argtypes=[C.c_void_p]
 x.XSetInputFocus.argtypes=[C.c_void_p,C.c_ulong,C.c_int,C.c_ulong]
 x.XKeysymToKeycode.argtypes=[C.c_void_p,C.c_ulong];x.XKeysymToKeycode.restype=C.c_ubyte
 x.XFlush.argtypes=[C.c_void_p]
 xt.XTestFakeKeyEvent.argtypes=[C.c_void_p,C.c_uint,C.c_int,C.c_ulong]
 def children(window):
  rr=C.c_ulong();parent=C.c_ulong();ptr=C.POINTER(C.c_ulong)();count=C.c_uint()
  if not x.XQueryTree(d,window,C.byref(rr),C.byref(parent),C.byref(ptr),C.byref(count)):return []
  result=[ptr[i] for i in range(count.value)]
  if ptr:x.XFree(ptr)
  return result
 def pid(window):
  actual=C.c_ulong();fmt=C.c_int();n=C.c_ulong();after=C.c_ulong();data=C.POINTER(C.c_ubyte)()
  x.XGetWindowProperty(d,window,x.XInternAtom(d,b'_NET_WM_PID',0),0,1,0,0,C.byref(actual),C.byref(fmt),C.byref(n),C.byref(after),C.byref(data))
  result=C.cast(data,C.POINTER(C.c_ulong))[0] if data and n.value and fmt.value==32 else None
  if data:x.XFree(data)
  return result
 window=None
 for _ in range(80):
  window=next((w for w in children(x.XDefaultRootWindow(d)) if pid(w)==process.pid),None)
  if window:break
  if process.poll() is not None:raise RuntimeError(process.stderr.read().decode()[-1000:])
  time.sleep(.015)
 assert window,'Demo window did not appear'
 target=None
 for _ in range(60):
  windows=json.loads(subprocess.check_output(['niri','msg','--json','windows'],text=True))
  target=next((w for w in windows if w.get('pid')==process.pid or (w['id'] not in before and 'relay' in str(w.get('title','')).lower())),None)
  if target:break
  time.sleep(.02)
 assert target,'Demo window not mapped by compositor'
 subprocess.run(['niri','msg','action','focus-window','--id',str(target['id'])],check=True,stdout=subprocess.DEVNULL)
 x.XSetInputFocus(d,window,2,0);x.XFlush(d)
 subprocess.run(['wl-copy','--type','image/png'],input=fixture,check=True)
 time.sleep(.13)
 control=x.XKeysymToKeycode(d,0xffe3);v=x.XKeysymToKeycode(d,ord('v'));assert control and v
 for key,pressed in [(control,1),(v,1),(v,0),(control,0)]:xt.XTestFakeKeyEvent(d,key,pressed,0)
 x.XFlush(d);time.sleep(.65)
 try:current=subprocess.check_output(['wl-paste','--no-newline','--type','image/png'],stderr=subprocess.DEVNULL)
 except subprocess.CalledProcessError:current=None
 if current==fixture:
  subprocess.run(['wl-copy','--type',old_type],input=old,check=True);restored=True
 process.wait(timeout=8)
 assert process.returncode==0,process.stderr.read().decode()[-1000:]
 screenshot=Image.open(shot).convert('RGB');pixels=sum(1 for pixel in screenshot.getdata() if pixel==(18,212,202))
 assert pixels>200,'The pasted thumbnail did not appear'
 print(json.dumps({'ctrl_v':'PASS','thumbnail_pixels':pixels,'clipboard_restored':restored,'screenshot':str(shot)}))
finally:
 if process.poll() is None:process.terminate();process.wait(timeout=5)
 if not restored:
  try:
   if subprocess.check_output(['wl-paste','--no-newline','--type','image/png'],stderr=subprocess.DEVNULL)==fixture:
    subprocess.run(['wl-copy','--type',old_type],input=old,check=True)
  except subprocess.CalledProcessError:pass
