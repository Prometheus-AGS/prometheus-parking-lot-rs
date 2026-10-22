# Documentation index

Start with the [README](../README.md): what the crate does, its crates and
features, and compiled examples. Everything in this directory outside
[`archive/`](archive/) describes the crate as it is on `main`; anything planned
is marked as planned and names its OpenSpec change.

## Using the crate

| Document | What it covers |
| --- | --- |
| [README](../README.md) | Overview, crates and features, quick start, `ResourcePool` and `WorkerPool` examples |
| [API.md](API.md) | Map of the public API by module, with features |
| [CHANGELOG](../CHANGELOG.md) | What changed in each release, including migration notes |
| [WHAT_WHY.md](WHAT_WHY.md) | Why the crate exists: the problem and the intended guarantees |
| [Docs site](../website/docs/intro.md) | Concept chapters (theory and history); guides arrive with cr-47 |

## Design

| Document | What it covers |
| --- | --- |
| [adr/0001-workspace-split.md](adr/0001-workspace-split.md) | Crate map, dependency rules, facade features |
| [adr/0002-core-contracts.md](adr/0002-core-contracts.md) | Error taxonomy, fallible executors, clock, ordering, cost vector |
| [adr/0003-backend-contracts.md](adr/0003-backend-contracts.md) | Async `LeaseQueue` and `ResultMailbox` contracts and the conformance suite |
| [ARCHITECTURE.md](ARCHITECTURE.md) | System context and deployment scenarios |
| [PARKING_LOT.md](PARKING_LOT.md) | The parking-lot algorithm and the planned skip-ahead and multi-resource admission (cr-20, cr-23) |
| [update-plan-10-22-2026.md](update-plan-10-22-2026.md) | The audit and plan of record for `phase-correctness-and-durability` |

## Working on the crate

| Document | What it covers |
| --- | --- |
| [agent-context.md](agent-context.md) | Layout, commands and hard rules for contributors and agents |
| [agent-team.md](agent-team.md) | Roles, owned paths and worktrees |
| [skill-routing.md](skill-routing.md) | Which skills to load for which work |
| [build-performance.md](build-performance.md) | Build caching and disk use |
| [coding-standards/README.md](coding-standards/README.md) | Pragmatic Rust guidelines |
| [PUBLISHING.md](PUBLISHING.md) | Release playbook (0.2 era; cr-52 replaces it) |
| [TODO.md](TODO.md) | Where open work is tracked |

## Configuration and migrations

- Sample scheduler configuration:
  [`examples/configs/scheduler-config.json`](../examples/configs/scheduler-config.json),
  parsed by `config::SchedulerConfig::from_json_str` and turned into pools by
  `builders::build_pools`. The configuration selects backend kinds and limits;
  the application supplies the queue, mailbox and executor factories.
- Postgres schema: [`crates/parking-lot-pg/migrations/`](../crates/parking-lot-pg/migrations/).
