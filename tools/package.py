from pathlib import Path
import hashlib,json,shutil,tarfile,zipfile,tomllib

root=Path(__file__).resolve().parents[1]
out=root/'dist';out.mkdir(exist_ok=True)
version=tomllib.loads((root/'Cargo.toml').read_text())['package']['version']
linux=root/'target/release/relay'
windows=root/'target/windows/x86_64-pc-windows-gnullvm/release/relay.exe'
if not windows.exists():windows=root/'target/release/relay.exe'
files={}
if linux.exists():
    folder=out/'Relay-linux-x64';folder.mkdir(exist_ok=True)
    for source,name in [(linux,'relay'),(root/'README.md','README.md'),(root/'LICENSE','LICENSE'),(root/'THIRD-PARTY-NOTICES.txt','THIRD-PARTY-NOTICES.txt'),(root/'assets/relay.svg','relay.svg'),(root/'tools/install-linux.sh','install.sh')]:
        shutil.copy2(source,folder/name)
    (folder/'relay').chmod(0o755);(folder/'install.sh').chmod(0o755)
    path=out/('Relay-'+version+'-linux-x64.tar.gz')
    with tarfile.open(path,'w:gz') as archive:archive.add(folder,arcname=folder.name)
    files[path.name]={'size':path.stat().st_size,'sha256':hashlib.sha256(path.read_bytes()).hexdigest()}
if windows.exists():
    folder=out/'Relay-windows-x64';folder.mkdir(exist_ok=True)
    for source,name in [(windows,'Relay.exe'),(root/'README.md','README.md'),(root/'LICENSE','LICENSE'),(root/'THIRD-PARTY-NOTICES.txt','THIRD-PARTY-NOTICES.txt')]:shutil.copy2(source,folder/name)
    path=out/('Relay-'+version+'-windows-x64.zip')
    with zipfile.ZipFile(path,'w',zipfile.ZIP_DEFLATED) as archive:
        for p in sorted(folder.iterdir()):archive.write(p,folder.name+'/'+p.name)
    files[path.name]={'size':path.stat().st_size,'sha256':hashlib.sha256(path.read_bytes()).hexdigest()}
(out/'checksums.json').write_text(json.dumps(files,indent=2)+'\n')
print(json.dumps(files,indent=2))
