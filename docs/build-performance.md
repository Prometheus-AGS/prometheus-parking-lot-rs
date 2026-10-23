# Build performance and disk use

Owner: `platform-engineer`. Measured on 2026-10-22 (macOS, Rust 1.91.1, Samsung T7); the workspace-loop
numbers below are from 2026-10-22 on Rust 1.99.1 after the workspace split (cr-48).
Update the numbers when they change; do not keep unmeasured claims here.

## What is already configured (user-level `~/.cargo/config.toml`)

| Setting | Effect |
|---|---|
| `build.build-dir = "/Volumes/T7/rust/cargo-build/{workspace-path-hash}"` | Intermediate artifacts on the T7, **one build-dir per checkout path** — every worktree gets its own. |
| `rustc-wrapper = sccache`, `SCCACHE_DIR=/Volumes/T7/rust/sccache`, 100 GB | Cross-checkout cache for non-incremental crates. |
| `[cache] auto-clean-frequency = "7 days"` | Registry/git cache GC. Nothing GCs build-dirs. |
| `profile.dev`: incremental, `debug = "line-tables-only"`, CGU 256; deps `opt-level = 2` | Fast incremental rebuilds of this crate; optimized deps. |

## Measurements

| Item | Value |
|---|---|
| Build-dir for this repo, default features | 940 MB |
| All build-dirs on the T7 | 113 GB across 23 workspaces |
| sccache size | 9.0 GB |
| sccache Rust hit rate (lifetime) | **2.68%** of 5,910 compile requests |

Workspace loop, 2026-10-22, 1.99.1, fresh worktree build-dir (first run builds dependencies):

| Command | Wall time |
|---|---|
| `cargo ck` (cold) | 1m15s |
| `cargo lint` (after `ck`, same worktree) | 3.5s |
| `cargo t` (cold, builds test profile) | 55s; the 64 tests themselves take ~2.2s |

Wasm loop (cr-38), 2026-10-22, 1.99.1, `wasm32-unknown-unknown`, dependencies already built, other
agents building on the same machine (so treat as upper bounds):

| Command | Wall time |
|---|---|
| `cargo check --target wasm32-unknown-unknown -p core -p wasm`, dependencies cold | 29s to 46s |
| `cargo wasm-ck` (`--all-targets`, includes the wasm-bindgen-test crate) | 19.6s |
| `cargo wasm` after touching `pool.rs` (rebuild, wasm-bindgen-test-runner, Node.js); the 10 tests take 0.3 to 0.9s | 4.4s |
| `cargo +1.95 check` of the same two crates (MSRV, cold) | 2m58s |

The wasm target has its own build-dir subtree (`target/wasm32-unknown-unknown`), so native and wasm
loops do not invalidate each other; do not mix `RUSTFLAGS` between them. The negative-control run
(`RUSTFLAGS='--cfg parking_lot_negative_control="s-4"'`) rebuilds every dependency: run it once, not in a loop.

The rows measured before the workspace split (940 MB, 113 GB, 2.68%) are the 2026-10-22
pre-split baseline and are kept for comparison; re-measure after CI caching lands.

The low hit rate is the main finding. sccache never caches incremental crates (this
crate in dev), `proc-macro`/`bin`/`cdylib` crates, or anything whose absolute paths
differ — and each build-dir has a different path.

## Rules for agents

1. **One canonical feature set per worktree.** A different feature set, profile or
   `RUSTFLAGS` produces new artifact hashes; alternating them in one worktree rebuilds
   everything. Because each worktree has its own build-dir, roles may differ from each
   other — just never alternate inside one worktree.

   | Role | Loop commands (aliases in `.cargo/config.toml`) |
   |---|---|
   | scheduler, events, tech-lead | `cargo ck`, `cargo lint`, `cargo t` |
   | postgres | `cargo ck-pg`, `cargo lint-pg`, `cargo t-pg` (adds `--features postgres`) |
   | platform | the default aliases locally; `--all-features` and cross targets only in CI |

   Every alias passes `--workspace --locked`: the repository root is both the facade
   package and the workspace root, so a bare `cargo check` only covers the facade.
   `postgres` is a feature of the facade package (`prometheus_parking_lot`), not of
   `prometheus-parking-lot-pg`, which is always built under `--workspace`.

2. **One Cargo process per worktree**, and at most **4 worktrees building at once**.
   `build.jobs = 10` applies per process, so 4 parallel builds already oversubscribe;
   set `CARGO_BUILD_JOBS=3` in an agent shell when 3+ agents build concurrently.
3. **Never** `cargo clean`, `--release`, or cross-compile during implementation.
4. Tests: `cargo t` (nextest) — faster and per-test process isolation. nextest does not run
   doctests; CI also runs `cargo test --workspace --doc --locked`.

## CI (`.github/workflows/ci.yml`)

- `Swatinem/rust-cache@v2` with a `shared-key` per job (`test-<toolchain>` per matrix leg) and
  `save-if` only on `main`, so pull requests restore but never write the cache.
- `--locked --workspace` on every cargo call; `CARGO_INCREMENTAL=0` (incremental artifacts
  are useless in a clean runner and bloat the cache).
- Jobs: fmt, clippy (pinned 1.99.1 only; 1.95 lacks the `unused_async_trait_impl` lint), test on
  1.99.1 and MSRV 1.95 via nextest (build step separated so the 5-minute watchdog bounds test
  execution only), docs with `-D warnings`, cargo-deny, coverage (cargo-llvm-cov).
- `wasm` job (cr-38), pinned toolchain and MSRV: `cargo check`/`test` for `wasm32-unknown-unknown`;
  on the pinned leg also `.github/scripts/check-wasm-graph.sh`, wasm clippy and wasm docs. Cache key
  `wasm-<toolchain>`. `wasm-bindgen-test-runner` is installed prebuilt (`taiki-e/install-action`, no
  compile) at the version of `wasm-bindgen` in `Cargo.lock`; a runner of another version rejects the
  test binary.
- `deny` runs with `--workspace` (cr-44). Without it cargo-deny walks only the facade package's graph, so
  a crate the facade does not depend on (the FFI crate) was never checked.
- `Platform` workflow (`.github/workflows/platform.yml`, cr-44), separate from `ci.yml` and behind a
  workflow-level `paths` filter (plus push to `main`, a weekly schedule and manual dispatch): `android`
  (ubuntu: `cargo ndk` for arm64-v8a and x86_64, then Kotlin generation and a JVM smoke test), `apple`
  (macOS: iOS device and simulator builds, then Swift generation and a host smoke test), `desktop` (macOS:
  the Tauri example's fmt, clippy and restart test) and `bindgen` (the generator's fmt and deny). The FFI
  crate is also a root-workspace member, so `ci.yml`'s fmt, clippy, nextest, docs, deny and MSRV legs cover
  it on every pull request; the Platform jobs add only other targets and toolchains. Cache keys:
  `android`, `apple`, `desktop`; `save-if` on `main` as everywhere else.
- No CI timings measured yet; record the first green main run and a cache-hit run here.

## Mobile and desktop bindings (cr-44)

Measured 2026-10-23 on the same Mac as above, 1.99.1, other agents building concurrently (upper bounds).

| Item | Value |
|---|---|
| New packages in the root `Cargo.lock` | 14 (13 third-party: `uniffi*`, `toml` and friends, `camino`, `fs-err`, `siphasher`, `static_assertions`) |
| Cold dev build of those 13 crates plus `prometheus-parking-lot-ffi` (`--timings`, fresh target dir) | 46 CPU-seconds of 304 for the whole `ffi` lib graph; 54s wall for the graph |
| `cargo ck` (warm) after the crate was added | 6.8s |
| `cargo clippy --workspace --all-targets --all-features`, warm, after the last edit | 5.2s |
| `cargo nextest run --workspace --all-features`, 256 tests (10 are the FFI crate's, 5 of them restart tests) | 6.7s of test time; 1m41s wall including the build |
| `cargo ndk -t arm64-v8a build -p ...-ffi`, cold dependencies (NDK 28) | 1m38s |
| `cargo ndk -t arm64-v8a -t x86_64 build`, x86_64 leg after arm64 was built | 20s |
| `cargo build --target aarch64-apple-ios` / `aarch64-apple-ios-sim`, cold dependencies | 24s / 41s |
| `bindings/generate.sh` (host library, generator already built, Kotlin and Swift) | 43s, of which the host library build was 18s |
| `bindings/uniffi-bindgen`, cold build | 1m42s (clap, askama and the generators) |
| `bindings/tauri-desktop`, `cargo check` cold (Tauri 2.12 on macOS) | 2m57s |
| `bindings/tauri-desktop`, `cargo test` of the restart tests, warm | 2.7s |
| Debug `libparking_lot_ffi.so` (aarch64 android) / `.a` (ios) | 22 MB / 60 MB (unstripped, with debug info) |

Why the generator and the Tauri example are outside the root workspace: the generator brings clap and
askama, Tauri brings several hundred crates. Inside the root workspace every `--workspace` job (clippy,
nextest, docs, coverage, the MSRV leg) would compile them; standing alone they cost the main build nothing
and are built only by `bindings/generate.sh` and the Platform workflow. The price is two more `Cargo.lock`
files to keep current and a duplicated `target/` namespace (`.cargo/config.toml` sends all of them to the
repository's `target/`; intermediates stay in the per-workspace build-dir).

`rust-cache` for the root workspace also covers the generator's artifacts (they land in the same `target/`);
the `desktop` job passes `workspaces: "bindings/tauri-desktop -> ../../target"` so the Tauri lockfile is part
of its key.

## Disk hygiene

- Each implementer worktree costs ~1–3 GB of build-dir (more with `postgres`).
- Remove finished worktrees with `scripts/agent-worktree.sh remove <role>`; their
  build-dirs are not removed automatically.
- Weekly: `cargo sweep --time 14 --recursive /Volumes/T7/Projects/prometheus` and
  review `find /Volumes/T7/rust/cargo-build -mindepth 2 -maxdepth 2 -type d -mtime +21`
  before deleting (hash → path mapping is not recorded, so check mtimes).

## Experiments for the platform engineer (measure before adopting)

| Idea | Expected effect | Status |
|---|---|---|
| `SCCACHE_BASEDIRS` covering `/Volumes/T7/rust/cargo-build` and the worktree parent | Path-independent cache keys → dependency hits across worktrees | sccache 0.18 has the variable; Rust effect unverified |
| `CARGO_INCREMENTAL=0` in agent shells | Lets sccache cache this crate across worktrees, at the cost of incremental speed | Measure rebuild time both ways |
| `[profile.dev.build-override] opt-level = 0` | Faster build-script/proc-macro compiles | sea-orm is gone and the sqlx `macros` feature is off (raw queries), so the cost falls on serde/thiserror/uniffi derives; measure |
| `split-debuginfo = "unpacked"` (macOS default) vs current `"off"` | Link time / disk | Unverified on macOS |
| Workspace split (core / pg / embedded / iggy / ffi / wasm) | Smaller rebuild scope; natural ownership | Done: ADR 0001 (cr-48); core, runtime-tokio, pg, embedded, wasm and ffi crates exist; no iggy crate yet |
| CI: `Swatinem/rust-cache@v2` with `save-if: main`, `--locked`, `CARGO_INCREMENTAL=0` | Reproducible cached CI | Adopted in `.github/workflows/ci.yml` (cr-42) |

Sources: Rust blog build-dir layout v2 (2026-03-13); sccache README v0.18.0; cargo
issues #5931, #5026, #4282, #16147, #16804; Swatinem/rust-cache.
