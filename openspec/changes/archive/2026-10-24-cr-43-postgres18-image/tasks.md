Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-43-postgres18-image / <task id>`.

## 1. Project Postgres 18 image (flint-forge pattern) with pgmq for tests and local dev

- [x] 1.1 Write docker/postgres18/Dockerfile (+ pg17 variant) modelled on flint-forge images/postgres18: PGDG base, pgmq from pinned tag, tuned conf, init scripts; no Flint-specific extensions
- [x] 1.2 Add a testcontainers helper that builds/reuses the image and exposes a pool; add a compose service for local use
- [x] 1.3 Hand platform-engineer the CI image-build step with layer caching
- [x] 1.4 Independent review at the merge gate (image pinning, extension availability)
