#!/usr/bin/env python3
"""Discriminating manual artifact controls; synthetic data never certifies a release."""
from __future__ import annotations

import copy
import datetime as dt
import importlib.util
import json
import os
from pathlib import Path
import stat
import struct
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import zipfile

sys.dont_write_bytecode = True
SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
spec = importlib.util.spec_from_file_location("boundary_manual", SCRIPTS / "operational-assurance.py")
boundary = importlib.util.module_from_spec(spec)
spec.loader.exec_module(boundary)


class ManualBundleControls(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="oa20-manual-fixture-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.candidate, self.binding = "1" * 40, "2" * 64
        self.model = {"profiles": {p: "Darwin" if p in {"macos", "ios", "macos-gui"}
                                  else "Windows" if p == "windows" else "native"
                                  for p in boundary.AUTOMATIC_PROFILES | set(boundary.MANUAL_PROFILES)},
                      "profile_sources": {**{p: "github-actions" for p in boundary.AUTOMATIC_PROFILES},
                                          **{p: "approved-manual-bundle" for p in boundary.MANUAL_PROFILES}},
                      "commands": [{"id": p + "-oracle", "profile": p, "argv": ["printf", p],
                                    "scenarios": ["OA-15" if p == "macos-gui" else "OA-17"],
                                    "contains": p, "tools": [], "timeout": 60}
                                   for p in boundary.MANUAL_PROFILES]}
        self.definition = self.root / "definition.json"
        boundary.write(self.definition, self.model)
        self.addCleanup(patch.stopall)
        patch.object(boundary, "DEFINITION", self.definition).start()
        self.source_hashes = boundary.sources()
        self.config_path = self.root / "authority.json"
        self.inputs = {"authority_config": str(self.config_path),
                       "quiet_attestation": str(self.root / "quiet-attestation.json")}
        boundary.write(Path(self.inputs["quiet_attestation"]), {"synthetic": True})
        self.config = {"candidate": self.candidate, "contract_digest": self.binding,
                       "max_age_seconds": 3600, "manual_profiles": {}, "subjects": {}}
        for profile, subject in boundary.MANUAL_PROFILES.items():
            self.bundle(profile)
            archive = self.root / (profile + ".zip")
            report_path = self.root / (profile + "-authority.json")
            report = {"candidate": self.candidate, "contract_digest": self.binding,
                      "observed_at": boundary.stamp(), "verdict": "PASS", "profile": profile,
                      "machine": "synthetic-machine", "host": "synthetic-host", "os": "synthetic-Darwin",
                      "archive_sha256": boundary.digest(archive),
                      "positive_observation": "synthetic positive", "negative_observation": "synthetic negative"}
            boundary.write(report_path, report)
            evidence = [{"path": archive.name, "sha256": boundary.digest(archive)}]
            if profile == "quiet":
                evidence.append({"path": "quiet-attestation.json",
                                 "sha256": boundary.digest(Path(self.inputs["quiet_attestation"]))})
            self.config["subjects"][subject] = {"report": report_path.name,
                "sha256": boundary.digest(report_path), "source": "fixture-only", "evidence": evidence}
            self.config["manual_profiles"][profile] = {"path": archive.name,
                "sha256": boundary.digest(archive), "subject": subject}
        self.pin()

    def pin(self):
        boundary.write(self.config_path, self.config)
        os.environ["OA18_AUTHORITY_SHA256"] = boundary.digest(self.config_path)
        self.addCleanup(os.environ.pop, "OA18_AUTHORITY_SHA256", None)

    def bundle(self, profile, mutate=None):
        folder = self.root / (profile + "-source")
        folder.mkdir()
        row = copy.deepcopy(next(r for r in self.model["commands"] if r["profile"] == profile))
        row.update(cwd="/fixture/workspace", env={}, paths=[], images=[])
        frozen = {"candidate": self.candidate, "contract_digest": self.binding,
                  "profile": profile, "system": "Darwin", "os": "synthetic-Darwin",
                  "machine": "synthetic-machine", "host": "synthetic-host", "source_sha256": self.source_hashes,
                  "frozen_at": boundary.stamp(), "commands": [row],
                  "inputs": {"workspace": "/fixture/workspace", "candidate": self.candidate,
                             "contract": "/fixture/contract.json", "out": "/fixture/output"}}
        (folder / "stdout.txt").write_text(profile)
        (folder / "stderr.txt").write_text("")
        command = {"argv": row["argv"], "cwd": row["cwd"], "exit": 0,
                   "stdout": "stdout.txt", "stderr": "stderr.txt",
                   "stdout_sha256": boundary.digest(folder / "stdout.txt"),
                   "stderr_sha256": boundary.digest(folder / "stderr.txt")}
        receipt = {"candidate": self.candidate, "contract_digest": self.binding,
                   "definition_sha256": boundary.digest(self.definition), "profile": profile,
                   "status": "PASS", "finished": boundary.stamp(),
                   "records": [{"id": row["id"], "scenarios": row["scenarios"],
                                "status": "PASS", "command": command}]}
        if mutate:
            mutate(frozen, receipt)
        boundary.write(folder / "checked-manifest.json", frozen)
        receipt["manifest_sha256"] = boundary.digest(folder / "checked-manifest.json")
        boundary.write(folder / "job-receipt.json", receipt)
        boundary.seal(folder)
        with zipfile.ZipFile(self.root / (profile + ".zip"), "w") as archive:
            for path in folder.iterdir():
                archive.write(path, path.name)

    def repack(self, profile, mutate):
        import shutil
        shutil.rmtree(self.root / (profile + "-source"))
        self.bundle(profile, mutate)
        entry = self.config["manual_profiles"][profile]
        entry["sha256"] = boundary.digest(self.root / entry["path"])
        subject = self.config["subjects"][entry["subject"]]
        subject["evidence"][0]["sha256"] = entry["sha256"]
        report = self.root / subject["report"]
        value = json.loads(report.read_bytes())
        value["archive_sha256"] = entry["sha256"]
        value["observed_at"] = boundary.stamp()
        boundary.write(report, value)
        subject["sha256"] = boundary.digest(report)
        self.pin()

    def collect(self):
        output = self.root / "output"
        output.mkdir()
        return boundary.import_manual_jobs(output, self.model, self.inputs, self.candidate, self.binding)

    def test_windows_native_commands_and_portable_nested_packet_survive_export(self):
        # A Windows producer's native execution identities coexist with POSIX
        # archive identities consumed by a Linux collector; execute no oracle.
        checked = {"id": "windows-oracle", "profile": "windows", "argv": ["python3", "oracle.py"],
                   "scenarios": ["OA-15"], "tools": [], "timeout": 60,
                   "receipt": "{out}/nested/child.json",
                   "receipt_identity": {"os": "Windows", "case_id": "OA15-BUILD-WINDOWS"}}
        self.model["commands"].append(checked)
        boundary.write(self.definition, self.model)
        folder = self.root / "windows-source"
        (folder / "nested").mkdir(parents=True)
        (folder / "nested/stdout.txt").write_text("synthetic Windows oracle")
        (folder / "nested/stderr.txt").write_text("")
        boundary.write(folder / "nested/child.json", {"status": "PASS", "candidate": self.candidate,
                       "contract_digest": self.binding, **checked["receipt_identity"]})
        row = {**checked, "cwd": r"C:\work\repo", "env": {}, "paths": [], "images": [],
               "receipt": r"C:\artifacts\out/nested/child.json"}
        frozen = {"candidate": self.candidate, "contract_digest": self.binding,
                  "profile": "windows", "system": "Windows", "commands": [row],
                  "source_sha256": self.source_hashes,
                  "inputs": {"workspace": r"C:\work\repo", "candidate": self.candidate,
                             "out": r"C:\artifacts\out", "contract": r"C:\work\repo\contract.json"}}
        boundary.write(folder / "checked-manifest.json", frozen)
        command = {"argv": checked["argv"], "cwd": r"C:\work\repo", "exit": 0,
                   "stdout": "nested/stdout.txt", "stderr": "nested/stderr.txt",
                   "stdout_sha256": boundary.digest(folder / "nested/stdout.txt"),
                   "stderr_sha256": boundary.digest(folder / "nested/stderr.txt")}
        receipt = {"candidate": self.candidate, "contract_digest": self.binding, "profile": "windows",
                   "status": "PASS", "definition_sha256": boundary.digest(self.definition),
                   "manifest_sha256": boundary.digest(folder / "checked-manifest.json"),
                   "records": [{"id": checked["id"], "scenarios": checked["scenarios"], "status": "PASS",
                                "command": command, "child_receipt": "nested/child.json",
                                "child_sha256": boundary.digest(folder / "nested/child.json")}]}
        boundary.write(folder / "job-receipt.json", receipt)
        boundary.seal(folder)
        manifest = json.loads((folder / "files-sha256.json").read_bytes())
        self.assertIn("nested/child.json", manifest)
        self.assertTrue(all("\\" not in name for name in manifest))
        exported = self.root / "windows-export"
        self.assertEqual(boundary.export(SimpleNamespace(receipts=folder, out=exported)), 0)
        self.assertEqual(boundary.verify_job(exported, self.model, self.candidate, self.binding)["status"], "PASS")
        manifest = {name.replace("/", "\\"): value for name, value in manifest.items()}
        boundary.write(exported / "files-sha256.json", manifest)
        with self.assertRaisesRegex(boundary.ReleaseError, "omitted-raw-evidence"):
            boundary.verify_job(exported, self.model, self.candidate, self.binding)

    def test_complete_synthetic_manual_archives_accepted(self):
        self.assertTrue(all(r["status"] == "PASS" for r in self.collect().values()))

    def test_unapproved_archive_bytes_rejected(self):
        self.config["subjects"]["OA15-GUI"]["evidence"] = [
            {"path": "quiet-attestation.json", "sha256": boundary.digest(Path(self.inputs["quiet_attestation"]))}]
        self.pin()
        self.assertEqual(self.collect()["macos-gui"]["status"], "BLOCKED")

    def test_archive_hash_change_rejected(self):
        with (self.root / "macos-gui.zip").open("ab") as stream:
            stream.write(b"changed")
        self.assertEqual(self.collect()["macos-gui"]["status"], "BLOCKED")

    def test_stale_candidate_contract_definition_os_commands_and_scenario_rejected(self):
        mutations = [lambda f, r: r.update(candidate="3" * 40),
                     lambda f, r: r.update(contract_digest="3" * 64),
                     lambda f, r: r.update(definition_sha256="3" * 64),
                     lambda f, r: f.update(system="Linux"),
                     lambda f, r: f["commands"][0].update(contains="fabricated"),
                     lambda f, r: r["records"][0].update(scenarios=["OA-17"]),
                     lambda f, r: f.update(frozen_at=(dt.datetime.now(dt.timezone.utc) - dt.timedelta(days=2)).isoformat())]
        for number, mutation in enumerate(mutations):
            with self.subTest(mutation=number):
                self.repack("macos-gui", mutation)
                result = self.collect()["macos-gui"]
                self.assertEqual(result["status"], "BLOCKED", result)
                import shutil
                shutil.rmtree(self.root / "output")

    def test_manual_cannot_override_automatic_or_omit_required_profile(self):
        self.config["manual_profiles"]["linux"] = self.config["manual_profiles"].pop("quiet")
        self.pin()
        self.assertTrue(all(r["status"] == "BLOCKED" for r in self.collect().values()))

    def test_changed_authority_host_binding_rejected(self):
        subject = self.config["subjects"]["OA15-GUI"]
        path = self.root / subject["report"]
        report = json.loads(path.read_bytes())
        report["machine"] = "another-machine"
        boundary.write(path, report)
        subject["sha256"] = boundary.digest(path)
        self.pin()
        self.assertEqual(self.collect()["macos-gui"]["status"], "BLOCKED")

    def test_material_traversal_and_symlink_rejected(self):
        for name in ("../authority.json", "sub/file", "C:\\file", ".", ".."):
            with self.assertRaises(boundary.ReleaseError):
                boundary.authority_path(self.config_path, name)
        (self.root / "linked.json").symlink_to(self.config_path)
        with self.assertRaises(boundary.ReleaseError):
            boundary.authority_path(self.config_path, "linked.json")

    def test_archive_traversal_symlink_fifo_duplicate_and_excess_rejected(self):
        for number, name in enumerate(("../escape", "/absolute", "a/../b", "a\\b", "a//b", "C:/file")):
            archive = self.root / f"bad-{number}.zip"
            with zipfile.ZipFile(archive, "w") as bundle:
                bundle.writestr(name, "bad")
            with self.assertRaises(boundary.ReleaseError):
                boundary.extract_archive(archive, self.root / f"bad-{number}")
        for number, kind in enumerate((stat.S_IFLNK, stat.S_IFIFO)):
            archive = self.root / f"type-{number}.zip"
            info = zipfile.ZipInfo("link")
            info.external_attr = (kind | 0o600) << 16
            with zipfile.ZipFile(archive, "w") as bundle:
                bundle.writestr(info, "bad")
            with self.assertRaises(boundary.ReleaseError):
                boundary.extract_archive(archive, self.root / f"type-{number}")
        archive = self.root / "duplicate.zip"
        with zipfile.ZipFile(archive, "w") as bundle:
            bundle.writestr("same", "one")
            import warnings
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", UserWarning)
                bundle.writestr("same", "two")
        with self.assertRaises(boundary.ReleaseError):
            boundary.extract_archive(archive, self.root / "duplicate")
        archive = self.root / "large.zip"
        with zipfile.ZipFile(archive, "w") as bundle:
            bundle.writestr("one", "one")
            bundle.writestr("two", "two")
        data = bytearray(archive.read_bytes())
        offset = 0
        while (offset := data.find(b"PK\x01\x02", offset)) >= 0:
            struct.pack_into("<I", data, offset + 24, 3 * 1024 ** 3)
            offset += 4
        archive.write_bytes(data)
        with self.assertRaises(boundary.ReleaseError):
            boundary.extract_archive(archive, self.root / "large")



if __name__ == "__main__":
    unittest.main(verbosity=2)
