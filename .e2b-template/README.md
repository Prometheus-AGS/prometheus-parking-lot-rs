# E2B Template for Prometheus Parking Lot

This directory contains the E2B template configuration for creating instant Rust development sandboxes optimized for the `prometheus_parking_lot` library.

## What is This?

This template pre-builds a complete Rust development environment that can be instantiated in ~3 seconds instead of waiting 2-5 minutes for Rust installation on every new sandbox.

## Quick Start

### Prerequisites

- Node.js 18+ installed
- E2B account (sign up at https://e2b.dev)
- E2B API key

### Step 1: Configure API Key

```bash
# Copy the example environment file
cp .env.example .env

# Edit .env and add your E2B API key
# Get your key from: https://e2b.dev/dashboard
```

Your `.env` file should look like:
```
E2B_API_KEY=e2b_your_actual_key_here
```

### Step 2: Install Dependencies

```bash
npm install
```

### Step 3: Build the Template

```bash
npm run build
```

This will:
- Upload the template to E2B
- Install all system dependencies
- Install Rust toolchain (stable)
- Configure the environment
- Register the template with alias `prometheus-rust-dev`

**⏳ First build takes ~5 minutes**

### Step 4: Get Your Template ID

After successful build, you'll see output like:

```
✅ Template built successfully!

📋 Template Details:
   Template ID: tmp_abc123xyz456
   Build ID: bld_def789uvw012
   Alias: prometheus-rust-dev
```

**Save the Template ID!** You'll need it to create sandboxes.

## Usage

### From TypeScript/JavaScript

```typescript
import { Sandbox } from "e2b";

// Create sandbox using alias
const sandbox = await Sandbox.create("prometheus-rust-dev", {
  timeout: 300000 // 5 minutes
});

// Rust is ready immediately!
const result = await sandbox.commands.run("cargo --version");
console.log(result.stdout); // cargo 1.75.0

// Use the sandbox
await sandbox.filesystem.write("/workspace/Cargo.toml", cargoToml);
await sandbox.filesystem.write("/workspace/src/lib.rs", code);
await sandbox.commands.run("cd /workspace && cargo check");

// Clean up
await sandbox.close();
```

### From CLI

```bash
# Test the template
npx e2b sandbox spawn prometheus-rust-dev --command "cargo --version"

# Interactive shell
npx e2b sandbox spawn prometheus-rust-dev --command "/bin/bash"

# List your templates
npx e2b template list
```

## What's Included

### System Packages
- `curl` - Download utilities
- `git` - Version control
- `build-essential` - GCC, G++, Make
- `pkg-config` - Library configuration
- `libssl-dev` - OpenSSL development headers

### Rust Toolchain
- **Rust stable** (latest version)
- **cargo** - Package manager and build tool
- **clippy** - Linter
- **rustfmt** - Code formatter

### Environment
- Working directory: `/workspace`
- Cargo home: `/root/.cargo`
- Rustup home: `/root/.rustup`
- `RUST_BACKTRACE=1` for better error messages
- 2 CPU cores, 2GB RAM

## Template Management

### Update Template

Make changes to `template.ts`, then rebuild:

```bash
npm run build
```

This creates a new version with the same alias.

### List Templates

```bash
npx e2b template list
```

### Delete Template

```bash
npx e2b template delete tmp_your_template_id
```

## Integration with Agent Workflow

This template is designed to integrate with our 5-phase development workflow:

```typescript
// Initialize once per session
const sandbox = await Sandbox.create("prometheus-rust-dev", {
  session_id: "prometheus-parking-lot-dev"
});

// Write code to sandbox
await sandbox.filesystem.write("/workspace/src/mutex.rs", code);

// Run quality gates instantly
await sandbox.commands.run("cd /workspace && cargo check");
await sandbox.commands.run("cd /workspace && cargo clippy -- -D warnings");
await sandbox.commands.run("cd /workspace && cargo test");
```

## Performance Comparison

### Before (Runtime Installation)
```
Create sandbox: 5s
Install Rust: 120s ⏳
Run cargo check: 10s
TOTAL: 135s per session
```

### After (Template-Based)
```
Create sandbox from template: 3s ⚡
Run cargo check: 10s
TOTAL: 13s per session
```

**10x faster!** ⚡

## Troubleshooting

### "E2B_API_KEY not found"

Make sure you've created the `.env` file:
```bash
cp .env.example .env
# Edit .env with your API key
```

### "npm: command not found"

Install Node.js:
```bash
# macOS
brew install node

# Or download from: https://nodejs.org/
```

### Template build fails

1. Check your API key is valid
2. Verify internet connection
3. Check E2B status: https://e2b.dev/status
4. Try with debug mode:
   ```bash
   E2B_DEBUG=1 npm run build
   ```

### Can't find template after building

Use the Template ID directly:
```typescript
const sandbox = await Sandbox.create("tmp_your_template_id");
```

Or check what templates you have:
```bash
npx e2b template list
```

## Files in This Directory

```
.e2b-template/
├── README.md              # This file
├── SETUP.md              # Detailed setup instructions
├── template.ts           # Template definition
├── build-template.ts     # Build script
├── package.json          # Node.js dependencies
├── tsconfig.json         # TypeScript configuration
├── .env.example          # Environment variable template
└── .env                  # Your API key (not committed)
```

## Resources

- **E2B Documentation**: https://e2b.dev/docs
- **Template Quickstart**: https://e2b.dev/docs/template/quickstart
- **E2B Dashboard**: https://e2b.dev/dashboard
- **E2B Status**: https://e2b.dev/status

## License

Same as parent project (MIT OR Apache-2.0)
