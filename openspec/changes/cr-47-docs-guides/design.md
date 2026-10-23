## Context

cr-47 writes the scenario guides, the agentic-value chapter update and the API
reference page, with every code sample compiled in CI. All dependencies
(cr-46, cr-37, cr-29, cr-44, cr-32, cr-23) and the stretch change cr-33 are
merged, so the streaming-replay guide is in scope.

## Decisions

1. **One example program per guide, excerpts verified, not duplicated.** Each
   guide's Rust blocks carry `title="examples/<file>.rs"` and must be a
   verbatim, contiguous excerpt of that file
   (`.github/scripts/check-doc-snippets.py`, stdlib Python; a negative control
   with a changed line and an untitled block fails it). Doctests were rejected
   for the guides: the site does not compile Markdown, and the README already
   carries the doctested quick-start examples.
2. **Examples run, not only build.** `.github/workflows/docs-examples.yml`
   builds every example once (`--all-features`) and runs all eight: four need
   no server, two run against `parking-lot-pg:18`, two against the pinned
   `apache/iggy:0.9.0`. A separate workflow file, not a job in `ci.yml`, so it
   does not conflict with platform-engineer's concurrent CI work (cr-53).
   platform-engineer was busy, so product-manager wrote it (task 1.3 changed
   from "hand off" to "write").
3. **No new files under `examples/configs/`.** The config test asserts that
   directory's exact file list, which is test code product-manager does not
   edit. New configurations are shown as JSON blocks in the guides, which
   `config_every_json_example_in_the_docs_loads` loads, and the examples build
   theirs in code through `SchedulerConfig::from_json_str`.
4. **API reference: link docs.rs; do not host rustdoc on the site.** docs.rs
   documents each published version with the crates' docs.rs metadata
   (all features; wasm target for the wasm crate); a site-hosted copy would
   document `main`, not the installed release, and would add a Rust
   all-features build to the Pages deploy. Cost: until cr-52 publishes 0.3.0
   the links return "not found"; the page says so and gives the local
   `cargo doc` command.
5. **Tauri guide points at `bindings/tauri-desktop`** and adds the
   durable-pool option (facade, `embedded`) backed by `shutdown_requeue.rs` and
   `disconnected_client.rs`, rather than duplicating the Tauri app.
6. **Guide pages:** `scenarios.md` keeps its id and becomes the chooser page,
   so no existing link breaks; the nine guides and the upgrade notes are new
   pages in the sidebar.

## Library API gaps found

Status as of the pre-release API-gaps change (`team/tech-lead/prerelease-api-gaps`):

- **Fixed:** `BackgroundNotifier` and its companions are re-exported by the
  facade (`core::backend`, `core::*`); `iggy_events.rs` imports only through
  the facade.
- **Open, 0.4.0:** a configuration-built durable pool cannot take a notifier, a
  result TTL, or heartbeat/poll intervals, and ignores the Postgres `listen`
  listener (it polls every 500 ms). Tracked in #52, #53 and #54.
- **Fixed:** `IggyBackendConfig` has a `stream_topic` field.
- **Fixed:** `CapacityExceeded` names the configured pool and the resource kind
  that did not fit (`kind: Option<ResourceKind>`).
- **Open, 0.4.0:** a `SchedulerConfig` with no pools cannot be loaded (#55).
