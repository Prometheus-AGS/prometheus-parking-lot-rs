#!/usr/bin/env python3
"""Stage independently approved public SSH trust bytes in an ephemeral release job."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path

from release_evidence import ReleaseError, canonical, require, sha256, validate_policy

TRUST_FILE = Path('/tmp/prometheus-release-trust/allowed_signers')


def stage_public_trust(policy: dict) -> None:
    validate_policy(policy)
    signature = policy.get('signature', {})
    require(signature.get('format') == 'ssh',
            'hosted public-trust setup supports selected SSH mode; OpenPGP still requires preprovisioned public trust')
    require(signature.get('allowed_signers_file') == str(TRUST_FILE),
            'approved fixed hosted SSH trust path required')
    require(signature.get('revocations_file') is None,
            'hosted SSH trust setup cannot omit a configured revocation file')
    raw = os.environ.get('RELEASE_SSH_ALLOWED_SIGNERS', '').encode('utf-8')
    require(raw and len(raw) <= 1024 * 1024 and b'\x00' not in raw,
            'independently configured public SSH allowed-signers bytes required')
    public = {'format': 'ssh', 'fingerprint': signature.get('fingerprint'),
              'identity': signature.get('identity'), 'allowed_signers_sha256': sha256(raw),
              'revocations_sha256': None}
    require(sha256(canonical(public)) == signature['public_trust_sha256'],
            'configured public SSH trust bytes differ from the approved consumer digest')
    require(not TRUST_FILE.parent.is_symlink() and not TRUST_FILE.is_symlink(),
            'public SSH trust path cannot use symlinks')
    TRUST_FILE.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if TRUST_FILE.exists():
        require(TRUST_FILE.is_file() and TRUST_FILE.read_bytes() == raw,
                'preexisting public SSH trust bytes differ from approved bytes')
        return
    with TRUST_FILE.open('xb') as output:
        output.write(raw)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--policy', required=True, type=Path)
    args = parser.parse_args()
    stage_public_trust(json.loads(args.policy.read_bytes()))


if __name__ == '__main__':
    try:
        main()
    except (ReleaseError, OSError, ValueError, KeyError) as error:
        raise SystemExit(f'public release trust staging failed: {error}') from None
