#!/usr/bin/env python3
"""Verify an operator-authorized annotated tag at one frozen full commit ID.

Uses Git's real verifier. SSH authorization pins both principal and fingerprint;
OpenPGP authorization pins the primary fingerprint from GnuPG's VALIDSIG status.
No private key, key import, host configuration or network lookup is required.
"""

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys


class VerificationFailure(Exception):
    """A missing verification prerequisite or failed authorization oracle."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise VerificationFailure(message)


def run_git(args: list[str], env: dict[str, str]) -> bytes:
    result = subprocess.run(["git", *args], env=env, capture_output=True, check=False)
    require(result.returncode == 0, "Git object/ref inspection failed")
    return result.stdout


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True, help="exact short tag name, not a revision expression")
    parser.add_argument("--candidate", required=True, help="frozen full lowercase commit object ID")
    parser.add_argument("--format", required=True, choices=("ssh", "openpgp"))
    parser.add_argument("--fingerprint", required=True, help="SSH SHA256 or OpenPGP primary fingerprint")
    parser.add_argument("--identity", help="exact authorized SSH principal")
    parser.add_argument("--allowed-signers", type=Path, help="operator-approved public SSH trust file")
    parser.add_argument("--revocations", type=Path, help="optional operator-approved SSH revocation file")
    parser.add_argument("--gnupg-home", type=Path, help="absolute existing operator-approved GnuPG home")
    args = parser.parse_args()
    env = {**os.environ, "GIT_NO_REPLACE_OBJECTS": "1", "LC_ALL": "C"}
    require(re.fullmatch(r"(?:[0-9a-f]{40}|[0-9a-f]{64})", args.candidate) is not None,
            "candidate must be a full lowercase commit ID")
    require(not args.tag.startswith("refs/"), "tag must be an exact short tag name")
    ref = f"refs/tags/{args.tag}"
    run_git(["check-ref-format", ref], env)
    require(run_git(["cat-file", "-t", args.candidate], env).strip() == b"commit",
            "candidate is not a commit")
    tag_oid = run_git(["show-ref", "--verify", "--hash", ref], env).decode("ascii").strip()
    require(run_git(["cat-file", "-t", tag_oid], env).strip() == b"tag",
            "annotated tag required; lightweight tags are not authorized")
    payload = run_git(["cat-file", "tag", tag_oid], env)
    header, separator, body = payload.partition(b"\n\n")
    require(bool(separator), "malformed annotated tag")
    headers = header.splitlines()
    require(headers[:3] == [f"object {args.candidate}".encode("ascii"), b"type commit",
                            f"tag {args.tag}".encode("utf-8")],
            "tag target or internal name does not match frozen candidate/tag")

    options: list[str] = []
    if args.format == "ssh":
        require(b"-----BEGIN SSH SIGNATURE-----\n" in body,
                "SSH signed annotated tag required; unsigned or different signature format")
        require(bool(args.identity) and not any(ord(c) < 32 for c in args.identity),
                "explicit SSH signing identity required")
        require(re.fullmatch(r"SHA256:[A-Za-z0-9+/]{43}", args.fingerprint) is not None,
                "full SSH SHA256 fingerprint required")
        require(args.allowed_signers is not None and args.allowed_signers.is_file(),
                "operator SSH allowed-signers file is unavailable")
        ssh_keygen = shutil.which("ssh-keygen")
        require(ssh_keygen is not None, "ssh-keygen is unavailable")
        options = ["-c", f"gpg.ssh.program={ssh_keygen}", "-c", "gpg.minTrustLevel=fully",
                   "-c", f"gpg.ssh.allowedSignersFile={args.allowed_signers.resolve()}"]
        if args.revocations is not None:
            require(args.revocations.is_file(), "SSH revocation file is unavailable")
            options += ["-c", f"gpg.ssh.revocationFile={args.revocations.resolve()}"]
    else:
        require(b"-----BEGIN PGP SIGNATURE-----\n" in body,
                "OpenPGP signed annotated tag required; unsigned or different signature format")
        require(re.fullmatch(r"(?:[0-9A-Fa-f]{40}|[0-9A-Fa-f]{64})", args.fingerprint) is not None,
                "full OpenPGP primary fingerprint required")
        require(args.gnupg_home is not None and args.gnupg_home.is_absolute() and args.gnupg_home.is_dir(),
                "operator GnuPG home is unavailable")
        require(shutil.which("gpg") is not None, "gpg is unavailable")
        wrapper = Path(__file__).resolve().with_name("gpg-verify-only.sh")
        require(wrapper.is_file() and os.access(wrapper, os.X_OK), "GPG verification wrapper is unavailable")
        env["RELEASE_VERIFY_GNUPGHOME"] = str(args.gnupg_home.resolve())
        # Explicit fingerprint authorization replaces reliance on local UID
        # ownertrust. Bad/revoked/expired statuses are rejected independently.
        options = ["-c", f"gpg.openpgp.program={wrapper}", "-c", f"gpg.program={wrapper}",
                   "-c", "gpg.minTrustLevel=undefined"]

    result = subprocess.run(["git", *options, "verify-tag", "--raw", tag_oid],
                            env=env, capture_output=True, check=False)
    require(result.returncode == 0, "cryptographic tag verification failed")
    status = (result.stdout + result.stderr).decode("utf-8", errors="strict")
    if args.format == "ssh":
        # Git/OpenSSH's first success line includes both principal and key.
        # Match a whole line, never a tag message, UID substring or key comment.
        matches = re.findall(r'^Good "git" signature for (.+) with \S+ key (SHA256:[A-Za-z0-9+/]{43})$',
                             status, flags=re.MULTILINE)
        require(len(matches) == 1, "SSH verifier did not provide one trusted signing identity")
        identity, fingerprint = matches[0]
        require(identity == args.identity, "SSH signing identity mismatch")
        require(fingerprint == args.fingerprint, "SSH signing fingerprint mismatch")
    else:
        records = [line.split()[1:] for line in status.splitlines() if line.startswith("[GNUPG:] ")]
        forbidden = {"BADSIG", "ERRSIG", "EXPSIG", "EXPKEYSIG", "REVKEYSIG", "KEYREVOKED",
                     "KEYEXPIRED", "SIGEXPIRED", "NO_PUBKEY", "NODATA", "FAILURE", "BADARMOR"}
        require(not any(record and record[0] in forbidden for record in records),
                "OpenPGP signature/key is invalid, expired or revoked")
        valid = [record for record in records if record and record[0] == "VALIDSIG"]
        require(len(valid) == 1 and len(valid[0]) >= 10,
                "OpenPGP verifier did not provide one VALIDSIG fingerprint")
        fingerprint = valid[0][1]
        identity = valid[0][10] if len(valid[0]) >= 11 else fingerprint
        require(identity.upper() == args.fingerprint.upper(), "OpenPGP signing identity mismatch")
    require(run_git(["show-ref", "--verify", "--hash", ref], env).decode("ascii").strip() == tag_oid,
            "tag ref moved during verification")
    print(json.dumps({"status": "PASS", "tag": args.tag, "tag_object": tag_oid,
                      "candidate": args.candidate, "format": args.format,
                      "identity": identity, "signing_fingerprint": fingerprint}, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except (VerificationFailure, OSError, UnicodeError) as error:
        # Never dump signed payloads, private material, environment or tool output.
        print(f"verify-release-tag: {error}", file=sys.stderr)
        sys.exit(1)
