from pathlib import Path
import subprocess
root=Path(__file__).resolve().parents[1]
for name,extra in [("chat",[]),("connection",["--demo-connect"]),("empty",["--demo-empty"]),("light",["--light"]),("lists",["--demo-lists"]),("tables",["--demo-tables"]),("desktop",["--demo-desktop"])]:
    path=root/"previews"/(name+".png")
    subprocess.run([str(root/"target/debug/relay"),"--demo",*extra,"--screenshot",str(path)],check=True,timeout=25)
    print(name+": "+str(path),flush=True)
