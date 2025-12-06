/**
 * Prometheus Rust Development Template for E2B
 * 
 * This template provides a pre-configured environment with:
 * - Rust 1.90.0 (stable) toolchain with latest features
 * - Cargo build system with workspace publishing support
 * - Clippy linter with enhanced diagnostics
 * - Rustfmt formatter
 * - LLD linker for significantly faster compile times
 * - Essential build tools (gcc, make, pkg-config)
 * - Git for version control
 * 
 * Rust 1.90.0 Key Features (Released September 18, 2024):
 * - LLD as default linker on x86_64-unknown-linux-gnu (faster compile times)
 * - Native Cargo workspace publishing (cargo publish --workspace)
 * - Stabilized APIs for safer systems programming
 * - Improved const fn capabilities
 * - Enhanced slice::reverse() performance (now const)
 * - Better diagnostics and error messages
 * 
 * The template is optimized for building and testing the prometheus_parking_lot library.
 * 
 * IMPORTANT: This template is built for linux/amd64 (E2B server architecture)
 * even when building on ARM64 (M1/M2 Mac). The platform is specified in build-template.ts.
 */

import { Template } from "e2b";

// Create the template configuration
export const template = Template()
  .fromImage("ubuntu:22.04")  // Use fromImage() for custom Docker images
  .setUser("root")  // Explicitly set user to root for system operations
  .setEnvs({
    // Rust environment variables
    RUSTUP_HOME: "/usr/local/rustup",
    CARGO_HOME: "/usr/local/cargo",
    PATH: "/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    
    // Cargo configuration
    CARGO_NET_GIT_FETCH_WITH_CLI: "true",
    RUST_BACKTRACE: "1",
    
    // Debian frontend (prevents interactive prompts)
    DEBIAN_FRONTEND: "noninteractive",
  })
  .runCmd([
    // Install system dependencies
    "apt-get update && apt-get install -y build-essential curl git pkg-config libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*",
  ])
  .runCmd([
    // Install Rust 1.90.0 toolchain
    "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.90.0 --profile default --no-modify-path",
  ])
  .runCmd([
    // Install Rust components (clippy, rustfmt)
    "/usr/local/cargo/bin/rustup component add rustfmt clippy",
  ])
  .runCmd([
    // Configure Cargo for parallel builds
    "mkdir -p /root/.cargo && echo '[build]' > /root/.cargo/config.toml && echo 'jobs = 4' >> /root/.cargo/config.toml",
  ])
  .runCmd([
    // Verify all tools are installed and working
    "/usr/local/cargo/bin/cargo --version && /usr/local/cargo/bin/rustc --version && /usr/local/cargo/bin/rustfmt --version && /usr/local/cargo/bin/cargo clippy --version",
  ]);

export default template;
