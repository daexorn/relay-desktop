"""Bounded, binary attachment uploads over a dedicated authenticated SSH channel."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import sys
import uuid

MAX_BYTES = 512 * 1024 * 1024
CHUNK = 256 * 1024
MANIFEST = '.relay-upload.json'


def ident(value):
    if not isinstance(value, str) or str(uuid.UUID(value)) != value:
        raise ValueError('Некорректный идентификатор вложения.')
    return value


def file_name(value):
    name = str(value).replace('\\', '/').split('/')[-1]
    name = ''.join(c for c in name if ord(c) >= 32 and c != '\x7f')
    name = name.encode('utf-8')[:180].decode('utf-8', errors='ignore')
    if not name or name in ('.', '..'):
        raise ValueError('Некорректное имя файла.')
    return name


class UploadStore:
    def __init__(self, root=None):
        self.root = Path(root) if root else Path.home() / '.cache/relay-desktop/attachments'

    def resolve(self, sid, source):
        transfer = ident(source.get('upload_id'))
        index = source.get('index')
        if type(index) is not int or not 0 <= index < 8:
            raise ValueError('Некорректное вложение.')
        folder = self.root / ident(sid) / transfer
        manifest = folder / MANIFEST
        if folder.is_symlink() or not manifest.is_file() or manifest.stat().st_size > 16384:
            raise ValueError('Файл не загружен. Прикрепи его ещё раз.')
        if not folder.resolve().is_relative_to((self.root / sid).resolve()):
            raise ValueError('Некорректный каталог вложения.')
        saved = json.loads(manifest.read_text(encoding='utf-8'))
        if not isinstance(saved, list) or index >= len(saved):
            raise ValueError('Файл не загружен.')
        item = saved[index]
        name = file_name(item['name'])
        path = folder / f'{index:02d}-{name}'
        info = path.lstat()
        if not stat.S_ISREG(info.st_mode) or not 0 <= info.st_size <= MAX_BYTES or info.st_size != item['bytes']:
            raise ValueError('Загруженный файл изменился. Прикрепи его ещё раз.')
        if source.get('bytes') != item['bytes'] or source.get('sha256') != item['sha256']:
            raise ValueError('Данные вложения не совпадают.')
        if not re.fullmatch('[0-9a-f]{64}', item['sha256']):
            raise ValueError('Некорректная контрольная сумма.')
        return {'name': name, 'path': str(path), 'bytes': info.st_size, 'sha256': item['sha256']}


def receive(reader, writer, root=None):
    """A small JSON header, exact binary bodies, then a checksum footer. No base64."""
    staging = None
    try:
        header = reader.readline(16385)
        if len(header) > 16384 or not header.endswith(b'\n'):
            raise ValueError('Некорректный заголовок передачи.')
        request = json.loads(header)
        sid, transfer = ident(request.get('sid')), ident(request.get('upload_id'))
        files = request.get('files')
        if not isinstance(files, list) or not 1 <= len(files) <= 8:
            raise ValueError('До восьми файлов в сообщении.')
        names, sizes = [], []
        for item in files:
            name, size = file_name(item['name']), item['bytes']
            if type(size) is not int or not 0 <= size <= MAX_BYTES:
                raise ValueError('Размер вложений — до 512 МБ.')
            names.append(name); sizes.append(size)
        if sum(sizes) > MAX_BYTES:
            raise ValueError('Общий размер вложений — до 512 МБ.')
        store = UploadStore(root)
        parent = store.root / sid
        parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        destination = parent / transfer
        if destination.exists():
            raise ValueError('Этот идентификатор передачи уже использован.')
        staging = parent / ('.' + transfer + '.partial')
        staging.mkdir(mode=0o700)
        writer.write(json.dumps({'ready': True}) + '\n'); writer.flush()
        saved = []
        for index, (name, size) in enumerate(zip(names, sizes)):
            path = staging / f'{index:02d}-{name}'
            digest = hashlib.sha256()
            with path.open('xb') as output:
                path.chmod(0o600)
                remaining = size
                while remaining:
                    block = reader.read(min(CHUNK, remaining))
                    if not block:
                        raise ValueError('Передача файла прервалась.')
                    output.write(block); digest.update(block); remaining -= len(block)
            saved.append({'name': name, 'bytes': size, 'sha256': digest.hexdigest()})
        footer = reader.readline(16385)
        if len(footer) > 16384 or not footer.endswith(b'\n'):
            raise ValueError('Передача файла не завершена.')
        checksums = json.loads(footer).get('sha256')
        if checksums != [item['sha256'] for item in saved]:
            raise ValueError('Контрольная сумма файла не совпала. Повтори отправку.')
        manifest = staging / MANIFEST
        manifest.write_text(json.dumps(saved, ensure_ascii=False), encoding='utf-8')
        manifest.chmod(0o600)
        os.rename(staging, destination)
        staging = None
        result = [dict(item, type='uploaded_file', upload_id=transfer, index=index)
                  for index, item in enumerate(saved)]
        writer.write(json.dumps({'ok': True, 'files': result}, ensure_ascii=False) + '\n')
        writer.flush()
        return True
    except (ValueError, KeyError, TypeError, AttributeError, OSError) as error:
        writer.write(json.dumps({'ok': False, 'message': str(error)[:800]}, ensure_ascii=False) + '\n')
        writer.flush()
        return False
    finally:
        if staging is not None:
            shutil.rmtree(staging, ignore_errors=True)


if __name__ == '__main__':
    sys.exit(0 if receive(sys.stdin.buffer, sys.stdout) else 1)
