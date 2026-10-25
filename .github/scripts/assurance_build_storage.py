"""Reclaim only freshly leased OA-12 build scratch after a hosted row exits.

The caller must wait for the entire row subprocess before finish_case. Raw
commands, oracle receipts and logs live outside the checkout and are never removed.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import shutil
import stat
import uuid

ROOT = Path(__file__).resolve().parents[2]
MARKER = '.assurance-build-owner.json'


def _context() -> str | None:
    if (os.environ.get('GITHUB_ACTIONS') != 'true'
            or os.environ.get('BOOTSTRAP_RUNNER_ENVIRONMENT') != 'github-hosted'):
        return None
    run = os.environ.get('GITHUB_RUN_ID', '')
    if not re.fullmatch(r'[0-9]+', run) or os.environ.get('GITHUB_WORKSPACE') != str(ROOT):
        raise ValueError('hosted build-storage context does not match this checkout')
    return run


def _directory(path: Path, create: bool = False) -> int:
    """Open each absolute component without following redirected parents."""
    fd = os.open(path.anchor, os.O_RDONLY | os.O_DIRECTORY)
    try:
        for component in path.parts[1:]:
            if component in ('.', '..'):
                raise ValueError('unsafe directory component')
            if create:
                try:
                    os.mkdir(component, dir_fd=fd)
                except FileExistsError:
                    pass
            child = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
            os.close(fd)
            fd = child
        return fd
    except BaseException:
        os.close(fd)
        raise


def _tree_bytes(fd: int, device: int) -> int:
    """Refuse symlinks/mount redirects and count allocated scratch bytes."""
    total = 0
    with os.scandir(fd) as entries:
        for entry in entries:
            metadata = entry.stat(follow_symlinks=False)
            if metadata.st_dev != device or stat.S_ISLNK(metadata.st_mode):
                raise ValueError('redirected build scratch descendant')
            if stat.S_ISDIR(metadata.st_mode):
                child = os.open(entry.name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
                try:
                    total += _tree_bytes(child, device)
                finally:
                    os.close(child)
            elif stat.S_ISREG(metadata.st_mode):
                total += metadata.st_blocks * 512
            else:
                raise ValueError('unexpected build scratch descendant type')
    return total


def begin_case(case_id: str, candidate: str, output: Path) -> dict | None:
    """Claim a fresh exact case directory; refuse existing or redirected scratch."""
    run = _context()
    if run is None:
        return None
    manifest = json.loads((ROOT / '.github/negative-controls.json').read_bytes())
    if (not isinstance(case_id, str) or not re.fullmatch(r'[a-z0-9]+(?:-[a-z0-9]+)*', case_id)
            or case_id not in {case['id'] for case in manifest['cases']}):
        raise ValueError('unknown or unsafe negative-control case ID')
    if not isinstance(candidate, str) or not candidate:
        raise ValueError('candidate identity is required')
    output = Path(output).resolve()
    if output.is_relative_to(ROOT):
        raise ValueError('raw evidence output must be outside checkout scratch')
    if not shutil.rmtree.avoids_symlink_attacks:
        raise ValueError('safe directory-relative removal is unavailable')
    parent = _directory(ROOT / 'target/oa12', create=True)
    try:
        os.mkdir(case_id, dir_fd=parent)
        fd = os.open(case_id, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
        try:
            metadata = os.fstat(fd)
            lease = {'case_id': case_id, 'candidate': candidate, 'run_id': run,
                     'output': str(output), 'root': str(ROOT), 'nonce': uuid.uuid4().hex,
                     'device': metadata.st_dev, 'inode': metadata.st_ino}
            marker = os.open(MARKER, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                             0o600, dir_fd=fd)
            with os.fdopen(marker, 'w') as stream:
                json.dump(lease, stream, sort_keys=True)
            return lease
        finally:
            os.close(fd)
    finally:
        os.close(parent)


def finish_case(lease: dict | None, exit_code: object, *, row_passed: bool = False) -> dict:
    """Reclaim only after exit zero and the caller validates the row PASS record."""
    receipt = {'status': 'DISABLED' if lease is None else 'NOT_RECLAIMED',
               'exit_code': exit_code, 'row_passed': row_passed, 'reclaimed_bytes': 0}
    if lease is None:
        return receipt
    receipt['owner'] = dict(lease)
    if type(exit_code) is not int or exit_code != 0 or row_passed is not True:
        receipt['reason'] = 'row lacks both a completed zero exit and a validated PASS record'
        return receipt
    parent = fd = None
    try:
        if (_context() != lease['run_id'] or lease['root'] != str(ROOT)
                or not re.fullmatch(r'[a-z0-9]+(?:-[a-z0-9]+)*', lease['case_id'])):
            raise ValueError('lease context changed')
        parent = _directory(ROOT / 'target/oa12')
        fd = os.open(lease['case_id'], os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
        metadata = os.fstat(fd)
        if (metadata.st_dev, metadata.st_ino) != (lease['device'], lease['inode']):
            raise ValueError('claimed scratch directory was replaced')
        marker = os.open(MARKER, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=fd)
        with os.fdopen(marker) as stream:
            if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
                raise ValueError('ownership marker is not a regular file')
            if json.load(stream) != lease:
                raise ValueError('scratch ownership marker does not match lease')
        allocated = _tree_bytes(fd, metadata.st_dev)
        receipt['free_bytes_before'] = shutil.disk_usage(ROOT).free
        shutil.rmtree(lease['case_id'], dir_fd=parent)
        receipt.update(status='RECLAIMED', reclaimed_bytes=allocated,
                       free_bytes_after=shutil.disk_usage(ROOT).free)
    except (OSError, ValueError, KeyError) as error:
        receipt['reason'] = str(error)
    finally:
        if fd is not None:
            os.close(fd)
        if parent is not None:
            os.close(parent)
    return receipt
