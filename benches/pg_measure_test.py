"""Report-only classifier fixtures; synthetic data never claims PG measurement.

The real workload/control entry is pg_measure.py measure [--slow-control].
These fixtures check validation/math independently at the final boundary.
"""

import importlib.util
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("pg_measure", Path(__file__).with_name("pg_measure.py"))
MEASURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MEASURE)


def corpus(directory, slow=False):
    """An explicitly synthetic, internally consistent report-validation corpus."""
    identity = {"os": "synthetic Linux fixture", "uname": ["Linux", "fixture", "fixture", "fixture", "x86_64", "fixture"],
                "cpu": {"memory_bytes": 1024, "cpu_fields": {"model name": ["fixture CPU"]}}, "logical_cpus": 8,
                "python": "synthetic fixture", "docker_version": "synthetic fixture", "docker_vm": {"NCPU": 8, "MemTotal": 1024},
                "image": {"id": "sha256:" + "a" * 64, "os": "linux", "architecture": "amd64", "repo_digests": []}}
    attestation = {key: True for key in ("no_active_agents", "no_concurrent_builds", "no_other_benchmarks", "no_host_tuning")}
    attestation.update(operator="synthetic fixture", machine_id="synthetic fixture")
    receipt = {"harness_sha256": "b" * 64, "features": list(MEASURE.FEATURES), "profile": {"opt_level": "3"},
               "rustc": "rustc 1.99.1 (synthetic fixture)", "cargo": "synthetic fixture", "tool_sha256": MEASURE.digest(MEASURE.__file__)}
    manifest = {"schema": 1, "mode": "slow-control" if slow else "normal", "samples": 5,
                "cases": ["mailbox_duplicate"] if slow else list(MEASURE.CASES), "clients": [8] if slow else list(MEASURE.CLIENTS),
                "machine": identity, "attestation": attestation, "records": [], "build_logs": {}}
    for arm in ("baseline", "candidate"):
        manifest[arm] = dict(receipt, revision=MEASURE.BASELINE if arm == "baseline" else "c" * 40,
                             executable_sha256=("d" if arm == "baseline" else "e") * 64)
        manifest["build_logs"][arm] = {}
        for field in ("build_log", "build_stderr"):
            file = directory / f"{arm}-{field}.log"
            file.write_text("synthetic fixture\n")
            manifest["build_logs"][arm][field] = {"path": file.name, "sha256": MEASURE.digest(file)}
            manifest[arm][field + "_sha256"] = MEASURE.digest(file)
    for case in manifest["cases"]:
        for clients in manifest["clients"]:
            for pair in range(5):
                for arm in ("baseline", "candidate"):
                    stem = f"{case}-{clients}-{pair}-{arm}"
                    latency = 101_000_000 if slow and arm == "candidate" else 1_000_000
                    raw = {"workload": MEASURE.expected_workload(case, clients), "verified_operations": MEASURE.OPERATIONS,
                           "latencies_ns": [latency] * MEASURE.OPERATIONS, "elapsed_ns": latency * MEASURE.OPERATIONS,
                           "control_delay_us": MEASURE.SLOW_US if slow and arm == "candidate" else 0}
                    stdout, stderr = directory / f"{stem}.stdout", directory / f"{stem}.stderr"
                    MEASURE.save(stdout, raw)
                    stderr.write_text("")
                    row = {"arm": arm, "case": case, "clients": clients, "pair": pair, "machine": identity,
                           "executable_sha256": manifest[arm]["executable_sha256"], "exit": 0, "timed_out": False,
                           "peak_rss_bytes": 1024 * 1024, "rss_scope": MEASURE.RSS_SCOPE,
                           "rss_raw": {"ru_maxrss": 1024, "native_unit": "KiB", "method": "wait4", "pid": 99},
                           "raw": raw, "postgres": "PostgreSQL synthetic fixture", "container_resources": {},
                           "stdout": stdout.name, "stdout_sha256": MEASURE.digest(stdout),
                           "stderr": stderr.name, "stderr_sha256": MEASURE.digest(stderr)}
                    for field in ("before", "after"):
                        file = directory / f"{stem}.{field}.ps"
                        file.write_text("1 0 /usr/bin/python3\n")
                        row[field] = {"path": file.name, "sha256": MEASURE.digest(file)}
                    file = directory / f"{stem}.json"
                    MEASURE.save(file, row)
                    manifest["records"].append({"path": file.name, "sha256": MEASURE.digest(file)})
    MEASURE.save(directory / "manifest.json", manifest)
    return manifest


class ReportControls(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)

    def mutate_row(self, manifest, mutate):
        record = manifest["records"][0]
        file = self.directory / record["path"]
        row = MEASURE.load(file)
        mutate(row)
        MEASURE.save(file, row)
        record["sha256"] = MEASURE.digest(file)
        MEASURE.save(self.directory / "manifest.json", manifest)

    def test_complete_equal_samples_validate_without_claiming_gain(self):
        corpus(self.directory)
        manifest, rows = MEASURE.validate(self.directory)
        summary = MEASURE.summarize(manifest, rows)
        self.assertTrue(all(metric["classification"] == "inconclusive-noise-overlap"
                            for item in summary for metric in item["metrics"].values()))

    def test_dropped_raw_sample_record_is_rejected(self):
        manifest = corpus(self.directory)
        manifest["records"].pop()
        MEASURE.save(self.directory / "manifest.json", manifest)
        with self.assertRaisesRegex(ValueError, "OA-17:sample-count"):
            MEASURE.validate(self.directory)

    def test_missing_raw_sample_file_is_rejected(self):
        manifest = corpus(self.directory)
        (self.directory / manifest["records"][0]["path"]).unlink()
        with self.assertRaisesRegex(ValueError, "OA-17:missing-raw"):
            MEASURE.validate(self.directory)

    def test_changed_machine_identity_is_rejected_even_with_rehashed_record(self):
        manifest = corpus(self.directory)
        self.mutate_row(manifest, lambda row: row["machine"].update(logical_cpus=16))
        with self.assertRaisesRegex(ValueError, "OA-17:machine-identity"):
            MEASURE.validate(self.directory)

    def test_changed_workload_is_rejected_even_with_rehashed_record(self):
        manifest = corpus(self.directory)
        self.mutate_row(manifest, lambda row: row["raw"]["workload"].update(operations=999))
        with self.assertRaisesRegex(ValueError, "OA-17:workload-identity"):
            MEASURE.validate(self.directory)

    def test_missing_raw_latency_is_rejected(self):
        manifest = corpus(self.directory)
        self.mutate_row(manifest, lambda row: row["raw"]["latencies_ns"].pop())
        with self.assertRaisesRegex(ValueError, "OA-17:raw-latencies"):
            MEASURE.validate(self.directory)

    def test_slow_control_cannot_enter_normal_report(self):
        corpus(self.directory, slow=True)
        with self.assertRaisesRegex(ValueError, "OA-17:control"):
            MEASURE.validate(self.directory)

    def test_declared_slow_control_is_classified_as_regression(self):
        corpus(self.directory, slow=True)
        manifest, rows = MEASURE.validate(self.directory, allow_slow=True)
        summary = MEASURE.summarize(manifest, rows)
        for metric in ("throughput_ops_s", "p50_ns", "p95_ns", "p99_ns"):
            self.assertEqual(summary[0]["metrics"][metric]["classification"], "regression-observed")


if __name__ == "__main__":
    unittest.main()
