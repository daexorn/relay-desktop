import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock
import uuid

SPEC = importlib.util.spec_from_file_location('relay_attachments', Path(__file__).parents[1] / 'bridge/attachments.py')
uploads = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(uploads)
BRIDGE_SPEC = importlib.util.spec_from_file_location('attachment_bridge', Path(__file__).parents[1] / 'bridge/relay_bridge.py')
bridge = importlib.util.module_from_spec(BRIDGE_SPEC)
BRIDGE_SPEC.loader.exec_module(bridge)


class BoundedReader(io.BytesIO):
    def read(self, size=-1):
        if not 0 <= size <= uploads.CHUNK:
            raise AssertionError('Upload tried to read an unbounded body')
        return super().read(size)


class AttachmentTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.sid = str(uuid.uuid4())
        self.transfer = str(uuid.uuid4())

    def receive(self, files, checksums=None, header=None, tail=None):
        if header is None:
            header = {'sid': self.sid, 'upload_id': self.transfer,
                      'files': [{'name': name, 'bytes': len(body)} for name, body in files]}
        body = b''.join(body for _, body in files)
        if tail is None:
            tail = json.dumps({'sha256': checksums if checksums is not None else
                              [hashlib.sha256(body).hexdigest() for _, body in files]}).encode() + b'\n'
        stream = BoundedReader(json.dumps(header).encode() + b'\n' + body + tail)
        result = io.StringIO()
        ok = uploads.receive(stream, result, self.root)
        events = [json.loads(line) for line in result.getvalue().splitlines()]
        return ok, events[-1]

    def test_file_above_old_limit_streams_and_resolves_with_matching_hash(self):
        body = b'large-attachment\x00' * (1024 * 1024)
        self.assertGreater(len(body), 12 * 1024 * 1024)
        ok, result = self.receive([('mod.jar', body)])
        self.assertTrue(ok, result)
        item = uploads.UploadStore(self.root).resolve(self.sid, result['files'][0])
        path = Path(item['path'])
        self.assertTrue(path.is_relative_to(self.root / self.sid))
        self.assertEqual(path.read_bytes(), body)
        self.assertEqual(item['sha256'], hashlib.sha256(body).hexdigest())
        self.assertFalse(list((self.root / self.sid).glob('.*.partial')))
        events = []
        host = bridge.Bridge(send=events.append, binary='fixture-only', transcripts=bridge.Transcripts(self.root / 'transcripts'))
        host.uploads.root = self.root
        with mock.patch.object(bridge, 'Run') as run:
            host.dispatch({'op': 'prompt', 'sid': self.sid, 'cwd': str(self.root),
                           'text': '', 'files': result['files'], 'mode': 'auto'})
            run.assert_called_once()
            text = run.return_value.prompt.call_args.args[0]
            self.assertIn(str(path), text)
            self.assertNotIn('data', result['files'][0])
        self.assertFalse(any(e.get('type') == 'error' for e in events), events)
        host.close()

    def test_duplicate_and_unsafe_names_never_overwrite_each_other(self):
        ok, result = self.receive([('../../same.txt', b'first'), ('same.txt', b'second'), ('empty.txt', b'')])
        self.assertTrue(ok)
        store = uploads.UploadStore(self.root)
        files = [store.resolve(self.sid, source) for source in result['files']]
        self.assertEqual([Path(f['path']).read_bytes() for f in files], [b'first', b'second', b''])
        self.assertEqual(len({f['path'] for f in files}), 3)
        self.assertEqual([f['name'] for f in files], ['same.txt', 'same.txt', 'empty.txt'])

    def test_bad_checksum_and_truncation_remove_partial_uploads(self):
        for checksums, tail in [(['0' * 64], None), (None, b'')]:
            with self.subTest(checksums=checksums):
                ok, result = self.receive([('mod.jar', b'payload')], checksums=checksums, tail=tail)
                self.assertFalse(ok)
                self.assertFalse((self.root / self.sid / self.transfer).exists())
                self.assertFalse(list((self.root / self.sid).glob('.*.partial')))

    def test_oversize_is_rejected_before_receiving_any_body(self):
        header = {'sid': self.sid, 'upload_id': self.transfer,
                  'files': [{'name': 'large.zip', 'bytes': uploads.MAX_BYTES + 1}]}
        ok, result = self.receive([], header=header)
        self.assertFalse(ok)
        self.assertIn('512', result['message'])
        self.assertFalse((self.root / self.sid).exists())
        header['files'] = [{'name': 'a', 'bytes': uploads.MAX_BYTES}, {'name': 'b', 'bytes': 1}]
        self.assertFalse(self.receive([], header=header)[0])

    def test_other_session_and_changed_file_cannot_be_used(self):
        ok, result = self.receive([('data.txt', b'payload')])
        self.assertTrue(ok)
        source = result['files'][0]
        store = uploads.UploadStore(self.root)
        with self.assertRaises(ValueError): store.resolve(str(uuid.uuid4()), source)
        with self.assertRaises(ValueError): store.resolve(self.sid, dict(source, upload_id='../outside'))
        with self.assertRaises(ValueError): store.resolve(self.sid, dict(source, sha256='0' * 64))
        path = Path(store.resolve(self.sid, source)['path'])
        path.write_bytes(b'changed length')
        with self.assertRaises(ValueError): store.resolve(self.sid, source)

    def test_symlink_is_not_resolved_as_an_uploaded_file(self):
        ok, result = self.receive([('data.txt', b'payload')])
        self.assertTrue(ok)
        store = uploads.UploadStore(self.root)
        path = Path(store.resolve(self.sid, result['files'][0])['path'])
        target = self.root / 'elsewhere'; target.write_bytes(b'payload')
        path.unlink(); path.symlink_to(target)
        with self.assertRaises(ValueError): store.resolve(self.sid, result['files'][0])


if __name__ == '__main__':
    unittest.main()
