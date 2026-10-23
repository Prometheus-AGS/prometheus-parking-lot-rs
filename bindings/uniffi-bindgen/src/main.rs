//! `uniffi-bindgen`: forwards to UniFFI's command line.
//!
//! Usage (see `bindings/generate.sh`):
//! `uniffi-bindgen generate --library <cdylib> --language kotlin --out-dir <dir>`.

fn main() {
    uniffi::uniffi_bindgen_main();
}
