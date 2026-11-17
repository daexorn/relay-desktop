"""Exercise the real TUI in a private PTY; no servers, credentials or model calls."""
import argparse
import json
import os
from pathlib import Path
import select
import signal
import struct
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser()
parser.add_argument('--binary', default='target/debug/relay-tui')
parser.add_argument('--report', type=Path)
args = parser.parse_args()
binary = str(Path(args.binary).resolve())
report = {'binary': binary, 'network': False, 'modelRequests': 0}

with tempfile.TemporaryDirectory(prefix='relay-tui-check-') as temporary:
    env = {**os.environ, 'RELAY_CONFIG_DIR': temporary, 'TERM': 'xterm-256color'}
    assert 'Relay TUI' in subprocess.check_output([binary, '--help'], env=env, text=True)
    report['version'] = subprocess.check_output([binary, '--version'], env=env, text=True).strip()
    assert subprocess.run([binary, '--unknown'], env=env, capture_output=True).returncode != 0
    snapshots = []
    for width, height in [(32, 10), (60, 18), (80, 24), (120, 40), (180, 55)]:
        output = Path(temporary) / f'{width}x{height}.txt'
        subprocess.run([binary, '--demo', '--snapshot', str(output), '--width', str(width), '--height', str(height)], env=env, check=True)
        cells = json.loads(output.with_suffix('.json').read_text())
        assert len(cells['cells']) == height and all(len(row) == width for row in cells['cells'])
        assert '\x1b' not in output.read_text()
        snapshots.append([width, height])
    report['snapshots'] = snapshots
    if os.name == 'posix':
        import fcntl
        import pty
        import termios

        def scenario(terminate=False):
            master, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 100, 0, 0))
            original = termios.tcgetattr(slave)

            def session():
                os.setsid()
                fcntl.ioctl(0, termios.TIOCSCTTY, 0)

            process = subprocess.Popen([binary, '--demo'], stdin=slave, stdout=slave, stderr=slave, env=env, preexec_fn=session)
            output = bytearray()

            def drain(seconds=0.2):
                until = time.monotonic() + seconds
                while time.monotonic() < until:
                    ready, _, _ = select.select([master], [], [], 0.02)
                    if ready:
                        try:
                            data = os.read(master, 65536)
                        except OSError:
                            break
                        output.extend(data)

            try:
                until = time.monotonic() + 10
                while b'Relay' not in output and process.poll() is None and time.monotonic() < until:
                    drain()
                assert process.poll() is None, output.decode(errors='replace')[-2000:]
                assert b'Relay' in output, 'No TUI screen in PTY'
                assert not termios.tcgetattr(slave)[3] & termios.ICANON
                if terminate:
                    process.send_signal(signal.SIGTERM)
                else:
                    os.write(master, b'\x1b[200~' + 'проверь файл 👩‍💻'.encode() + b'\x1b[201~')
                    drain()
                    os.write(master, b'\x1b[D' + 'я'.encode() + b'\x1b\r' + 'новая строка'.encode())
                    drain()
                    os.write(master, b'\x1bOQ')
                    drain()
                    os.write(master, b'\x1b')
                    drain()
                    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 14, 40, 0, 0))
                    process.send_signal(signal.SIGWINCH)
                    drain()
                    os.write(master, b'\x11')
                    drain()
                    os.write(master, b'\x19')
                until = time.monotonic() + 10
                while process.poll() is None and time.monotonic() < until:
                    drain()
                assert process.poll() == 0, output.decode(errors='replace')[-2000:]
                drain()
                assert termios.tcgetattr(slave) == original, 'Raw mode was not restored'
                assert b'\x1b[?1049l' in output, 'Alternate screen was not restored'
                assert b'\x1b[?2004l' in output, 'Bracketed paste was not disabled'
                if not terminate:
                    assert 'проверь'.encode() in output, 'Unicode paste did not appear'
                    assert 'Серверы'.encode() in output, 'F2 did not open hosts'
                return len(output)
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()
                os.close(master)
                os.close(slave)

        report['pty'] = {'editingAndResize': scenario(), 'sigtermCleanup': scenario(True), 'terminalRestored': True}
    assert not list(Path(temporary).glob('*.pending'))
    assert not (Path(temporary) / 'settings.json').exists(), 'Demo modified GUI settings'
    assert not (Path(temporary) / 'tui-state.json').exists(), 'Demo persisted fake state'

report['ok'] = True
if args.report:
    args.report.write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report))
