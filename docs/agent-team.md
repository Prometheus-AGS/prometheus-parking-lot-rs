# Agent team — `parking-lot-core`

The team that delivers `phase-correctness-and-durability` from
`docs/update-plan-10-22-2026.md`. Source of truth: `.agent-team/parking-lot-core/team.json`
(regenerate native files from it; do not hand-edit them). Routing pointer: the
`prometheus-team-routing` block in `AGENTS.md`.

## Why this shape

Evidence (2025–2026): multi-agent setups gain on parallel work but lose 39–70% on
tightly coupled sequential work, cost roughly 15× chat tokens, and fail mostly on
missing verification and spec drift (Google scaling study arXiv 2512.08296; Anthropic
multi-agent research system; MAST arXiv 2503.13657; Claude Code agent-teams docs:
start with 3–5 teammates, one file owner each). So the team is: one integrator who is
the single writer for shared files, a product owner who writes no code, four
implementers on disjoint modules, and one independent reviewer. Tightly coupled
scheduler logic stays with one owner.

## Roster

| Role | Expertise | Owns (summary) | Plan items | Claude model |
|---|---|---|---|---|
| `product-manager` | Product management, specs | `openspec/`, `CHANGELOG.md`, `README.md`, product docs | CR-16, CR-17, acceptance criteria for all | opus |
| `tech-lead` | Systems architecture, public API, integration | `Cargo.toml`, `src/lib.rs`, `src/config`, `src/builders`, `src/core/contracts/**`, error/executor, `docs/ARCHITECTURE.md`, ADRs | CR-18, CR-24, CR-37, contracts for CR-25/CR-31, workspace-split ADR | opus |
| `scheduler-engineer` | Threading, concurrency, memory model | `resource_pool`, dispatcher, native `worker_pool`, shutdown, cancellation, scheduler tests, benches | CR-19..CR-23, C1–C5, C7 | opus |
| `postgres-engineer` | Postgres, sqlx, migrations | Postgres queue/mailbox, entities, `migrations/`, `tests/integration/` | CR-26, CR-27, CR-30, CR-36 | sonnet |
| `events-engineer` | Event systems, queues, Iggy | hooks, audit, memory/embedded queues and mailboxes, Iggy | CR-28, CR-29 (with scheduler), CR-31..CR-34 | sonnet |
| `platform-engineer` | Cross-platform Rust (desktop/mobile/wasm), CI, build performance, supply chain | `.github/`, `.cargo/config.toml`, toolchain, `deny.toml`, `.gitignore`, `Cargo.lock`, `src/runtime/`, wasm worker pool, FFI/wasm crates | CR-15, CR-35, CR-38, CR-39, mobile/desktop bindings | sonnet |
| `reviewer` | Independent Rust/async/SQL/security review | `.agent-team/reviews/**` only | merge gates | opus |

Ownership is a planning agreement, not a filesystem permission. Paths outside a role go
to the owner as a handoff with the exact diff. If the tech lead accepts the workspace
split, each implementer also owns its `crates/parking-lot-*` crate (listed in the manifest).

## Dependency and merge order

```text
product-manager (OpenSpec + acceptance)
        │
tech-lead (contracts merged first, per milestone)
        │
  ┌─────┼──────────────┬───────────────────┐
scheduler   postgres   events   platform        ← parallel, one worktree each
  └─────┴──────────────┴───────────────────┘
        │
reviewer (fresh context: diff + spec) ──► tech-lead merges in dependency order
```

M0 (containment, platform + product) → M1 (CR-18 contracts → scheduler CR-19 → CR-20..23)
→ M2 (CR-25 contract → postgres/events CR-26/27/28 → CR-29) → M3 (events) ; M4 platform
work runs in parallel after CR-25. After each merge, the others rebase.

## Running it

At most **4 implementers build at once** (CPU and disk, see `docs/build-performance.md`).
Create worktrees with `scripts/agent-worktree.sh add <role> <CR-id>`.

| Harness | Definitions | How roles are used |
|---|---|---|
| Claude Code | `.claude/agents/*.md` | Native subagents (`/agents`, or ask for the role by name). Not an experimental agent team. |
| Codex | `.codex/agents/*.toml` (underscore names, e.g. `tech_lead`) | Native subagents; Codex defaults `max_threads=6`, `max_depth=1`. |
| OpenCode | `.opencode/agents/*.md` | Native agents (`@role`). |
| Kimi Code | `.kimi-code/agents/*.md` | Native agents; per-role model is ignored — choose the model at invocation. |
| MiniMax Code | `.minimax/agents/<role>/agent.md` | Read from the user data dir: launch with `MINIMAX_DATA_DIR=$PWD/.minimax` (this replaces `~/.minimax` for that session — unverified whether login/config carry over) or copy the role dirs into `~/.minimax/agents/`. `mcode exec` cannot select a custom agent. |
| DeepSeek Harness | `.dsh/profiles/parking-lot-core-<role>/`, composition `parking-lot-core-team` | Persona profiles; experimental team package shares one process/workspace and has no per-member model or worktree isolation — use it for planning/review, not parallel builds. |
| Zed | none (no native agent export) | Zed reads the first instruction file it finds (`.cursorrules` before `AGENTS.md`); `.cursorrules` is a symlink to `AGENTS.md`, so Zed gets the same rules and team pointer. Follow role instructions sequentially. |

When a harness cannot delegate, apply one role's instructions at a time and say so.
Review done in the builder's own context is not independent review — run the
`reviewer` in a different model family or harness from the author.

All exports are source-verified only: native discovery and invocation have not been
tested in each harness.
