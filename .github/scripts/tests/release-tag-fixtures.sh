#!/usr/bin/env bash
# OA-03 real Git SSH/OpenPGP fixture source. Execute only at the final boundary:
#   bash .github/scripts/tests/release-tag-fixtures.sh
# G-03 permits throwaway test identities. This script then creates keys ONLY
# under its owned temporary root, never imports/copies a private key, contacts a
# keyserver, uses host trust/agents, creates a release tag or publishes anything.
set -euo pipefail
source_root=$(cd "$(dirname "$0")/../../.." && pwd)
verifier="$source_root/.github/scripts/verify-release-tag.py"
for tool in git python3 ssh-keygen gpg gpgconf; do
  command -v "$tool" >/dev/null || { echo "BLOCKED: $tool is required" >&2; exit 2; }
done
umask 077
fixture_root=$(mktemp -d "${TMPDIR:-/tmp}/oa03-release-tags.XXXXXX")
gnupg_home="$fixture_root/gnupg"
cleanup() {
  local status=$?
  trap - EXIT
  if [ -d "$gnupg_home" ]; then
    if ! gpgconf --homedir "$gnupg_home" --kill gpg-agent >/dev/null 2>&1; then
      echo 'FAIL: could not stop the fixture-owned GPG agent' >&2
      status=1
    fi
  fi
  rm -rf -- "$fixture_root" || status=1
  exit "$status"
}
trap cleanup EXIT
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES
unset SSH_AUTH_SOCK SSH_AGENT_PID GPG_AGENT_INFO
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
export GIT_AUTHOR_NAME='Tag fixture' GIT_COMMITTER_NAME='Tag fixture'
export GIT_AUTHOR_EMAIL='fixture@example.invalid' GIT_COMMITTER_EMAIL='fixture@example.invalid'
export GNUPGHOME="$gnupg_home" GIT_TERMINAL_PROMPT=0
export GIT_TEMPLATE_DIR="$fixture_root/empty-template" LC_ALL=C
mkdir "$GIT_TEMPLATE_DIR" "$gnupg_home"
repo="$fixture_root/repo"
git init -q "$repo"
printf 'First fixture commit\n' > "$repo/README.md"
git -C "$repo" add .
git -C "$repo" -c core.hooksPath=/dev/null -c commit.gpgsign=false commit -qm 'Frozen fixture candidate'
candidate=$(git -C "$repo" rev-parse HEAD)
printf 'Other fixture commit\n' >> "$repo/README.md"
git -C "$repo" add .
git -C "$repo" -c core.hooksPath=/dev/null -c commit.gpgsign=false commit -qm 'Other fixture candidate'
other=$(git -C "$repo" rev-parse HEAD)
checks=0

expect_verifier() {
  local label="$1" expected="$2" message="$3" actual=0 log
  shift 3
  checks=$((checks + 1))
  log="$fixture_root/check-$checks.txt"
  printf '\nfixture %s: %s; expected exit %s\n' "$checks" "$label" "$expected"
  printf 'cwd: %s\ncommand: python3 %q' "$repo" "$verifier"
  printf ' %q' "$@"
  printf '\n'
  (cd "$repo" && python3 "$verifier" "$@") > "$log" 2>&1 || actual=$?
  cat "$log"
  if [ "$actual" -ne "$expected" ] || ! grep -Fq -- "$message" "$log"; then
    echo "FAIL: $label (exit $actual; required oracle: $message)" >&2
    exit 1
  fi
  echo "PASS: $label (exit $actual; oracle matched)"
}

# Only this deferred fixture invocation generates signing keys. No ssh-agent is
# used: Git signs with each temporary unencrypted fixture key file directly.
ssh-keygen -q -t ed25519 -N '' -C 'approved-fixture@example.invalid' -f "$fixture_root/ssh-approved"
ssh-keygen -q -t ed25519 -N '' -C 'other-fixture@example.invalid' -f "$fixture_root/ssh-other"
ssh_fp=$(ssh-keygen -lf "$fixture_root/ssh-approved.pub" -E sha256 | awk '{print $2}')
other_ssh_fp=$(ssh-keygen -lf "$fixture_root/ssh-other.pub" -E sha256 | awk '{print $2}')
printf 'approved-fixture@example.invalid %s\n' "$(cat "$fixture_root/ssh-approved.pub")" > "$fixture_root/allowed-signers"
printf 'other-fixture@example.invalid %s\n' "$(cat "$fixture_root/ssh-other.pub")" >> "$fixture_root/allowed-signers"
ssh_args=(--format ssh --identity approved-fixture@example.invalid --fingerprint "$ssh_fp" --allowed-signers "$fixture_root/allowed-signers")
git -C "$repo" -c gpg.format=ssh -c user.signingkey="$fixture_root/ssh-approved" tag -s oa03-ssh-valid "$candidate" -m 'Fixture signed tag'
git -C "$repo" -c gpg.format=ssh -c user.signingkey="$fixture_root/ssh-approved" tag -s oa03-ssh-wrong-commit "$other" -m 'Fixture signed tag'
git -C "$repo" -c gpg.format=ssh -c user.signingkey="$fixture_root/ssh-other" tag -s oa03-ssh-other-identity "$candidate" -m 'Fixture signed tag'
git -C "$repo" -c tag.gpgsign=false tag -a oa03-unsigned "$candidate" -m 'Unsigned fixture tag'
git -C "$repo" -c tag.gpgsign=false tag oa03-lightweight "$candidate"
expect_verifier 'authorized SSH tag at frozen candidate' 0 '"status": "PASS"' --tag oa03-ssh-valid --candidate "$candidate" "${ssh_args[@]}"
expect_verifier 'unsigned annotated tag rejected' 1 'unsigned or different signature format' --tag oa03-unsigned --candidate "$candidate" "${ssh_args[@]}"
expect_verifier 'lightweight tag rejected' 1 'annotated tag required' --tag oa03-lightweight --candidate "$candidate" "${ssh_args[@]}"
expect_verifier 'valid SSH signature at wrong commit rejected' 1 'does not match frozen candidate/tag' --tag oa03-ssh-wrong-commit --candidate "$candidate" "${ssh_args[@]}"
expect_verifier 'trusted but unauthorized SSH identity rejected' 1 'SSH signing identity mismatch' --tag oa03-ssh-other-identity --candidate "$candidate" "${ssh_args[@]}"
expect_verifier 'different SSH fingerprint rejected' 1 'SSH signing fingerprint mismatch' --tag oa03-ssh-valid --candidate "$candidate" "${ssh_args[@]}" --fingerprint "$other_ssh_fp"
expect_verifier 'missing SSH trust rejected' 1 'allowed-signers file is unavailable' --tag oa03-ssh-valid --candidate "$candidate" "${ssh_args[@]}" --allowed-signers "$fixture_root/absent"
expect_verifier 'abbreviated candidate rejected' 1 'full lowercase commit ID' --tag oa03-ssh-valid --candidate "${candidate:0:7}" "${ssh_args[@]}"

signed_oid=$(git -C "$repo" rev-parse refs/tags/oa03-ssh-valid)
git -C "$repo" update-ref refs/tags/oa03-renamed "$signed_oid"
expect_verifier 'renamed signed tag object rejected' 1 'does not match frozen candidate/tag' --tag oa03-renamed --candidate "$candidate" "${ssh_args[@]}"
tampered_oid=$(git -C "$repo" cat-file tag "$signed_oid" | sed 's/Fixture signed tag/Fixture altered tag/' | git -C "$repo" hash-object -t tag -w --stdin)
git -C "$repo" update-ref refs/tags/oa03-ssh-valid "$tampered_oid" "$signed_oid"
expect_verifier 'tampered SSH signature rejected cryptographically' 1 'cryptographic tag verification failed' --tag oa03-ssh-valid --candidate "$candidate" "${ssh_args[@]}"
git -C "$repo" update-ref refs/tags/oa03-ssh-valid "$signed_oid" "$tampered_oid"

# Generate two disposable OpenPGP authorities and signing subkeys in this one
# owned GNUPGHOME. The verifier must authorize the primary fingerprint while
# Git/GPG verifies the actual signing subkey. No key imports or host trust edits.
for name in approved other; do
  gpg --no-options --batch --pinentry-mode loopback --passphrase '' \
    --quick-generate-key "$name-fixture@example.invalid" ed25519 cert 0 > "$fixture_root/gpg-$name-generate.txt" 2>&1
  fingerprint=$(gpg --no-options --batch --with-colons --list-keys "$name-fixture@example.invalid" 2>/dev/null | awk -F: '$1 == "fpr" {print $10; exit}')
  [ -n "$fingerprint" ] || { echo 'FAIL: missing fixture primary fingerprint' >&2; exit 1; }
  gpg --no-options --batch --pinentry-mode loopback --passphrase '' \
    --quick-add-key "$fingerprint" ed25519 sign 0 > "$fixture_root/gpg-$name-subkey.txt" 2>&1
  if [ "$name" = approved ]; then gpg_fp="$fingerprint"; else other_gpg_fp="$fingerprint"; fi
done
gpg_args=(--format openpgp --fingerprint "$gpg_fp" --gnupg-home "$gnupg_home")
git -C "$repo" -c gpg.format=openpgp -c gpg.program=gpg -c user.signingkey="$gpg_fp" tag -s oa03-gpg-valid "$candidate" -m 'Fixture signed tag'
git -C "$repo" -c gpg.format=openpgp -c gpg.program=gpg -c user.signingkey="$gpg_fp" tag -s oa03-gpg-wrong-commit "$other" -m 'Fixture signed tag'
git -C "$repo" -c gpg.format=openpgp -c gpg.program=gpg -c user.signingkey="$other_gpg_fp" tag -s oa03-gpg-other-identity "$candidate" -m 'Fixture signed tag'
expect_verifier 'authorized OpenPGP primary identity and signing subkey' 0 '"status": "PASS"' --tag oa03-gpg-valid --candidate "$candidate" "${gpg_args[@]}"
expect_verifier 'trusted but unauthorized OpenPGP identity rejected' 1 'OpenPGP signing identity mismatch' --tag oa03-gpg-other-identity --candidate "$candidate" "${gpg_args[@]}"
expect_verifier 'valid OpenPGP signature at wrong commit rejected' 1 'does not match frozen candidate/tag' --tag oa03-gpg-wrong-commit --candidate "$candidate" "${gpg_args[@]}"
expect_verifier 'unsigned tag rejected in OpenPGP mode' 1 'unsigned or different signature format' --tag oa03-unsigned --candidate "$candidate" "${gpg_args[@]}"
expect_verifier 'signature format substitution rejected' 1 'unsigned or different signature format' --tag oa03-ssh-valid --candidate "$candidate" "${gpg_args[@]}"
expect_verifier 'missing OpenPGP trust home rejected' 1 'GnuPG home is unavailable' --tag oa03-gpg-valid --candidate "$candidate" "${gpg_args[@]}" --gnupg-home "$fixture_root/absent"

printf '\nPASS: OA-03 release-tag fixture oracles matched (%s checks; real Git/SSH/OpenPGP, temporary identities only)\n' "$checks"
