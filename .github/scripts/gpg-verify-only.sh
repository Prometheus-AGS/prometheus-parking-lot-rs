#!/usr/bin/env bash
# Git's verification-only OpenPGP program. Uses an existing approved home;
# no key import, key lookup, agent startup or global trust/configuration writes.
# https://www.gnupg.org/documentation/manuals/gnupg/GPG-Configuration-Options.html
set -euo pipefail
: "${RELEASE_VERIFY_GNUPGHOME:?existing approved GnuPG home required}"
exec gpg --no-options --batch --no-tty --no-autostart \
  --homedir "$RELEASE_VERIFY_GNUPGHOME" --no-auto-key-retrieve \
  --auto-key-locate clear --no-auto-check-trustdb "$@"
