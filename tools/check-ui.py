"""Capture isolated native demo windows; never connects to a real host."""
import argparse,os,subprocess,tempfile
from pathlib import Path

root=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument('--binary',default=str(root/'target/debug/relay'));p.add_argument('--out',default=str(root/'previews'/'0.6.0'));p.add_argument('--only',nargs='*');args=p.parse_args()
out=Path(args.out);out.mkdir(parents=True,exist_ok=True)
cases=[('messages',['--demo-send']),('attachments',['--demo-attachments']),('attachments-small',['--demo-attachments','--demo-small']),
       ('commands',['--demo-activity-open']),('settings',['--demo-settings']),
       ('cluster',['--demo-cluster','--theme','midnight']),('forest',['--theme','forest']),
       ('paper',['--theme','paper'])]
for name,extra in cases:
    if args.only and name not in args.only:continue
    with tempfile.TemporaryDirectory(prefix='relay-demo-') as temp:
        env=dict(os.environ,XDG_CONFIG_HOME=temp+'/config',XDG_DATA_HOME=temp+'/data')
        result=subprocess.run([args.binary,'--demo',*extra,'--screenshot',str(out/(name+'.png'))],env=env,capture_output=True,text=True,timeout=30)
        if result.returncode:raise SystemExit(result.stderr[-4000:])
        assert (out/(name+'.png')).is_file(),name
        print(name,flush=True)
