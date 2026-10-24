#!/usr/bin/env bash
# No writes or server starts: reject actual incompatible cluster fixtures.
set -euo pipefail
export LC_ALL=C
old_dir=${1:?old directory required}
new_dir=${2:?new directory required}
run_id=${OA14_RUN_ID:?owned run id required}
case "$old_dir:$new_dir" in
    /upgrade/17:/upgrade/*) ;;
    *) echo 'OA-14:unsafe-fixture-path' >&2; exit 40 ;;
esac
[[ $(cat /upgrade/.oa14-owned) == "$run_id" ]] || exit 40
[[ ! -e "$old_dir/postmaster.pid" && ! -e "$new_dir/postmaster.pid" ]] || {
    echo 'OA-14:preflight-running-cluster' >&2; exit 41;
}
[[ $(cat "$old_dir/PG_VERSION") == 17 && $(cat "$new_dir/PG_VERSION") == 18 ]] || {
    echo 'OA-14:preflight-version' >&2; exit 42;
}
old_checksums=$(/usr/lib/postgresql/17/bin/pg_controldata "$old_dir" |
    awk '/Data page checksum version/ {print $NF}')
new_checksums=$(/usr/lib/postgresql/18/bin/pg_controldata "$new_dir" |
    awk '/Data page checksum version/ {print $NF}')
[[ "$old_checksums" =~ ^[01]$ && "$new_checksums" =~ ^[01]$ ]] || exit 44
[[ "$old_checksums" == "$new_checksums" ]] || {
    echo 'OA-14:preflight-checksum' >&2; exit 43;
}
printf 'OA-14:preflight-ok old=17 new=18 checksums=%s\n' "$old_checksums"
