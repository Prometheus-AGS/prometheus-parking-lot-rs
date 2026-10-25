#!/usr/bin/env python3
"""OA-18 discriminating packet predicates. No mock can certify real scenarios."""
from __future__ import annotations

import copy
import importlib.util
import json
from pathlib import Path
import sys
import unittest

sys.dont_write_bytecode = True
SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
spec = importlib.util.spec_from_file_location("boundary", SCRIPTS / "operational-assurance.py")
boundary = importlib.util.module_from_spec(spec)
spec.loader.exec_module(boundary)


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


if __name__ == "__main__":
    unittest.main(verbosity=2)
