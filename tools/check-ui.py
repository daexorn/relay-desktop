"""Capture isolated native demo windows; never connects to a real host."""
import argparse,json,os,struct,subprocess,tempfile,time,tomllib
from pathlib import Path

root=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument('--binary',default=str(root/'target/debug/relay'));p.add_argument('--out',default=str(root/'previews'/tomllib.loads((root/'Cargo.toml').read_text())['package']['version']));p.add_argument('--only',nargs='*');p.add_argument('--browser-frame');args=p.parse_args()
out=Path(args.out);out.mkdir(parents=True,exist_ok=True)
expected=tomllib.loads((root/'Cargo.toml').read_text())['package']['version']
assert subprocess.check_output([args.binary,'--version'],text=True).strip()=='Relay '+expected,'Build the current Relay before capturing previews'

cases=[('cluster-editor',['--demo-cluster-editor']),('cluster-prompts',['--demo-cluster-prompts']),('cluster-keys',['--demo-cluster-keys']),('leader',['--demo-leader']),('command-output',['--demo-command-output']),('messages',['--demo-send']),('attachments',['--demo-attachments']),('attachments-small',['--demo-attachments','--demo-small']),
       ('math',['--demo-math']),('math-2',['--demo-math-2']),('math-light',['--demo-math','--light']),
       ('commands',['--demo-activity-open']),('settings',['--demo-settings']),
       ('cluster',['--demo-cluster','--theme','midnight']),('forest',['--theme','forest']),
       ('paper',['--theme','paper'])]
cases += [('home',['--demo-empty']),('home-small',['--demo-empty','--demo-small']),('api',['--demo-api-settings']),('connection',['--demo-connect'])]
if args.browser_frame:
    cases += [('browser',['--demo-browser-frame',args.browser_frame]),('browser-small',['--demo-browser-frame',args.browser_frame,'--demo-small','--demo-browser-expanded']),('browser-expanded',['--demo-browser-frame',args.browser_frame,'--demo-browser-expanded']),('browser-docked',['--demo-browser-frame',args.browser_frame,'--demo-browser-docked'])]
for name,extra in cases:
    if args.only and name not in args.only:continue
    with tempfile.TemporaryDirectory(prefix='relay-demo-') as temp:
        env=dict(os.environ,XDG_CONFIG_HOME=temp+'/config',XDG_DATA_HOME=temp+'/data',RELAY_CONFIG_DIR=temp+'/config/relay-desktop')
        command=[args.binary,'--demo',*extra,'--screenshot',str(out/(name+'.png'))]
        small='--demo-small' in extra
        if small:command += ['--screenshot-delay-ms','6000']
        process=subprocess.Popen(command,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
        try:
            # Configure only this temporary preview window. The tiling compositor
            # otherwise overrides the size requested by --demo-small.
            if small and env.get('NIRI_SOCKET'):
                for _ in range(125):
                    windows=json.loads(subprocess.check_output(['niri','msg','--json','windows'],text=True))
                    window=next((w for w in windows if w.get('pid')==process.pid),None)
                    if window:
                        ident=str(window['id'])
                        if not window.get('is_floating'):subprocess.run(['niri','msg','action','toggle-window-floating','--id',ident],check=True,stdout=subprocess.DEVNULL)
                        for action,size in [('set-window-width','680'),('set-window-height','720')]:subprocess.run(['niri','msg','action',action,'--id',ident,size],check=True,stdout=subprocess.DEVNULL)
                        break
                    time.sleep(.04)
            stdout,stderr=process.communicate(timeout=30)
        except Exception:
            process.terminate();process.communicate(timeout=5);raise
        if process.returncode:raise SystemExit(stderr[-4000:])
        assert (out/(name+'.png')).is_file(),name
        dimensions=struct.unpack('!II',(out/(name+'.png')).read_bytes()[16:24])
        print(name,dimensions,flush=True)
