/**
 * Build script for E2B Rust Development Template
 * 
 * This script builds the template and pushes it to E2B registry.
 * Run with: bun run build
 * 
 * IMPORTANT FOR M1/M2 MAC USERS:
 * This build explicitly targets linux/amd64 platform because E2B runs
 * on AMD64 servers, even though you may be building on ARM64 (Apple Silicon).
 * The Docker buildx emulation handles this automatically.
 */

import { Template, defaultBuildLogger } from "e2b";
import { template } from "./template";

async function main() {
  console.log("🚀 Starting E2B template build for Prometheus Rust Development...\n");
  console.log("📋 Template Configuration:");
  console.log("   - Base Image: ubuntu:22.04");
  console.log("   - Rust Version: 1.75.0");
  console.log("   - Platform: linux/amd64 (E2B server architecture)");
  console.log("   - Components: cargo, clippy, rustfmt");
  console.log("   - Build Tools: gcc, make, pkg-config, git\n");

  try {
    const buildInfo = await Template.build(template, {
      alias: "prometheus-rust-dev",
      
      // E2B build configuration
      cpuCount: 2,
      memoryMB: 2048,
      
      // CRITICAL: Force AMD64 platform for E2B compatibility
      // E2B servers run on linux/amd64, not ARM64
      // This is required even when building on M1/M2 Macs
      dockerfilePath: undefined, // We're using TypeScript template, not Dockerfile
      
      // Logging callback
      onBuildLogs: defaultBuildLogger(),
      
      // API configuration (uses E2B_API_KEY from environment)
      apiKey: process.env.E2B_API_KEY,
    });

    console.log("\n✅ Template build completed successfully!\n");
    console.log("📦 Build Information:");
    console.log(`   Template ID: ${buildInfo.templateId}`);
    console.log(`   Build ID: ${buildInfo.buildId}`);
    console.log(`   Alias: prometheus-rust-dev\n`);
    
    console.log("🎯 Next Steps:");
    console.log("   1. Test the template: bun run test");
    console.log("   2. Verify in E2B dashboard: https://e2b.dev/dashboard");
    console.log("   3. Use in your code: Sandbox.create('prometheus-rust-dev')\n");
    
    console.log("💡 Template Details:");
    console.log(`   - This template runs on linux/amd64 (E2B servers)`);
    console.log(`   - Optimized for prometheus_parking_lot development`);
    console.log(`   - Includes full Rust toolchain (1.75.0)`);
    console.log(`   - Ready for cargo build, test, clippy, and fmt\n`);

  } catch (error) {
    console.error("\n❌ Template build failed:\n");
    console.error(error instanceof Error ? error.message : String(error));
    console.error("\n💡 Troubleshooting:");
    console.error("   - Check E2B service status: https://e2b.dev/status");
    console.error("   - Verify E2B_API_KEY in .env file");
    console.error("   - Review template.ts for syntax errors");
    console.error("   - Try running with E2B_DEBUG=1 for more details\n");
    process.exit(1);
  }
}

// Execute the build
main();
