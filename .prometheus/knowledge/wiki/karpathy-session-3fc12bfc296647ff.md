---
type: SessionRecord
id: karpathy-session-3fc12bfc296647ff
title: Karpathy session 3fc12bfc2966
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-22T16:30:24.182097+00:00
created_at: 2026-10-22T16:30:24.182097+00:00
updated_at: 2026-10-22T16:30:24.182097+00:00
revision: 0
---

## Delta

The fix is back, with a confirmed root cause: two waits on one deadline, where whichever fires first decides the error. Its new regression test fails at iteration 0 on the old code and passes with the fix. Re-checking it myself before opening its PR.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: unknown
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-22T16:29:35.994641Z
- Project: /Volumes/T7/Projects/prometheus/prometheus-parking-lot

## Changed Paths

- No changed paths detected.
