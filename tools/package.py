from pathlib import Path
import hashlib
import json
import shutil
import tarfile
import tempfile
import tomllib
import zipfile

root = Path(__file__).resolve().parents[1]
out = root / 'dist'
out.mkdir(exist_ok=True)
version = tomllib.loads((root / 'Cargo.toml').read_text())['package']['version']
files = {}


def first(*paths):
    return next((p for p in paths if p.exists()), None)


def package(name, folder_name, sources, linux=False):
    # Isolated staging avoids carrying an obsolete binary from a previous build.
    with tempfile.TemporaryDirectory(prefix='relay-package-', dir=out) as stage:
        folder = Path(stage) / folder_name
        folder.mkdir()
        for source, target in sources:
            shutil.copy2(source, folder / target)
            if linux and target in ('relay', 'relay-tui', 'install.sh'):
                (folder / target).chmod(0o755)
        path = out / name
        if linux:
            with tarfile.open(path, 'w:gz') as archive:
                archive.add(folder, arcname=folder.name)
        else:
            with zipfile.ZipFile(path, 'w', zipfile.ZIP_DEFLATED) as archive:
                for item in sorted(folder.iterdir()):
                    archive.write(item, folder.name + '/' + item.name)
        files[name] = {'size': path.stat().st_size, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}


docs = [(root / p, p) for p in ('README.md', 'LICENSE', 'THIRD-PARTY-NOTICES.txt')]
linux = root / 'target/release/relay'
windows = first(root / 'target/windows/x86_64-pc-windows-gnullvm/release/relay.exe', root / 'target/release/relay.exe')
linux_tui = first(root / 'target/release/relay-tui')
windows_tui = first(root / 'target/windows/x86_64-pc-windows-gnullvm/release/relay-tui.exe', root / 'target/release/relay-tui.exe')
if linux.exists():
    assert linux_tui, 'Build the standalone TUI before packaging the desktop'
    package(f'Relay-{version}-linux-x64.tar.gz', 'Relay-linux-x64', [
        (linux, 'relay'), (linux_tui, 'relay-tui'), *docs,
        (root / 'assets/relay.svg', 'relay.svg'), (root / 'tools/install-linux.sh', 'install.sh')], linux=True)
if windows:
    assert windows_tui, 'Build the standalone TUI before packaging the desktop'
    package(f'Relay-{version}-windows-x64.zip', 'Relay-windows-x64', [
        (windows, 'Relay.exe'), (windows_tui, 'Relay-TUI.exe'), *docs])
if linux_tui:
    package(f'Relay-TUI-{version}-linux-x64.tar.gz', 'Relay-TUI-linux-x64', [
        (linux_tui, 'relay-tui'), *docs, (root / 'tools/install-tui-linux.sh', 'install.sh')], linux=True)
if windows_tui:
    package(f'Relay-TUI-{version}-windows-x64.zip', 'Relay-TUI-windows-x64', [
        (windows_tui, 'Relay-TUI.exe'), *docs])
(out / 'checksums.json').write_text(json.dumps(files, indent=2) + '\n')
print(json.dumps(files, indent=2))
