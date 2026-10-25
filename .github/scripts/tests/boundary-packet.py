#!/usr/bin/env python3
"""OA-18 discriminating packet predicates. No mock can certify real scenarios."""
from __future__ import annotations

import copy
import importlib.util
import json
import os
import subprocess
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
from unittest.mock import patch
import unittest

sys.dont_write_bytecode = True
SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
spec = importlib.util.spec_from_file_location("boundary", SCRIPTS / "operational-assurance.py")
boundary = importlib.util.module_from_spec(spec)
spec.loader.exec_module(boundary)


class AndroidToolControls(unittest.TestCase):
    @unittest.skipUnless(os.name == "posix", "Android hosted profile uses POSIX executables")
    def test_cargo_plugin_health_preserves_plugin_identity(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            launcher = root / "cargo"
            plugin = root / "cargo-ndk"
            launcher.write_text('#!/bin/sh\n[ "$1" = ndk ] && [ "$2" = --version ] && echo "cargo-ndk 4.1.2"\n')
            launcher.chmod(0o755)
            plugin.write_bytes(b"distinct plugin identity")
            plugin.chmod(0o755)
            output = root / "out"
            output.mkdir()
            args = SimpleNamespace(candidate="1" * 40, contract=root / "contract", inputs=None, profile="android")
            model = {"contract_digest": "2" * 64, "profiles": {"android": "native"},
                     "commands": [{"profile": "android", "argv": [], "tools": ["ndk"]}],
                     "required_authority": [], "budgets": {}}
            with patch.dict(os.environ, {"PATH": str(root)}), \
                 patch.object(boundary, "clean"), \
                 patch.object(boundary, "contract", return_value="2" * 64), \
                 patch.object(boundary, "sources", return_value={}):
                result = boundary.make_freeze(args, model, output)["tools"]["ndk"]
            self.assertEqual(result["status"], "PASS")
            self.assertEqual(result["version_argv"], ["cargo", "ndk", "--version"])
            self.assertEqual(result["path"], str(plugin.resolve()))
            self.assertEqual(result["sha256"], boundary.digest(plugin))
            self.assertNotEqual(result["sha256"], boundary.digest(launcher))


class PacketControls(unittest.TestCase):
    def setUp(self):
        self.candidate = "1" * 40
        self.binding = "2" * 64
        self.model = {"profiles": {"linux": "Linux", "macos": "Darwin", "windows": "Windows"},
                      "required_scenarios": [f"OA-{n:02}" for n in range(1, 18)]}
        self.packet = {"candidate": self.candidate, "contract_digest": self.binding,
                       "profiles": {p: {"status": "PASS"} for p in self.model["profiles"]},
                       "scenarios": {s: {"status": "PASS"} for s in self.model["required_scenarios"]},
                       "baseline": {"status": "PASS"}, "review": {"status": "PASS"},
                       "provenance": {"status": "PASS"},
                       "operator_gates": {s: {"status": "PASS"} for s in ("G-02", "G-03", "G-04")},
                       "status": "PASS"}

    def accept(self, packet):
        boundary.validate_packet(packet, self.model, self.candidate, self.binding)

    def test_control_reference_predicate_accepts_complete_fixture(self):
        # Synthetic predicate input only, never uploaded/used as an observed receipt.
        self.accept(self.packet)

    def test_stale_candidate_is_rejected(self):
        self.packet["candidate"] = "3" * 40
        with self.assertRaisesRegex(boundary.ReleaseError, "OA-18:stale-packet"):
            self.accept(self.packet)

    def test_stale_contract_is_rejected(self):
        self.packet["contract_digest"] = "4" * 64
        with self.assertRaisesRegex(boundary.ReleaseError, "OA-18:stale-packet"):
            self.accept(self.packet)

    def test_omitted_os_receipt_is_rejected(self):
        del self.packet["profiles"]["windows"]
        with self.assertRaisesRegex(boundary.ReleaseError, "OA-18:omitted-os-receipt"):
            self.accept(self.packet)

    def test_omitted_scenario_is_rejected(self):
        del self.packet["scenarios"]["OA-07"]
        with self.assertRaisesRegex(boundary.ReleaseError, "OA-18:omitted-scenario"):
            self.accept(self.packet)

    def test_skip_cannot_replace_required_scenario(self):
        self.packet["scenarios"]["OA-15"]["status"] = "SKIPPED"
        with self.assertRaisesRegex(boundary.ReleaseError, "OA-18:skipped-packet"):
            self.accept(self.packet)

    def test_missing_authority_cannot_be_pass(self):
        self.packet["operator_gates"]["G-03"]["status"] = "BLOCKED"
        with self.assertRaisesRegex(boundary.ReleaseError, "OA-18:false-packet-pass"):
            self.accept(self.packet)

    def test_not_exercised_cannot_be_pass(self):
        self.packet["scenarios"]["OA-14"]["status"] = "NOT_EXERCISED"
        with self.assertRaisesRegex(boundary.ReleaseError, "OA-18:false-packet-pass"):
            self.accept(self.packet)

    def test_all_four_dispositions_remain_distinct(self):
        for status in ("FAIL", "BLOCKED", "NOT_EXERCISED"):
            packet = copy.deepcopy(self.packet)
            packet["scenarios"]["OA-14"]["status"] = status
            packet["status"] = status
            self.accept(packet)
            self.assertNotEqual(packet["status"], "PASS")

    def test_no_test_execution_cannot_be_pass(self):
        self.assertEqual(boundary.discovered("test result: ok. 0 passed; 12 ignored", "libtest"), 0)
        self.assertEqual(boundary.discovered("Summary 0 tests run", "nextest"), 0)
        self.assertEqual(boundary.discovered("test result: ok. 12 passed; 0 failed", "libtest"), 12)

    def test_desktop_receipts_require_each_exact_os_and_build_case(self):
        cases = {"Linux": "OA15-BUILD-LINUX", "Darwin": "OA15-BUILD-MACOS",
                 "Windows": "OA15-BUILD-WINDOWS"}
        for system, case in cases.items():
            row = {"receipt_identity": {"os": system, "case_id": case}}
            receipt = {"status": "PASS", "candidate": self.candidate,
                       "contract_digest": self.binding, "os": system, "case_id": case}
            boundary.validate_child_receipt(receipt, row, self.candidate, self.binding)
            for wrong_system, wrong_case in cases.items():
                if wrong_system == system:
                    continue
                for replacement in ({"os": wrong_system}, {"case_id": wrong_case},
                                    {"os": wrong_system, "case_id": wrong_case}):
                    with self.subTest(required=system, replacement=replacement):
                        with self.assertRaisesRegex(boundary.ReleaseError, "OA-18:child-receipt-identity"):
                            boundary.validate_child_receipt({**receipt, **replacement}, row,
                                                            self.candidate, self.binding)

    def test_desktop_definition_keeps_three_distinct_native_profiles(self):
        model = json.loads(boundary.DEFINITION.read_bytes())
        expected = {"desktop-linux": ("linux", "Linux", "OA15-BUILD-LINUX"),
                    "desktop-macos": ("macos", "Darwin", "OA15-BUILD-MACOS"),
                    "desktop-windows": ("windows", "Windows", "OA15-BUILD-WINDOWS")}
        for name, (profile, system, case) in expected.items():
            row = next(row for row in model["commands"] if row["id"] == name)
            self.assertEqual(row["profile"], profile)
            self.assertEqual(model["profiles"][profile], system)
            self.assertEqual(row["receipt_identity"], {"os": system, "case_id": case})

    def test_caller_flag_has_no_external_authority(self):
        result = boundary.authority("review", {"approved": "true"}, self.candidate, self.binding)
        self.assertEqual(result["status"], "BLOCKED")


class SourcePortabilityControls(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="oa20-link-identity-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.env = {**os.environ, "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull}
        for key in ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY",
                    "GIT_ALTERNATE_OBJECT_DIRECTORIES", "GIT_NO_REPLACE_OBJECTS"):
            self.env.pop(key, None)
        self.git("init", "-q")
        self.git("config", "core.symlinks", "false")
        self.target = b"../../LICENSE-MIT"
        self.blob = self.git("hash-object", "-w", "--stdin", data=self.target).decode()
        self.git("update-index", "--add", "--cacheinfo", "120000," + self.blob + ",link")
        self.path = self.root / "link"
        self.path.write_bytes(self.target)
        self.root_patch = patch.object(boundary, "ROOT", self.root)
        self.root_patch.start()
        self.addCleanup(self.root_patch.stop)
        self.env_patch = patch.dict(os.environ, self.env, clear=True)
        self.env_patch.start()
        self.addCleanup(self.env_patch.stop)

    def git(self, *argv, data=None):
        return subprocess.run(["git", *argv], cwd=self.root, env=self.env, input=data,
                              capture_output=True, check=True).stdout.strip()

    def native_link(self, target):
        self.path.unlink()
        try:
            self.path.symlink_to(os.fsdecode(target))
        except OSError as error:
            self.skipTest("native link privileges unavailable: " + str(error))

    def test_exact_regular_materialization_uses_canonical_git_blob(self):
        self.assertEqual(boundary.sources(), {"link": boundary.sha256(self.target)})

    @unittest.skipUnless(os.name == "posix", "POSIX native link byte semantics")
    def test_exact_posix_link_uses_canonical_git_blob(self):
        self.native_link(self.target)
        with patch.object(boundary.platform, "system", return_value="Linux"):
            self.assertEqual(boundary.sources(), {"link": boundary.sha256(self.target)})

    def test_windows_native_separator_representation_uses_canonical_git_blob(self):
        self.native_link(self.target)
        represented = self.target.replace(b"/", b"\\")
        with patch.object(boundary.platform, "system", return_value="Windows"), \
                patch.object(boundary.os, "readlink", return_value=os.fsdecode(represented)):
            self.assertEqual(boundary.sources(), {"link": boundary.sha256(self.target)})

    @unittest.skipUnless(os.name == "posix", "POSIX literal backslash link semantics")
    def test_posix_does_not_normalize_literal_backslashes(self):
        self.native_link(self.target.replace(b"/", b"\\"))
        with patch.object(boundary.platform, "system", return_value="Linux"):
            with self.assertRaisesRegex(boundary.ReleaseError, "changed-symlink-source"):
                boundary.sources()

    @unittest.skipUnless(os.name == "posix", "POSIX literal backslash link semantics")
    def test_posix_backslash_target_remains_byte_exact(self):
        target = b"literal\\name"
        blob = self.git("hash-object", "-w", "--stdin", data=target).decode()
        self.git("update-index", "--cacheinfo", "120000," + blob + ",link")
        self.native_link(target)
        with patch.object(boundary.platform, "system", return_value="Linux"):
            self.assertEqual(boundary.sources(), {"link": boundary.sha256(target)})

    def test_windows_wrong_native_target_is_rejected(self):
        self.native_link(self.target)
        with patch.object(boundary.platform, "system", return_value="Windows"), \
                patch.object(boundary.os, "readlink", return_value="..\\..\\wrong-license"):
            with self.assertRaisesRegex(boundary.ReleaseError, "changed-symlink-source"):
                boundary.sources()

    def test_materialized_contents_are_not_separator_normalized(self):
        self.path.write_bytes(self.target.replace(b"/", b"\\"))
        with patch.object(boundary.platform, "system", return_value="Windows"):
            with self.assertRaisesRegex(boundary.ReleaseError, "changed-symlink-source"):
                boundary.sources()

    def test_materialized_referent_contents_are_rejected(self):
        self.path.write_bytes(b"MIT license referent contents\n")
        with self.assertRaisesRegex(boundary.ReleaseError, "changed-symlink-source"):
            boundary.sources()

    def test_missing_link_is_rejected(self):
        self.path.unlink()
        with self.assertRaisesRegex(boundary.ReleaseError, "missing-symlink-source"):
            boundary.sources()

    def test_git_replacement_cannot_mask_original_link_identity(self):
        changed = b"../../OTHER-LICENSE"
        replacement = self.git("hash-object", "-w", "--stdin", data=changed).decode()
        self.git("replace", self.blob, replacement)
        # Verify this fixture really has an active replacement outside sources().
        self.assertEqual(self.git("cat-file", "blob", self.blob), changed)
        self.assertEqual(boundary.sources(), {"link": boundary.sha256(self.target)})
        self.path.write_bytes(changed)
        with self.assertRaisesRegex(boundary.ReleaseError, "changed-symlink-source"):
            boundary.sources()

    def test_altered_index_link_blob_rejects_unchanged_worktree_target(self):
        changed = self.git("hash-object", "-w", "--stdin", data=b"../../OTHER-LICENSE").decode()
        self.git("update-index", "--cacheinfo", "120000," + changed + ",link")
        with self.assertRaisesRegex(boundary.ReleaseError, "changed-symlink-source"):
            boundary.sources()

    def test_unknown_index_mode_fails_closed(self):
        self.git("update-index", "--cacheinfo", "160000," + self.blob + ",link")
        with self.assertRaisesRegex(boundary.ReleaseError, "unsupported-source-mode"):
            boundary.sources()

    def test_git_symlink_and_windows_materialization_bind_identical_source_bytes(self):
        with tempfile.TemporaryDirectory(prefix="oa20-source-portability-") as temporary:
            root = Path(temporary)
            env = {**os.environ, "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull}
            for key in ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY",
                        "GIT_ALTERNATE_OBJECT_DIRECTORIES"):
                env.pop(key, None)
            def git(*argv, data=None):
                return subprocess.run(["git", *argv], cwd=root, env=env, input=data,
                                      capture_output=True, check=True).stdout.strip()
            git("init", "-q")
            git("config", "core.symlinks", "false")
            (root / "nested").mkdir()
            (root / "nested/source.py").write_bytes(b"source bytes\n")
            git("add", "nested/source.py")
            target = b"nested/source.py"
            blob = git("hash-object", "-w", "--stdin", data=target).decode()
            git("update-index", "--add", "--cacheinfo", "120000," + blob + ",CLAUDE.md")
            # This is what core.symlinks=false materializes on Windows: the
            # tracked target payload, not a copy of nested/source.py's content.
            (root / "CLAUDE.md").write_bytes(target)
            with patch.object(boundary, "ROOT", root):
                materialized = boundary.sources()
                self.assertEqual(materialized, {"nested/source.py": boundary.sha256(b"source bytes\n"),
                                                "CLAUDE.md": boundary.sha256(target)})
                self.assertNotEqual(materialized["CLAUDE.md"], materialized["nested/source.py"])
                (root / "CLAUDE.md").unlink()
                try:
                    (root / "CLAUDE.md").symlink_to("nested/source.py")
                except OSError:
                    # Windows without link privileges still exercised the real
                    # index mode and materialized working-tree representation.
                    return
                self.assertEqual(boundary.sources(), materialized)
                (root / "nested/source.py").write_bytes(b"changed referent\n")
                changed = boundary.sources()
                self.assertEqual(changed["CLAUDE.md"], materialized["CLAUDE.md"])
                self.assertNotEqual(changed["nested/source.py"], materialized["nested/source.py"])



class ScopedContainmentControls(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="oa20-scope-fixture-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.binding = "2" * 64
        amendment = self.root / ".kbd-orchestrator/phases/phase-0-4-operational-assurance/evidence/oa20/publication-preparation/scope-amendment.json"
        amendment.parent.mkdir(parents=True)
        boundary.write(amendment, {"newContractDigest": self.binding, "action": "amend-contract",
            "scenarioChanges": {"added": [], "modified": ["OA-01"], "removed": []}})
        self.pin = boundary.digest(amendment)
        self.model = {"containment_scope": "current-source-and-fixtures",
                      "containment_scope_amendment_sha256": self.pin}
        self.report = {"scope": "current-source-and-fixtures", "history_cleanup": "DEFERRED",
                       "credential_rotation": "DEFERRED", "scope_amendment_sha256": self.pin,
                       "current_source_evidence_sha256": "3" * 64}
        self.proofs = [{"sha256": self.pin}, {"sha256": "3" * 64}]
        patch.object(boundary, "ROOT", self.root).start()
        patch.object(boundary, "definition", lambda: self.model).start()
        self.addCleanup(patch.stopall)

    def test_exact_amendment_deferral_and_evidence_accepted(self):
        boundary.scoped_containment(self.report, self.proofs, self.binding)

    def test_missing_deferral_false_cleanup_wrong_amendment_and_missing_evidence_rejected(self):
        for key, value in (("history_cleanup", "PASS"), ("credential_rotation", "PASS"),
                           ("scope", "clean-history"), ("scope_amendment_sha256", "4" * 64),
                           ("current_source_evidence_sha256", "5" * 64)):
            with self.subTest(field=key):
                with self.assertRaises(boundary.ReleaseError):
                    boundary.scoped_containment({**self.report, key: value}, self.proofs, self.binding)
        with self.assertRaises(boundary.ReleaseError):
            boundary.scoped_containment(self.report, self.proofs[1:], self.binding)
        with self.assertRaises(boundary.ReleaseError):
            boundary.scoped_containment(self.report, self.proofs, "6" * 64)

    def test_old_scope_still_requires_actual_rewrite_cache_evidence(self):
        self.model = {}
        with self.assertRaisesRegex(boundary.ReleaseError, "rewrite/refplan/cache"):
            boundary.scoped_containment(self.report, self.proofs, self.binding)



if __name__ == "__main__":
    unittest.main(verbosity=2)
