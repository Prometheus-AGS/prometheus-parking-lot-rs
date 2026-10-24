#!/usr/bin/env python3
"""OA-14 boundary runner. Never imports Docker/Python packages or installs tools."""

import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time


class Blocked(RuntimeError):
    """A required local tool/image is unavailable; no product PASS is possible."""


def adopted_contract_identity(path, expected_digest):
    """Match OA18 contract()/canonical(): compact sorted UTF-8 JSON plus LF."""
    raw = path.read_bytes()
    data = json.loads(raw)
    if not isinstance(data, dict):
        raise RuntimeError("OA-14:contract-identity changed")
    identity = data.get("identity")
    expected_identity = {
        "projectId": "f6c384db-6ee1-4f1b-9bab-0e1602ec2615",
        "runId": "phase-certification-remediation-20261022T103344Z",
        "phaseId": "phase-0-4-operational-assurance",
        "planRevision": 1,
    }
    if (data.get("kind") != "kbd.adopted-contract" or data.get("contractVersion") != "1"
            or not isinstance(identity, dict)
            or any(identity.get(key) != value for key, value in expected_identity.items())
            or type(identity.get("planRevision")) is not int):
        raise RuntimeError("OA-14:contract-identity changed")
    normalized = copy.deepcopy(data)
    normalized["identity"].pop("contractDigest", None)
    normalized["identity"].pop("planRevision", None)
    canonical = (json.dumps(normalized, sort_keys=True, separators=(",", ":"),
                            ensure_ascii=False) + "\n").encode()
    digest = hashlib.sha256(canonical).hexdigest()
    if digest != identity.get("contractDigest") or digest != expected_digest:
        raise RuntimeError("OA-14:contract-changed")
    return {"contract_sha256": digest, "contract_identity": identity,
            "contract_raw_sha256": hashlib.sha256(raw).hexdigest()}


class Rehearsal:
    def __init__(self, args):
        self.args = args
        self.root = Path(__file__).resolve().parents[3]
        self.out = Path(args.out)
        if not self.out.is_absolute() or self.out.exists():
            raise ValueError("--out must be a new absolute directory")
        self.out.mkdir()
        (self.out / "cluster").mkdir(mode=0o777)
        (self.out / "cluster").chmod(0o777)
        self.steps = []
        self.prefix = "oa14-" + args.run_id
        self.work = self.prefix + "-work"
        self.backup = self.prefix + "-backup"
        self.old = self.prefix + "-pg17"
        self.new = self.prefix + "-pg18"
        self.label = "ai.prometheus.oa14.run=" + args.run_id
        self.resources = {"containers": [], "volumes": [], "retain_volumes": True}
        self.receipt = {
            "scenario": "OA-14", "candidate": args.candidate,
            "requested_contract_sha256": args.contract_sha256,
            "contract_sha256": None, "run_id": args.run_id,
            "control": args.control, "status": "NOT_EXERCISED",
            "steps": self.steps, "resources": self.resources,
        }

    def save(self):
        (self.out / "receipt.json").write_text(json.dumps(self.receipt, indent=2) + "\n")

    def run(self, name, argv, *, timeout=120, data=None, env=None, allowed=(0,)):
        if argv[:2] == ["docker", "run"] and "--name" not in argv:
            # A timeout kills the CLI, not necessarily its daemon-side process.
            # Name and record even --rm containers so cleanup has exact handles.
            temporary = self.prefix + "-tmp-" + str(len(self.steps))
            self.owned_container(temporary)
            argv = [*argv[:2], "--name", temporary, *argv[2:]]
        index = len(self.steps)
        stdout = self.out / f"{index:03d}-{name}.stdout.txt"
        stderr = self.out / f"{index:03d}-{name}.stderr.txt"
        step = {"name": name, "argv": argv, "started": time.time(),
                "stdout": stdout.name, "stderr": stderr.name, "timeout_seconds": timeout}
        self.steps.append(step)
        self.save()
        try:
            with stdout.open("wb") as out, stderr.open("wb") as err:
                kwargs = {"input": data} if data is not None else {"stdin": subprocess.DEVNULL}
                result = subprocess.run(argv, cwd=self.root, env=env, stdout=out, stderr=err,
                                        timeout=timeout, check=False, **kwargs)
            step["exit"] = result.returncode
        except FileNotFoundError as error:
            step["exit"] = "UNAVAILABLE"
            raise Blocked(str(error)) from error
        except subprocess.TimeoutExpired as error:
            step["exit"] = "TIMEOUT"
            raise RuntimeError(f"OA-14:timeout {name}") from error
        finally:
            step["finished"] = time.time()
            self.save()
        if result.returncode not in allowed:
            raise RuntimeError(f"OA-14:step-failed {name} exit={result.returncode}; see raw logs")
        return result.returncode, stdout.read_text(errors="replace"), stderr.read_text(errors="replace")

    def inspect_image(self, name, image, major=None):
        code, output, _ = self.run(name, ["docker", "image", "inspect", image, "--format",
            '{{json .Id}} {{json .Architecture}} {{json .RepoDigests}}'], allowed=(0, 1))
        if code:
            raise Blocked(f"required prebuilt immutable image unavailable: {image}")
        if not output.startswith('"' + image + '"'):
            raise RuntimeError("OA-14:image-id mismatch")
        if major:
            _, label, _ = self.run(name + "-label", ["docker", "image", "inspect", image,
                "--format", '{{index .Config.Labels "ai.prometheus.pg.major"}}'])
            if label.strip() != major:
                raise RuntimeError("OA-14:image-major-label mismatch")

    def verify_inputs(self):
        for name in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS"]:
            if "parking_lot_negative_control" in os.environ.get(name, ""):
                raise RuntimeError("OA-14:combined-negative-control refused: " + name)
        if self.args.control != "none" and os.environ.get("CARGO_ENCODED_RUSTFLAGS"):
            raise RuntimeError("OA-14:encoded-rustflags would override the named control")
        _, head, _ = self.run("candidate", ["git", "rev-parse", "HEAD"])
        if head.strip() != self.args.candidate:
            raise RuntimeError("OA-14:candidate-changed")
        contract = self.root / ".kbd-orchestrator/phases/phase-0-4-operational-assurance/adopted-contract.json"
        self.receipt.update(adopted_contract_identity(contract, self.args.contract_sha256))
        fixture = self.root / "crates/parking-lot-pg/tests/fixtures/oa14"
        manifest = json.loads((fixture / "manifest.json").read_text())
        for name, digest in manifest["files"].items():
            if hashlib.sha256((fixture / name).read_bytes()).hexdigest() != digest:
                raise RuntimeError("OA-14:fixture-digest mismatch: " + name)
        self.receipt["fixture_manifest_sha256"] = hashlib.sha256((fixture / "manifest.json").read_bytes()).hexdigest()
        # Bind tracked source bytes too: a matching HEAD with dirty/new source
        # is not an executable receipt for the named candidate.
        sources = dict(manifest["candidate_sources"])
        sources["crates/parking-lot-pg/tests/fixtures/oa14/manifest.json"] = self.receipt["fixture_manifest_sha256"]
        for path, digest in sources.items():
            if hashlib.sha256((self.root / path).read_bytes()).hexdigest() != digest:
                raise RuntimeError("OA-14:candidate-working-source changed: " + path)
            _, committed, _ = self.run("candidate-source", ["git", "show", self.args.candidate + ":" + path])
            if hashlib.sha256(committed.encode()).hexdigest() != digest:
                raise RuntimeError("OA-14:candidate-committed-source mismatch: " + path)
        code, _, _ = self.run("docker", ["docker", "info", "--format", '{{.ServerVersion}}'], allowed=(0, 1))
        if code:
            raise Blocked("Docker daemon unavailable")
        code, version, _ = self.run("nextest", ["cargo", "nextest", "--version"], allowed=(0, 101))
        if code or not version.startswith("cargo-nextest 0.9.149"):
            raise Blocked("OA-14 requires preinstalled verified cargo-nextest 0.9.149; no automatic install")
        self.run("python", [sys.executable, "--version"])
        self.run("rustc", ["rustc", "--version", "--verbose"])
        self.inspect_image("image17", self.args.pg17_image, "17")
        self.inspect_image("image18", self.args.pg18_image, "18")
        self.inspect_image("upgrade-image", self.args.upgrade_image)
        for major, expected in [("17", self.args.pg17_image), ("18", self.args.pg18_image)]:
            _, actual, _ = self.run("upgrade-source" + major, ["docker", "image", "inspect",
                self.args.upgrade_image, "--format",
                '{{index .Config.Labels "ai.prometheus.upgrade.pg' + major + '-source"}}'])
            if actual.strip() != expected:
                raise RuntimeError("OA-14:upgrade-image source mismatch")
        _, base, _ = self.run("base18-layers", ["docker", "image", "inspect", self.args.pg18_image,
            "--format", '{{json .RootFS.Layers}}'])
        _, upgraded, _ = self.run("upgrade-layers", ["docker", "image", "inspect", self.args.upgrade_image,
            "--format", '{{json .RootFS.Layers}}'])
        base_layers = json.loads(base)
        if not base_layers or json.loads(upgraded)[:len(base_layers)] != base_layers:
            raise RuntimeError("OA-14:upgrade-image does not derive from exact project PG18 image")

    def volume(self, name):
        code, _, _ = self.run("unused-volume", ["docker", "volume", "inspect", name], allowed=(0, 1))
        if code == 0:
            raise RuntimeError("OA-14:refuse-existing-volume " + name)
        self.run("create-volume", ["docker", "volume", "create", "--label", self.label, name])
        self.resources["volumes"].append(name)
        self.save()

    def owned_container(self, name):
        code, _, _ = self.run("unused-container", ["docker", "container", "inspect", name], allowed=(0, 1))
        if code == 0:
            raise RuntimeError("OA-14:refuse-existing-container " + name)
        self.resources["containers"].append(name)
        self.save()

    def wait(self, container):
        for _ in range(60):
            code, _, _ = self.run("ready", ["docker", "exec", container, "pg_isready", "-h", "127.0.0.1", "-U", "postgres"],
                                  timeout=10, allowed=(0, 1, 2))
            if code == 0:
                return
            time.sleep(1)
        raise RuntimeError("OA-14:database-readiness timeout")

    def psql(self, name, container, sql, extra=()):
        return self.run(name, ["docker", "exec", "-i", container, "psql", "-X", "-U", "postgres",
            "-d", "postgres", "-v", "ON_ERROR_STOP=1", *extra], data=sql.encode(), timeout=120)

    def populate_and_backup(self):
        self.volume(self.work)
        self.volume(self.backup)
        self.run("mark-work", ["docker", "run", "--rm", "--pull=never", "--label", self.label,
            "--mount", f"type=volume,src={self.work},dst=/upgrade", "-e", "OA14_RUN_ID=" + self.args.run_id,
            self.args.upgrade_image, "-c",
            'test ! -e /upgrade/.oa14-owned && printf "%s" "$OA14_RUN_ID" > /upgrade/.oa14-owned && chown postgres:postgres /upgrade'])
        self.owned_container(self.old)
        self.run("source-start", ["docker", "run", "--detach", "--pull=never", "--name", self.old,
            "--label", self.label, "--mount", f"type=volume,src={self.work},dst=/var/lib/postgresql/data",
            "-e", "PGDATA=/var/lib/postgresql/data/17", "-e", "POSTGRES_PASSWORD=oa14-local-test-only",
            "-e", "POSTGRES_INITDB_ARGS=--data-checksums --encoding=UTF8 --locale=C", self.args.pg17_image])
        self.wait(self.old)
        _, version, _ = self.psql("source-version", self.old,
            "SELECT current_setting('server_version');\n", ("-At",))
        if version.strip() != "17.11 (Debian 17.11-1.pgdg12+2)":
            raise Blocked("source image is not pinned PostgreSQL17.11 PGDG package")
        fixture = self.root / "crates/parking-lot-pg/tests/fixtures/oa14"
        files = ["004_lease_queue_0_3.sql", "005_mailbox_populated_0_3.sql",
                 "006_pgmq_lease_queue_0_3.sql", "populated_queues.sql"]
        seed = "BEGIN;\n" + "\n".join((fixture / name).read_text() for name in files) + "\nCOMMIT;\n"
        self.psql("populate-real-0.3", self.old, seed, ("-v", "run_id=" + self.args.run_id))
        self.psql("source-oracle", self.old, "SELECT oa14_assert_source_preserved();\n")
        _, snapshot, _ = self.psql("source-logical-backup", self.old,
            "SELECT snapshot FROM oa14_fixture_baseline WHERE id;\n", ("-At",))
        (self.out / "source-logical-backup.json").write_text(snapshot)
        self.run("source-log", ["docker", "logs", self.old])
        self.run("source-stop-clean", ["docker", "stop", "--time=60", self.old], timeout=90)
        _, code, _ = self.run("source-exit", ["docker", "inspect", self.old, "--format", '{{.State.ExitCode}}'])
        if code.strip() != "0":
            raise RuntimeError("OA-14:unclean-source-stop")
        _, digest, _ = self.run("offline-backup", ["docker", "run", "--rm", "--pull=never", "--label", self.label,
            "--mount", f"type=volume,src={self.work},dst=/source,readonly",
            "--mount", f"type=volume,src={self.backup},dst=/backup", self.args.upgrade_image,
            "-c", 'test ! -e /source/17/postmaster.pid && test ! -e /backup/pg17-source.tar && '
            'tar -C /source -cf /backup/pg17-source.tar . && '
            'sha256sum /backup/pg17-source.tar | tee /backup/pg17-source.sha256 && '
            'chmod 444 /backup/pg17-source.tar /backup/pg17-source.sha256'], timeout=300)
        match = re.search(r"^([0-9a-f]{64})  /backup/pg17-source.tar$", digest, re.MULTILINE)
        if not match:
            raise RuntimeError("OA-14:missing-physical-backup-digest")
        self.receipt["backup_archive_sha256"] = match[1]

    def upgrade(self):
        self.run("physical-cluster-upgrade", ["docker", "run", "--rm", "--pull=never", "--label", self.label,
            "--mount", f"type=volume,src={self.work},dst=/upgrade",
            "--mount", f"type=bind,src={self.root},dst=/workspace,readonly",
            "--mount", f"type=bind,src={self.out / 'cluster'},dst=/receipts",
            "-e", "OA14_RUN_ID=" + self.args.run_id, self.args.upgrade_image, "-c",
            "exec gosu postgres bash /workspace/docker/postgres18/upgrade/cluster-rehearsal.sh"], timeout=900)
        self.owned_container(self.new)
        self.run("target-start", ["docker", "run", "--detach", "--pull=never", "--name", self.new,
            "--label", self.label, "--mount", f"type=volume,src={self.work},dst=/var/lib/postgresql",
            "--publish", "127.0.0.1::5432", "-e", "PGDATA=/var/lib/postgresql/18", self.args.pg18_image])
        self.wait(self.new)
        self.psql("physical-preservation-oracle", self.new, "SELECT oa14_assert_source_preserved();\n")
        _, port, _ = self.run("target-port", ["docker", "port", self.new, "5432/tcp"])
        match = re.fullmatch(r"127\.0\.0\.1:(\d+)\s*", port)
        if not match:
            raise RuntimeError("OA-14:unsafe-published-port")
        return "postgres://postgres:oa14-local-test-only@127.0.0.1:" + match[1] + "/postgres"

    def restore_backup(self):
        restored = self.prefix + "-restore"
        container = self.prefix + "-restore17"
        self.volume(restored)
        self.run("restore-untouched-backup", ["docker", "run", "--rm", "--pull=never", "--label", self.label,
            "--mount", f"type=volume,src={self.backup},dst=/backup,readonly",
            "--mount", f"type=volume,src={restored},dst=/restore", self.args.upgrade_image,
            "-c", 'test ! -e /restore/.oa14-owned && sha256sum -c /backup/pg17-source.sha256 && '
            'tar -C /restore -xf /backup/pg17-source.tar'], timeout=300)
        self.owned_container(container)
        self.run("restored-source-start", ["docker", "run", "--detach", "--pull=never", "--name", container,
            "--label", self.label, "--mount", f"type=volume,src={restored},dst=/var/lib/postgresql/data",
            "-e", "PGDATA=/var/lib/postgresql/data/17", self.args.pg17_image])
        self.wait(container)
        self.psql("restored-source-oracle", container,
            "SELECT oa14_assert_source_preserved();\nSHOW server_version;\nSHOW data_checksums;\n")
        self.receipt["backup_restore"] = "PASS: independent PG17 copy passed native logical oracle; archive mounted read-only"

    def suites(self, url):
        env = dict(os.environ, PARKING_LOT_PG_UPGRADE_URL=url,
                   PARKING_LOT_PG_UPGRADE_RUN_ID=self.args.run_id,
                   PARKING_LOT_PG_MAJOR="18", CARGO_TERM_COLOR="never")
        expr = "package(prometheus-parking-lot-pg)"
        if self.args.control == "missing-result":
            expr = "binary(postgres_upgrade)"
            env["RUSTFLAGS"] = env.get("RUSTFLAGS", "") + ' --cfg parking_lot_negative_control="oa-14-missing-result"'
        code, out, err = self.run("schema3-and-real-upgraded-suites", ["cargo", "nextest", "run",
            "--workspace", "--all-features", "--locked", "--run-ignored", "all", "--no-fail-fast",
            "--no-tests=fail", "--test-threads=1", "--retries=0", "-E", expr],
            timeout=2700, env=env, allowed=(0, 100))
        combined = re.sub(r"\x1b\[[0-9;]*m", "", out + err)
        counts = re.findall(r"Starting (\d+) tests? across", combined)
        if not counts or int(counts[-1]) == 0:
            raise RuntimeError("OA-14:missing-test-discovery")
        if self.args.control == "missing-result":
            if code != 100 or "OA-14:seeded-result" not in combined or int(counts[-1]) != 1:
                raise RuntimeError("OA-14:wrong-negative-failure")
            self.receipt["status"] = "EXPECTED_FAILURE"
            self.receipt["oracle"] = "OA-14:seeded-result"
        else:
            if code != 0 or "postgres_upgrade_populated_0_3_to_schema3" not in combined:
                raise RuntimeError("OA-14:real-upgraded-suites-failed")
            self.receipt["status"] = "PASS"
        self.receipt["discovered_tests"] = int(counts[-1])

    def cleanup(self):
        for name in self.resources["containers"]:
            code, label, error = self.run("cleanup-identity", ["docker", "container", "inspect", name,
                "--format", '{{index .Config.Labels "ai.prometheus.oa14.run"}}'], allowed=(0, 1))
            if code and "no such" not in error.lower():
                raise RuntimeError("OA-14:cleanup-inspection-unavailable")
            if code == 0:
                if label.strip() != self.args.run_id:
                    raise RuntimeError("OA-14:cleanup-owner-mismatch")
                self.run("retained-container-log", ["docker", "logs", name])
                self.run("cleanup-owned-container", ["docker", "rm", "--force", "--volumes", name], timeout=90)
        self.resources["manual_cleanup_argv"] = ["docker", "volume", "rm", *self.resources["volumes"]]
        self.resources["backup_restriction"] = "Retain offline archive and evidence. Never downgrade converted data; restore backup to a new owned volume with every new writer stopped."


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["out", "run-id", "candidate", "contract-sha256", "pg17-image", "pg18-image", "upgrade-image"]:
        parser.add_argument("--" + name, required=True)
    parser.add_argument("--control", choices=["none", "missing-result"], default="none")
    args = parser.parse_args()
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]{5,39}", args.run_id):
        parser.error("unsafe run id")
    if not re.fullmatch(r"[0-9a-f]{40}", args.candidate) or not re.fullmatch(r"[0-9a-f]{64}", args.contract_sha256):
        parser.error("full candidate SHA and contract SHA256 required")
    for image in [args.pg17_image, args.pg18_image, args.upgrade_image]:
        if not re.fullmatch(r"sha256:[0-9a-f]{64}", image):
            parser.error("local immutable Docker image IDs required; mutable tags forbidden")
    rehearsal = Rehearsal(args)
    result = 1
    try:
        rehearsal.verify_inputs()
        rehearsal.populate_and_backup()
        url = rehearsal.upgrade()
        rehearsal.restore_backup()
        rehearsal.suites(url)
        result = 0
    except Blocked as error:
        rehearsal.receipt.update(status="BLOCKED", error=str(error))
        result = 2
    except (RuntimeError, OSError, ValueError) as error:
        rehearsal.receipt.update(status="FAIL", error=str(error))
    finally:
        try:
            rehearsal.cleanup()
        except (RuntimeError, OSError) as error:
            rehearsal.receipt.update(status="FAIL", cleanup_error=str(error))
            result = 1
        rehearsal.save()
    print(json.dumps({"status": rehearsal.receipt["status"], "receipt": str(rehearsal.out / "receipt.json")}))
    return result


if __name__ == "__main__":
    sys.exit(main())
