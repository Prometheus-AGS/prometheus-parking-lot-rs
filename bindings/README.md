# Mobile and desktop bindings

The durable queue and result mailbox of `prometheus-parking-lot-embedded` (a redb file,
ADR 0004) for Android, iOS and desktop apps. Nothing here is published to crates.io;
the pieces are:

| Path | What | In the root workspace? |
|---|---|---|
| `crates/parking-lot-ffi` | `prometheus-parking-lot-ffi`: the UniFFI crate. A small async API over the embedded store; builds as `lib` (Rust hosts), `cdylib` (Android, bindgen) and `staticlib` (iOS). | yes |
| `bindings/uniffi-bindgen` | The Kotlin/Swift generator (a two-line binary around `uniffi::uniffi_bindgen_main`). | no (own workspace and `Cargo.lock`) |
| `bindings/generate.sh` | Builds the FFI crate and generates the bindings into `bindings/generated/` (git-ignored). | - |
| `bindings/smoke/` | A Kotlin and a Swift program that run the generated bindings against the real library, including a close and reopen of the store. | - |
| `bindings/tauri-desktop` | The Tauri 2 example (`queue-service` holds the logic and the restart test; `src-tauri` is the window). | no (own workspace and `Cargo.lock`) |

The generator and the Tauri example are outside the root workspace so that their large
dependency trees (clap and askama; several hundred Tauri crates) are never compiled by the
`--workspace` jobs that run on every pull request. CI builds them in
`.github/workflows/platform.yml`.

## The API

Everything is `async` (Kotlin `suspend`, Swift `async throws`). The entry point is
`openStore(config)`, which returns a `ParkingLotStore`:

| Method | Does |
|---|---|
| `enqueue(task)` | Stores a task durably (fsynced before it returns). A repeated `idempotencyKey` stores nothing and reports `duplicate`. |
| `lease(max, leaseMs)` | Hands out ready tasks, highest priority first then first-in first-out. A leased task is invisible to other callers until acked, nacked, or its lease expires; then it is handed out again with a higher `attempt`. |
| `ack(lease)` / `nack(lease, disposition)` / `extendLease(lease, leaseMs)` | Settle or prolong a lease. A handle whose lease expired fails with `FfiError.LeaseLost`, so a slow worker cannot ack work another worker now owns. `nack` requeues (optionally after a delay) or dead-letters. |
| `depth()` | Ready, delayed, leased and dead-lettered counts. |
| `deliverResult(delivery)` | Stores a task's outcome in a mailbox. Deliver before acking: a crash in between reruns the task, and the idempotency key stops a second result being stored. |
| `consumeResult(mailbox)` / `peekResult(mailbox)` / `evictExpiredResults()` | Collect (once), read, or expire results. A result waits in the mailbox until the client collects it, so it reaches a client that was not running when the task finished. |

Delivery is at-least-once: make the work idempotent.

### Payloads are bytes

A payload is `ByteArray` (Kotlin) or `Data` (Swift), never interpreted. The queue does
not care whether it holds JSON, protobuf or a file reference. (Storage detail: the store
serialises as JSON, which has no byte type, so the crate stores payloads as hex text,
2 bytes per byte, instead of serde's default array of numbers at about 3.5. See
`crates/parking-lot-ffi/src/payload.rs`.) Text is the caller's UTF-8 encoding.

### Lifecycle rules

- **One open store per file**, within a process and across processes (redb locks the
  file). Release the object before reopening: Swift, drop the last reference; Kotlin,
  call `close()` (the generated classes are `AutoCloseable`, so `use { }` works). A
  second open of a held file fails with `FfiError.Backend`.
- **Open off the main thread.** `openStore` is `async` and does its file work on a
  background thread, but call it from a coroutine on `Dispatchers.IO` / a Swift `Task`
  rather than the UI thread.
- **Cancelling a call does not undo it.** The work runs on the crate's own tokio runtime
  and always finishes; a cancelled `enqueue` may still have stored the task. Use an
  `idempotencyKey` and retry.
- **App kill is safe.** Every change is one fsynced transaction; the next open recovers
  the file. The restart tests kill a process mid-run to show it
  (`crates/parking-lot-ffi/tests/restart.rs`).
- **Where to put the file.** Android: under `Context.filesDir` (not `cacheDir`, which the
  OS may empty). iOS: Application Support, and exclude it from iCloud backup if the queue
  should not roam between devices.

## Generating Kotlin and Swift

```sh
bindings/generate.sh              # debug library; PROFILE=release for a release one
```

This builds `prometheus-parking-lot-ffi`, builds `bindings/uniffi-bindgen`, and runs
`uniffi-bindgen generate --library <cdylib>` for each language (library mode: UniFFI reads
the interface out of the compiled library, so there is no `.udl` file to keep in step).
Output, all git-ignored:

```
bindings/generated/kotlin/uniffi/parking_lot_ffi/parking_lot_ffi.kt
bindings/generated/swift/parking_lot_ffi.swift
bindings/generated/swift/parking_lot_ffiFFI.h
bindings/generated/swift/parking_lot_ffiFFI.modulemap
```

The generator's version is pinned with `=` to the `uniffi` version in `Cargo.lock`, and the
script refuses to run when they differ. Bump both together.

To check the generated code works, not just exists:

```sh
bindings/smoke/swift.sh           # macOS; compiles main.swift + the bindings, runs them
bindings/smoke/kotlin.sh          # needs kotlinc and java; JNA and kotlinx-coroutines are
                                  # fetched and verified against pinned SHA-256 digests
```

## Android

```sh
cargo install cargo-ndk --version 4.1.2 --locked
rustup target add aarch64-linux-android x86_64-linux-android
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/<version>      # r28 or newer
cargo ndk -t arm64-v8a -t x86_64 -o app/src/main/jniLibs build --release -p prometheus-parking-lot-ffi
bindings/generate.sh                                     # then copy the .kt into the app's source set
```

- Use NDK **r28 or newer**: its linker defaults give the 16 KB page alignment Google Play
  requires for apps targeting Android 15+. The CI job prints the alignment of the built
  library.
- The Kotlin code needs JNA as an **AAR** on Android
  (`implementation("net.java.dev.jna:jna:5.17.0@aar")`) and
  `org.jetbrains.kotlinx:kotlinx-coroutines-android`. The library is loaded by the name
  `parking_lot_ffi` (`libparking_lot_ffi.so`).
- Keep `arm64-v8a` for devices; add `x86_64` for emulators on Intel hosts.

## iOS

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
cargo build --release --target aarch64-apple-ios     -p prometheus-parking-lot-ffi
cargo build --release --target aarch64-apple-ios-sim -p prometheus-parking-lot-ffi
bindings/generate.sh
mkdir -p build/headers
cp bindings/generated/swift/parking_lot_ffiFFI.h build/headers/
cp bindings/generated/swift/parking_lot_ffiFFI.modulemap build/headers/module.modulemap
xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/release/libparking_lot_ffi.a     -headers build/headers \
  -library target/aarch64-apple-ios-sim/release/libparking_lot_ffi.a -headers build/headers \
  -output build/ParkingLotFFI.xcframework
```

Add `ParkingLotFFI.xcframework` and `bindings/generated/swift/parking_lot_ffi.swift` to the
Xcode target. (The `xcodebuild` step was run against debug libraries and produces the
`ios-arm64` and `ios-arm64-simulator` slices; wiring the framework into an app project was
not exercised.) An Intel Mac simulator needs `x86_64-apple-ios` as a third slice.

## Desktop (Tauri 2)

A Tauri backend is Rust in the same process as the window, so it calls the store directly;
there is no bridge to cross. Depend on `prometheus-parking-lot-ffi` for the same
`ParkingLotStore` the mobile apps use (one tested surface for every platform), or on
`prometheus-parking-lot-embedded` and your own payload type if the app wants typed payloads
rather than bytes.

`bindings/tauri-desktop` shows the shape:

- The shell opens the store once in `setup`, at `app.path().app_data_dir()/parking-lot.redb`,
  and registers it as managed state. The data directory is per user and per app, and
  survives updates.
- Commands are thin: `submit`, `process_next`, `collect`, `status`. The logic lives in the
  `queue-service` crate so it can be tested without a window.
- A task is durable when `submit` returns; a result is durable when `process_next` stores
  it. Quit the app between them and the next launch finds both. The test is
  `bindings/tauri-desktop/queue-service/tests/restart.rs`:

```sh
cd bindings/tauri-desktop
cargo test --locked          # restart tests, no GUI
cargo check --locked -p parking-lot-tauri-example
```

Run the app itself with the Tauri CLI (`cargo install tauri-cli --version "^2" --locked`,
then `cargo tauri dev` from `bindings/tauri-desktop/src-tauri`); this needs a display and
was not run in CI. The window is a static page with no bundler, to keep the example about
the queue. On Linux install the Tauri system libraries first (webkit2gtk 4.1 and friends,
see the Tauri prerequisites); the CI job covers macOS only.

Two things a real app adds that the example leaves out: a worker loop that calls
`process_next` on a timer or when the queue is non-empty (the example runs it on a button),
and a lease length sized to the slowest task, extended with `extendLease` for long work.
