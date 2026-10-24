# PR94 CI repair attempt1

Phase Execute; change oa-19-ci-repairs; backend task1 active at revision1126. Source base52d4363824b5619dbe11bd2b1c2a6193381e7a99. Credential rotation remains deferred. No credential/settings/history/publication action.

Observed GitHub CI40120299335 failures: PG17 job120950183711 and PG18 job120950183741 report sequence composite-type SQL42809 in seven mailbox snapshot tests; OA07 PG normal controls fail on the same snapshot. Browser jobs120950183806/120950183779 and s4-browser120950209477 lack configured absolute executable paths. Platform40120299306 job120950184054 lacks webkit2gtk-4.1. Crash-control discovery jobs120950211659/120950211822/120950211802/120950211935 use pg module names whereas actual discovery reports postgres.

Owned patch imports verified base/target hashes: PG f71a4be9ea63430aab98927ddfb904dd64bc005ceba4d43e99664070c699a7f2; platform5f814e4733a4c1c9d4f82acf260707a20b37d33294b04fa0267a777af1ee93ac. Two test SQL snapshots explicitly encode all three sequence state fields. Hosted CI explicitly provisions existing pinned browser and Tauri prerequisites. Ten stale test identities corrected across four cases. No production scheduler/store change or assertion weakening. New bootstrap guards protect the real downloaded-executable and operator-configured-runner boundaries. No unsolicited feature or dependency upgrade added.

Local coherent boundary commands observed:
- git diff --check: exit0.
- private Rust1.99.1 cargo fmt --all -- --check: exit0.
- Python ast.parse new bootstrap: PASS.
- PyYAML safe_load both changed workflows: PASS,20/4jobs (syntax only, not Actions expression validation).
- python3 .github/scripts/tests/negative-controls.py matrix: exit0,52 cases, source-derived inventory matches.
- SHA256 verification:47 OA14 candidate_sources and100 OA12 source_sha256 entries match; new bootstrap included; OA14 refreshed before OA12.
- Original /tmp/oa04-preparation-72adf5d/release-plan.json: all13 files_sha256 match; original release preparation preserved.

Raw logs/artifacts and owner handoffs retained privately under /Users/gqadonis/.local/state/prometheus/operational-assurance/pr94-ci-repair, oa19-postgres, and oa19-platform-handoff. Official CfT exact-version metadata and both archive HEADs returned200; this is availability evidence only. No local Linux/PG production pass claimed. Independent narrow review and actual repaired-head CI are pending. Earlier candidate180a receipts retain their original identities; normal PRCI cannot certify the deferred release packet.

Independent fresh-context narrow Rust and Python/workflow reviewers returned no concrete correctness/security findings; complete helpers, assertions and validator wiring read. Same-family static review only, no release authority. Additional observed s21-browser job120950212994 reports the same missing absolute Chrome path, covered by shared hosted setup. Full updated-head production CI remains pending.
