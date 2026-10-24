# OA-15 native shell and actual macOS GUI protocol

Status: **NOT_EXERCISED**. These are future final-boundary instructions, not
executed commands or GUI evidence. No browser/app, build, script or deletion
was run to author this document. The uncomfortable limitation is that a trace
can prove backend activity while the user never saw a usable window; actual
GUI captures and attributable observation must therefore accompany it.

The [protocol](protocol.json) and [fixture tool](desktop-assurance.py) implement
the [OA-15 contract](../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/adopted-contract.json).
The [operator gates](../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/operator-gates.md)
accept three native OS builds plus at least macOS actual GUI restart evidence.
G-06 host/runner access remains unverified; no paid runner, global installation
or account/host change is authorized by this runbook. The driver batches
concrete operator prerequisites after implementation/review, not per edit.
G-01 credential rotation and KBD signer replacement remain deferred/non-blocking.

## Required evidence matrix

| Fixture ID | Real path | Required final observation |
|---|---|---|
| `OA15-BUILD-MACOS` | Native macOS Tauri shell build | Actual binary artifact, exit 0, frozen source/tool/OS identities |
| `OA15-BUILD-LINUX` | Native Linux Tauri shell build | Same native-shell artifact requirements; not a macOS cross-build |
| `OA15-BUILD-WINDOWS` | Native Windows Tauri shell build | Same native-shell artifact requirements; not just queue-service compilation |
| `OA15-GUI-RESTART-PERSISTENCE` | Real macOS main window, two app processes | Submit/process before full quit; retrieve the same persisted result once after restart; actual captures/observer receipt |
| `OA15-GUI-STORE-DELETION-CONTROL` | Separate real macOS fixture, same two-process path | Delete only its closed owned store; actual post-restart retrieval fails with the named missing-result classifier |

Linux/Windows **runtime** is not claimed by these builds. Such a claim requires
corresponding actual launch evidence before it is made. The delivered manual
GUI wrapper is scoped to macOS; it does not pretend to drive another OS.
Queue-service restart tests remain useful regressions but do not satisfy either
GUI fixture. UI layout, CSS, HTML and JavaScript remain incumbent.

## Preconditions and native build discovery

Finish the entire production/documentation set and freeze a clean full candidate
before executing. The fixture checks clean status and binds its source snapshot;
adding/changing this runbook after a build also requires a new matching build
receipt. Use existing approved hosts, Python 3.11+, preinstalled Rust `1.99.1`
and fresh output directories outside the checkout. Serialize Cargo commands
within a target directory. `RUSTUP_AUTO_INSTALL=0` prevents implicit toolchain
installation by the wrapper.

| Host | Prerequisites; none installed here |
|---|---|
| macOS | Approved machine, Xcode command-line tools/SDK; for GUI, an actual logged-in graphical desktop and operator able to inspect only this test app |
| Linux | Native compiler/linker, `pkg-config`, WebKitGTK 4.1, GTK3 and OpenSSL development libraries; wrapper queries `webkit2gtk-4.1`, `gtk+-3.0`, `openssl` |
| Windows | MSVC C++ build tools and Windows SDK; WebView2 additionally required before claiming an actual runtime launch |

Run this from the repository root on **each native OS**, substituting a fresh
OS-specific `$NATIVE_BUILD_OUT` (on PowerShell, supply an actual path or its
native variable syntax):

```bash
python3 bindings/tauri-desktop/assurance/desktop-assurance.py build \
  --output "$NATIVE_BUILD_OUT"
```

The actual production build argv recorded by the wrapper is:

```bash
cargo +1.99.1 build --manifest-path bindings/tauri-desktop/Cargo.toml \
  --workspace --bins --locked --message-format=json-render-diagnostics
```

No release profile, cross target or guessed output filename is used. The
wrapper inspects actual Cargo `compiler-artifact` JSON, requires exactly one
distinct emitted `parking-lot-tauri-example` target of kind `bin`, checks that
its executable exists and records its actual path/SHA256. That nonzero native
artifact discovery is separate from any GUI launch discovery. Successful
build output is `BUILD_PASS: <native case ID>; GUI runtime NOT_EXERCISED` and
`build-receipt.json` retains `runtime_status: NOT_EXERCISED`.

[Platform CI](../../../.github/workflows/platform.yml) has macOS/Linux/Windows
native legs and raw artifacts `oa15-build-macos`, `oa15-build-linux`,
`oa15-build-windows`. A preprovisioned approved Linux runner may be selected with
`DESKTOP_LINUX_RUNNER`; no apt step is added. Existing macOS formatting, Clippy
and queue-service regression commands are preserved and have no GUI claim.
No workflow has been dispatched by this implementation.

The desktop lock pins Tauri `2.12.5`, tauri-build `2.7.4`, wry `0.57.1`,
webkit2gtk `2.0.2`, webview2-com `0.39.1`. The pinned build source requires an
ICO on Windows. `icons/icon.ico` contains the existing 128x128 PNG bytes inside
an ICO container; no pixel or frontend design change was made. Since no
`devUrl` is configured, the pinned context generator embeds the incumbent
`frontendDist` assets even in this native debug build.

## Private fixture ownership and profile

Use the actual matching macOS build receipt and executable on the operator's
machine. The tool verifies candidate/source/binary agreement before preparation.
Select an existing external parent the operator owns; the tool creates a fresh
private `parking-lot-oa15-*` directory with a random nonce, marker and explicit
case. Never point it at the normal app-data directory or a production store.

Only `PARKING_LOT_DESKTOP_ASSURANCE_ROOT` activates fixture behavior. The wrapper
sets it for the owned application child; no global environment/account setting
changes. Normal launches retain the incumbent per-user app-data path and write
no fixture trace. An invalid explicit marker/path refuses startup rather than
falling back to user data. Canonical root/store paths and regular-file checks
reject redirected symlinks. The fixture's window uses Tauri's incognito setting
for nonpersistent webview storage; the explicitly owned redb file is the
durable state under test. No production profile or store is deleted.

The source trace is a synced seven-column TSV: timestamp milliseconds, unique
process session, PID, event kind, window label, task ID (`-` when absent) and
UTF-8 data encoded as hex. It records main-webview submit/process/collect/result
commands and actual finished local page loads. Hex encoding is framing, not
redaction: use only the printed non-secret synthetic input, never user data.
Trace write errors fail commands or exit the owned fixture app with an error.

## Positive actual GUI operation

1. Prepare the independent positive case. Copy the exact canonical `root` and
   `input` printed in its JSON; below, `$POSITIVE_ROOT` means that printed root,
   not a guessed path. Preparation also writes a pending observation document.

   ```bash
   python3 bindings/tauri-desktop/assurance/desktop-assurance.py prepare \
     --case positive --build-receipt "$MAC_BUILD_OUT/build-receipt.json" \
     --parent "$EXTERNAL_FIXTURE_PARENT"
   ```

2. Launch the actual built executable. This wrapper blocks until that app
   process exits and preserves its actual argv/PID/exit, raw stdout/stderr and
   store digests. It does not click controls or invoke backend commands itself.

   ```bash
   python3 bindings/tauri-desktop/assurance/desktop-assurance.py launch \
     --root "$POSITIVE_ROOT"
   ```

3. In the **visible Parking Lot Queue Example main window**, replace the
   textarea with the exact printed `oa15-<nonce>` input. Click **Add to queue**
   once, then **Process next** once, waiting for the normal status update after
   each. Do not click Collect results in this first session. Manually capture
   only the test-app window showing the input and processed state. The task ID
   is available in the first session's actual submit/process TSV records; the
   incumbent UI does not display it until collection. Do not claim it was
   visually displayed before collection.

4. Quit the **entire app**, using Cmd+Q on macOS. Closing/reloading a page is
   not an app restart. Wait until the wrapper returns `APP_EXIT` with actual
   exit 0 and records the closed process in `launches.json`. A still active or
   unresolved launch prevents the next launch/deletion. An interrupted or
   failed process is not successful GUI evidence.

5. Run the same `launch --root "$POSITIVE_ROOT"` command again. In the actual
   new process/main window, do **not** resubmit or process. Click **Collect
   results** once. Visually verify the same logged task ID and exact uppercase
   synthetic input. Click Collect results a second time; no duplicate list
   item should appear. Capture the actual result list. Quit fully and wait
   again for the wrapper's actual exit 0.

6. Attach the two actual PNG/JPEG captures, preserving their contents. The
   fixture copies/hashes the files and does not manufacture or judge pixels.

   ```bash
   python3 bindings/tauri-desktop/assurance/desktop-assurance.py capture \
     --root "$POSITIVE_ROOT" --stage first-processed --image "$POSITIVE_BEFORE_IMAGE"
   python3 bindings/tauri-desktop/assurance/desktop-assurance.py capture \
     --root "$POSITIVE_ROOT" --stage second-collected --image "$POSITIVE_AFTER_IMAGE"
   python3 bindings/tauri-desktop/assurance/desktop-assurance.py classify \
     --root "$POSITIVE_ROOT"
   ```

Positive trace classification requires two actual successful process sessions,
matching PIDs/store paths, local main-page loads, exactly one matching first
submit/process and no first-session collection. The second session must contain
no submit/process, exactly one matching uppercase result and collect counts
`1`, then `0`. The store before the second launch must match the file after
the first closed process. Required classifier: exit 0 and `TRACE_ORACLE_PASS`;
`trace-oracle.json` still says
`gui_certification: PENDING_INDEPENDENT_OPERATOR_OBSERVATION_REVIEW`.

## Separate owned-store deletion control

Prepare a **separate root**, not the positive root:

```bash
python3 bindings/tauri-desktop/assurance/desktop-assurance.py prepare \
  --case delete-store-control --build-receipt "$MAC_BUILD_OUT/build-receipt.json" \
  --parent "$EXTERNAL_FIXTURE_PARENT"
python3 bindings/tauri-desktop/assurance/desktop-assurance.py launch \
  --root "$CONTROL_ROOT"
```

Use this root's own printed input. Repeat the first-session Add to queue,
Process next, no Collect results, actual capture and full quit. Wait for actual
successful process exit. Apply only the narrowly authorized control:

```bash
python3 bindings/tauri-desktop/assurance/desktop-assurance.py delete-store \
  --root "$CONTROL_ROOT"
```

The command checks the exact control case, owner nonce/candidate, canonical
external root, source identity, one successful closed process, actual matching
submit/process/page-load trace, absence of an active/unresolved child, and a
nonempty regular store whose digest is unchanged since that process exited.
It records intent, unlinks **only** `$CONTROL_ROOT/store/parking-lot.redb` and
records `OWNED_STORE_REMOVED`. It has no recursive deletion, glob, profile
deletion, app-data resolution or arbitrary target option. A positive root,
symlink, changed store, active process or repeated deletion refuses the action.
Never replace this command with `rm -rf`, deletion of a parent, or deletion of
a normal user/application directory. No fixture store has actually been
created/deleted by authoring this runbook.

Run `launch --root "$CONTROL_ROOT"` a second time. Do not submit/process.
Click Collect results in the actual main window and capture its empty result
state. Quit fully and wait for actual exit 0. Attach both control captures,
then invoke the retrieval oracle:

```bash
python3 bindings/tauri-desktop/assurance/desktop-assurance.py capture \
  --root "$CONTROL_ROOT" --stage first-processed --image "$CONTROL_BEFORE_IMAGE"
python3 bindings/tauri-desktop/assurance/desktop-assurance.py capture \
  --root "$CONTROL_ROOT" --stage second-collected --image "$CONTROL_AFTER_IMAGE"
python3 bindings/tauri-desktop/assurance/desktop-assurance.py classify \
  --root "$CONTROL_ROOT"
```

This last command **must exit 1**, with exact classifier
`OA15_PERSISTED_RESULT_MISSING_AFTER_RESTART` and `expected_negative=True`.
The actual oracle file must identify the deletion-control case, contain the
missing-result FAIL, require the deletion receipt/digest and observe empty
collects/no results after a second launch whose store initially was absent.
Capture that deliberate nonzero exit as the detected control. An arbitrary
error, malformed trace, missing capture, app crash, store-open failure or
absence of a GUI collect command is not this classifier and cannot pass it.
If the deleted-store case retrieves the old result, the control was not
detected and the assurance scenario fails.

## Manual observation and final certification

For **each** root, the actual authorized observer completes
`manual-observation.md` with identity/role, UTC times, macOS version/build,
architecture/display session, binary/candidate identity, actual first/second
launch PIDs, exact input/task/result, visible actions/outcomes, full quits and
capture paths/digests. Preserve the pending original and append attributable
observations or retain its digest in the boundary packet. Do not simply replace
NOT_EXERCISED with PASS; record what was actually seen and any deviation.

An independent final-boundary reviewer joins that attributable observation,
actual captures, raw wrapper/launch logs and backend trace with the three native
build receipts. Capture hashes prove file identity, not truthful image content
or operator authority. The fixture deliberately does not accept `approved`
JSON, emit GUI PASS, or promote trace-only success. The OA-18 producer may mark
the scenario PASS only after the required evidence is actually reviewed and
bound to the frozen candidate/contract/checked manifest. Native computer APIs
are disabled in this authoring session; the missing actual GUI observation
remains pending, never silently replaced by a browser or queue-service test.

Retain the actual build `commands.json`, numbered raw logs and build receipt;
each fixture's owner/manifest, `launches.json`, both launch logs, TSV,
`trace-oracle.json`, capture index/images, observer document and control deletion
receipt. Preserve outer argv/actual exits/stdout/stderr and timestamps as well.
No cleanup deletes the evidence or fixture root automatically. Later cleanup
is an operator-owned, separately scoped decision after evidence retention.

Record real Rust/Python/Tauri/OS/SDK/compiler/webview and approved runner/image
identities, binary hash, source/lock/protocol hashes, candidate and contract.
Build source snapshots also include relevant core/embedded/FFI source and the
desktop frontend/config/icon. Changes invalidate affected receipts. The
authored implementation snapshot is recorded in the driver-owned
[task 1 handoff](../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/evidence/implementation/oa-15-task-1-handoff.json);
it is not a runtime result.

Explicit missing tools/libraries/SDK, unsupported native build OS, non-macOS
manual GUI path or missing GUI captures return BLOCKED/exit 2 where the wrapper
has that classifier. Observed build/source/trace/capture/assertion failures
return FAIL/exit 1. A missing display may surface as an actual app launch error;
retain its raw reason and record unavailable G-06 access as BLOCKED in the
boundary packet. Such errors cannot satisfy the deletion negative control.
All five case outcomes and GUI certification remain NOT_EXERCISED today.
