import importlib.util
import json
import queue
import socket
import threading
import tempfile
import time
import unittest
from pathlib import Path

SOURCE = Path(__file__).parents[1] / "bridge/desktop_mcp.py"
spec = importlib.util.spec_from_file_location("phone_mcp_fixture", SOURCE)
mcp = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mcp)

class PairedPhoneTests(unittest.TestCase):
    def test_confirmation_is_session_scoped_and_result_is_replayed(self):
        with tempfile.TemporaryDirectory() as folder, socket.socket(socket.AF_UNIX) as server:
            service = mcp.RelayDesktop(lambda _: None)
            self.addCleanup(service.close)
            service.phone_socket = Path(folder) / "worker.sock"
            server.bind(str(service.phone_socket))
            server.listen(1)
            event = {"id":"job", "sid":"one", "tool":"run_local_command",
                     "type":"desktop_request", "input":{"command":"test"}}
            item = {"sid":"one", "event":threading.Event()}
            service.pending["job"] = item
            result = queue.Queue()
            def drive():
                try: result.put(service.run_phone(event, item, 10))
                except Exception as error: result.put(error)
            threading.Thread(target=drive, daemon=True).start()
            with server.accept()[0] as conn, conn.makefile("rb") as stream:
                conn.sendall(b'{"op":"phone_challenge","nonce":"fixture"}\n')
                for _ in range(100):
                    if service.phone_state("one")["jobs"][0]["stage"] == "approval": break
                    time.sleep(.01)
                self.assertEqual(service.phone_state("two")["jobs"], [])
                with self.assertRaises(ValueError): service.phone_decide("two", "job", "")
                payload = service.phone_state("one")["jobs"][0]["payload"]
                service.phone_decide("one", "job", "A"*43+"=")
                packet = json.loads(stream.readline())
                self.assertEqual(packet["payload"], payload)
                self.assertEqual(json.loads(payload)["input"], {"command":"test"})
                conn.sendall(b'{"op":"desktop_result","id":"job","result":{"ok":true}}\n')
                self.assertIsNone(result.get(timeout=2))
                self.assertEqual(item["result"], {"ok":True})
