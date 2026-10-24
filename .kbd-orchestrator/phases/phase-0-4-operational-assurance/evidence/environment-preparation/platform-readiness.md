# Platform readiness observation

Phase `phase-0-4-operational-assurance`, Execute / OA-18 final boundary. This is a read-only environment observation taken while the parent serial Cargo matrix runs. Every platform profile remains **NOT_EXERCISED** by this task. The uncomfortable limitation is that healthy Xcode lookup and installed target libraries do not prove an actual native build or GUI persistence result.

The adjacent [JSON record](platform-readiness.json) contains exact argv, stdout/stderr/exit codes, selected public SDK path variables, executable path lookups, public Android component properties, Rust target-library file inventory, current model rows, and source SHA256 identities. Only these two evidence documents were authored. Existing verification trees and production sources were untouched.

## Observed commands

`xcode-select -p` ran first. SDK/version/find probes and the selected installed JDK executables were the only platform executables invoked. Their actual results follow; filesystem inventories used Python standard-library reads.

| Actual argv | Exit | Actual output |
|---|---:|---|
| `/usr/bin/xcode-select -p` | 0 | /Applications/Xcode.app/Contents/Developer |
| `/usr/bin/xcodebuild -version` | 0 | Xcode 26.6 / Build version 17F113 |
| `/usr/bin/xcrun --find swift` | 0 | /Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swift |
| `/usr/bin/xcrun swift --version` | 0 | Apple Swift version 6.3.3 (swiftlang-6.3.3.1.3 clang-2100.1.1.101) / Target: x86_64-apple-macosx26.0 swift-driver version: 1.148.6 |
| `/usr/bin/xcrun --find swiftc` | 0 | /Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc |
| `/usr/bin/xcrun --find clang` | 0 | /Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/clang |
| `/usr/bin/xcrun --sdk macosx --show-sdk-path` | 0 | /Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX26.5.sdk |
| `/usr/bin/xcrun --sdk macosx --show-sdk-version` | 0 | 26.5 |
| `/usr/bin/xcrun --sdk iphoneos --show-sdk-path` | 0 | /Applications/Xcode.app/Contents/Developer/Platforms/iPhoneOS.platform/Developer/SDKs/iPhoneOS26.5.sdk |
| `/usr/bin/xcrun --sdk iphoneos --show-sdk-version` | 0 | 26.5 |
| `/usr/bin/xcrun --sdk iphonesimulator --show-sdk-path` | 0 | /Applications/Xcode.app/Contents/Developer/Platforms/iPhoneSimulator.platform/Developer/SDKs/iPhoneSimulator26.5.sdk |
| `/usr/bin/xcrun --sdk iphonesimulator --show-sdk-version` | 0 | 26.5 |
| `/Library/Java/JavaVirtualMachines/jdk-18.jdk/Contents/Home/bin/javac -version` | 0 | javac 18 |
| `/Library/Java/JavaVirtualMachines/jdk-18.jdk/Contents/Home/bin/java -version` | 0 | java version "18" 2022-03-22 / Java(TM) SE Runtime Environment (build 18+36-2087) / Java HotSpot(TM) 64-Bit Server VM (build 18+36-2087, mixed mode, sharing) |

All three returned SDK paths exist as directories. No probe reported a missing SDK or license error. License acceptance for actual compilation/linking remains untested; no acceptance action was attempted.

## Installed files and profile limits

The host is `Darwin 25.6.0 x86_64`. The selected developer directory is `/Applications/Xcode.app/Contents/Developer`; `DEVELOPER_DIR` and `SDKROOT` are unset. `JAVA_HOME` selects the functional JDK18 above. Other installed JDK directories were inventoried from public `release` metadata without executing them.

The actual gate selects `/tmp/oa04-private-tools/rustup/toolchains/1.99.1-x86_64-apple-darwin`, a symlink whose read-only `Path.resolve()` result is `/Users/gqadonis/.rustup/toolchains/stable-x86_64-apple-darwin`. The driver supplied this gate selection; it is distinct from this task shell's unset ambient `RUSTUP_HOME`. Actual library-file counts in that resolved root are:

| Target | `libstd-*` files | Filesystem observation |
|---|---:|---|
| `x86_64-apple-darwin` | 3 | present |
| `aarch64-apple-ios` | 3 | present |
| `aarch64-apple-ios-sim` | 3 | present |
| `aarch64-linux-android` | 3 | present |
| `x86_64-linux-android` | 3 | present |
| `wasm32-unknown-unknown` | 2 | present |

The initial checkpoint mistakenly treated the unused global numeric1.99.1 directory as gate-selected and implied missing iOS/Android components. That implication is withdrawn. The JSON retains the separate global inventory with this explicit scope correction. No selected target component is missing in the actual private view. File presence does not prove successful cross-linking or executable version; the parent owns actual execution receipts. No Rustup, Cargo, or rustc command was invoked, and no component was added.

`ANDROID_HOME=/Users/gqadonis/Library/Android/sdk`; `ANDROID_NDK_HOME`, `ANDROID_NDK_LATEST_HOME`, and `ANDROID_SDK_ROOT` are unset. Installed NDK directories include21.1/21.4/23.0/23.1/24.0 and `28.0.12433566`; the latter's actual public metadata says `28.0.12433566-beta1` / `r28-beta1`. All have `darwin-x86_64` prebuilt hosts. SDK platform/build-tools directories reach API35; command-line tools `latest` metadata says22.0. PATH currently resolves the older `tools/bin/sdkmanager`, not `cmdline-tools/latest/bin/sdkmanager`. Neither sdkmanager nor adb was executed; no Android license contents were read.

`cargo-ndk` and `cargo-tauri` executable paths are present, but their identities/functionality were not executed. `kotlinc` is absent from PATH. The exact model pins cargo-ndk4.1.2 and kotlinc2.4.20. Current model Android rows require **Linux**; its `android-elf` implementation selects `toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-readelf`. This Mac's SDK cannot replace that profile. No model or script change is proposed.

## Missing prerequisites and remaining evidence

- For `ios`, Apple SDK lookup and the gate-selected device/simulator target library presence were observed. No missing local prerequisite was identified by these limited probes. Actual builds and Swift generated-binding smoke remain unexecuted here.
- Before `android`: a supported Linux environment with its selected Linux NDK, appropriate Linux-host Rust toolchain/targets, cargo-ndk4.1.2, kotlinc2.4.20, functional JVM and operator-established SDK/license readiness. Both Android target libraries are already present in this Mac gate's private selection; they are not a missing local component. These observations do not authorize acquisition or host changes.
- Before Linux/Windows native receipt claims: their actual supported native hosts. No such host was observed here. Windows runtime additionally needs WebView2; Linux runtime additionally needs its graphical environment as described in the existing desktop runbook.
- Before macOS GUI receipt claims: an actual logged-in desktop, operator observation and the existing [marked-fixture manual protocol](../../../../../bindings/tauri-desktop/assurance/RUNBOOK.md). Actual submit/restart/persisted retrieval plus precise owned-store deletion control are pending. A source trace alone cannot certify GUI interaction. No GUI action was attempted.
- Before quiet benchmark evidence: a quiet window after concurrent builds finish and the existing observed quiet-attestation path. No host tuning or process changes were attempted.

The parent will run applicable real profiles serially at the final boundary. No builds, tests, scans, syntax checks, Docker retries, installations, configuration changes, GUI execution, remote actions, KBD mutations, or commits were performed here. This observation is not a PASS receipt for any source or scenario.
