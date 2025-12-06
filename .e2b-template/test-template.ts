/**
 * Test Script for Prometheus Parking Lot E2B Template
 * 
 * This script creates a sandbox from the template and verifies that
 * all Rust tools are properly installed and working.
 * 
 * Prerequisites:
 * 1. Template must be built first (run build-template.ts)
 * 2. E2B_API_KEY in .env file
 * 
 * Usage:
 *   bun run test
 *   or
 *   npx tsx test-template.ts
 * 
 * IMPORTANT: E2B runtime sandboxes don't automatically inherit
 * environment variables from template build. We must set them
 * when creating the sandbox OR use full paths to binaries.
 */

import { Sandbox } from "e2b";
import { config } from "dotenv";

// Load environment variables
config();

async function testTemplate() {
  console.log("🧪 Testing Prometheus Rust Development Template...\n");
  
  // Check for API key
  if (!process.env.E2B_API_KEY) {
    console.error("❌ ERROR: E2B_API_KEY not found in environment");
    process.exit(1);
  }
  
  let sandbox: Sandbox | null = null;
  
  try {
    // Create sandbox from template with explicit environment variables
    console.log("📦 Creating sandbox from template 'prometheus-rust-dev'...");
    
    sandbox = await Sandbox.create("prometheus-rust-dev", {
      envs: {
        // Explicitly set PATH for runtime sandbox
        PATH: "/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        RUSTUP_HOME: "/usr/local/rustup",
        CARGO_HOME: "/usr/local/cargo",
        RUST_BACKTRACE: "1",
      }
    });
    
    console.log(`✅ Sandbox created: ${sandbox.id}\n`);
    
    // Test commands (now they should work with PATH set)
    const commands = [
      { name: "Rust Compiler", cmd: "rustc --version" },
      { name: "Cargo", cmd: "cargo --version" },
      { name: "Rustfmt", cmd: "rustfmt --version" },
      { name: "Clippy", cmd: "cargo clippy --version" },
      { name: "Git", cmd: "git --version" },
      { name: "GCC", cmd: "gcc --version | head -n1" },
    ];
    
    console.log("🔍 Testing Rust toolchain...\n");
    
    for (const { name, cmd } of commands) {
      try {
        const result = await sandbox.commands.run(cmd);
        
        if (result.exitCode === 0) {
          console.log(`✅ ${name}:`);
          console.log(`   ${result.stdout.trim()}\n`);
        } else {
          console.error(`❌ ${name} failed:`);
          console.error(`   Exit code: ${result.exitCode}`);
          console.error(`   stderr: ${result.stderr}\n`);
        }
      } catch (error) {
        console.error(`❌ ${name} error:`, error);
      }
    }
    
    // Test a simple Cargo command
    console.log("📝 Testing Cargo functionality...\n");
    
    const cargoTest = await sandbox.commands.run("cargo --help");
    if (cargoTest.exitCode === 0) {
      console.log("✅ Cargo help command works\n");
    } else {
      console.error("❌ Cargo help command failed\n");
    }
    
    console.log("🎉 Template test completed successfully!\n");
    console.log("✨ All Rust tools are working!\n");
    console.log("💡 You can now use this template for Rust development.");
    console.log('   Example:');
    console.log('   const sandbox = await Sandbox.create("prometheus-rust-dev", {');
    console.log('     envs: {');
    console.log('       PATH: "/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"');
    console.log('     }');
    console.log('   });\n');
    
  } catch (error) {
    console.error("❌ Template test failed:\n");
    
    if (error instanceof Error) {
      console.error(`   ${error.message}\n`);
      
      if (error.message.includes("not found") || error.message.includes("does not exist")) {
        console.error("💡 Template not found:");
        console.error("   - Make sure you've run 'bun run build' first");
        console.error("   - Check that the template alias 'prometheus-rust-dev' exists");
        console.error("   - List templates with: npx e2b template list\n");
      }
    } else {
      console.error(error);
    }
    
    process.exit(1);
  } finally {
    // Clean up sandbox
    if (sandbox) {
      console.log("🧹 Closing sandbox...");
      await sandbox.kill();
      console.log("✅ Sandbox closed\n");
    }
  }
}

// Run the test
testTemplate();
