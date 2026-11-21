"""SSH stdio endpoint for one paired PC. Only the local PC verifies approvals."""
import fcntl
import json
import os
from pathlib import Path
import select
import socket
import sys

os.umask(0o077)
root = Path.home() / ".cache/relay-desktop/phone"
root.mkdir(mode=0o700, parents=True, exist_ok=True)
os.chdir(root)
guard = open("worker.lock", "a")
fcntl.flock(guard, fcntl.LOCK_EX | fcntl.LOCK_NB)
path = Path("worker.sock")
path.unlink(missing_ok=True)
with socket.socket(socket.AF_UNIX) as listener:
    listener.bind(str(path))
    listener.listen(4)
    print('{"type":"hello"}', flush=True)
    client = None
    try:
        while True:
            readers = [listener, 0] + ([client] if client else [])
            for ready in select.select(readers, [], [])[0]:
                if ready is listener:
                    incoming, _ = listener.accept()
                    if client:
                        try: incoming.sendall(b'{"op":"busy"}\n')
                        except OSError: pass
                        incoming.close()
                    else:
                        client = incoming
                        print('{"type":"phone_open"}', flush=True)
                elif ready == 0:
                    block = os.read(0, 65536)
                    if not block:
                        sys.exit()
                    if client:
                        client.sendall(block)
                else:
                    block = client.recv(65536)
                    if not block:
                        sys.exit()
                    sys.stdout.buffer.write(block)
                    sys.stdout.buffer.flush()
    finally:
        path.unlink(missing_ok=True)
        if client:
            client.close()
