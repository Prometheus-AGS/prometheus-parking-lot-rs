#!/usr/bin/env bash
# Runs as postgres inside the owned dual-binary image, never on host PGDATA.
set -euo pipefail
export LC_ALL=C
[[ $(cat /upgrade/.oa14-owned) == "${OA14_RUN_ID:?}" ]] || exit 40
[[ ! -e /upgrade/18 && ! -e /upgrade/oa14_upgrade_started ]] || exit 40
old_bin=/usr/lib/postgresql/17/bin
new_bin=/usr/lib/postgresql/18/bin
scripts=/workspace/docker/postgres18/upgrade
mkdir /upgrade/work /upgrade/socket
cd /upgrade/work
"$old_bin/postgres" --version
"$new_bin/postgres" --version
"$new_bin/pg_upgrade" --version
"$old_bin/pg_controldata" /upgrade/17

# The source project PG17 cluster explicitly uses checksums, matching PG18's
# default. No enabling/disabling of checksums in an operator cluster occurs.
"$new_bin/initdb" -D /upgrade/18 --encoding=UTF8 --locale=C \
    --data-checksums --auth-local=trust --auth-host=scram-sha-256
"$new_bin/initdb" -D /upgrade/negative-checksum --encoding=UTF8 --locale=C \
    --no-data-checksums --auth-local=trust --auth-host=scram-sha-256
"$old_bin/initdb" -D /upgrade/negative-version --encoding=UTF8 --locale=C \
    --data-checksums --auth-local=trust --auth-host=scram-sha-256

reject_fixture() {
    local target=$1 expected_exit=$2 oracle=$3
    local observed=0
    bash "$scripts/preflight.sh" /upgrade/17 "$target" \
        >"/receipts/$oracle.stdout.txt" 2>"/receipts/$oracle.stderr.txt" || observed=$?
    [[ "$observed" == "$expected_exit" ]] || {
        echo "OA-14:negative-preflight-wrong-exit $oracle $observed" >&2; exit 45;
    }
    grep -F "OA-14:preflight-$oracle" "/receipts/$oracle.stderr.txt"
    [[ ! -e /upgrade/oa14_upgrade_started ]] || exit 45
    printf 'OA-14:negative-preflight-pass %s exit=%s before-upgrade=true\n' "$oracle" "$observed"
}
reject_fixture /upgrade/negative-version 42 version
reject_fixture /upgrade/negative-checksum 43 checksum
bash "$scripts/preflight.sh" /upgrade/17 /upgrade/18

upgrade=("$new_bin/pg_upgrade" --old-bindir="$old_bin" --new-bindir="$new_bin"
    --old-datadir=/upgrade/17 --new-datadir=/upgrade/18
    --old-port=50417 --new-port=50418 --socketdir=/upgrade/socket --username=postgres --retain)
"${upgrade[@]}" --check
printf 'OA-14:cluster-check-pass\n'
# Copy mode keeps source data independent. Never use --link, --swap or --no-sync.
touch /upgrade/oa14_upgrade_started
"${upgrade[@]}" --copy
printf 'host all all all scram-sha-256\n' >> /upgrade/18/pg_hba.conf
"$new_bin/pg_controldata" /upgrade/18
printf 'OA-14:physical-upgrade-complete schema=legacy-0.3\n'
