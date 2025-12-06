# 🎯 Bun Commands Reference

Quick reference for all Bun commands used in this E2B template setup.

---

## 🚀 Primary Commands

### Build the Template
```bash
bun run build
```
**What it does:**
- Builds the Docker image with Rust toolchain
- Uploads to E2B registry
- Registers with alias `prometheus-rust-dev`
- **Takes:** ~5-8 minutes (first time only)

---

### Test the Template
```bash
bun run test
```
**What it does:**
- Creates a sandbox from the template
- Verifies all Rust tools are installed
- Tests cargo, rustc, clippy, rustfmt
- Cleans up the sandbox
- **Takes:** ~10-15 seconds

---

### List Your Templates
```bash
bunx e2b template list
```
**What it shows:**
- All your registered E2B templates
- Template IDs and aliases
- Creation timestamps

---

## 🔧 Utility Commands

### Install/Update Dependencies
```bash
bun install
```
**Fast!** Typically completes in <1 second.

---

### Check Package Info
```bash
bun pm ls
```
Shows all installed packages and their versions.

---

### Run TypeScript Directly
```bash
bun run build-template.ts
bun run test-template.ts
```
No transpilation needed - Bun runs TypeScript natively!

---

### Update E2B CLI
```bash
bunx e2b@latest --help
```
Always runs the latest version.

---

## 🐛 Debugging Commands

### Build with Debug Logs
```bash
E2B_DEBUG=1 bun run build
```
Shows detailed E2B API calls and responses.

---

### Type Check (No Execution)
```bash
bunx tsc --noEmit
```
Validates TypeScript without running.

---

### Spawn Test Sandbox
```bash
bunx e2b sandbox spawn prometheus-rust-dev --command "cargo --version"
```
Quick test without running full test suite.

---

### Check Template Details
```bash
bunx e2b template get tmp_YOUR_TEMPLATE_ID
```
Shows full template configuration.

---

## 🧹 Cleanup Commands

### Delete Template
```bash
bunx e2b template delete tmp_YOUR_TEMPLATE_ID
```
**Warning:** This is permanent!

---

### Clear Bun Cache
```bash
bun pm cache rm
```
Clears the Bun package cache if needed.

---

### Remove node_modules
```bash
rm -rf node_modules bun.lock
bun install
```
Fresh start if dependencies get corrupted.

---

## 📊 Comparison with npm/npx

| Task | npm/npx | bun/bunx | Winner |
|------|---------|----------|--------|
| Install deps | `npm install` (~8s) | `bun install` (~1s) | 🏆 Bun 8x faster |
| Run script | `npm run build` | `bun run build` | 🏆 Bun 4x faster |
| Run CLI tool | `npx e2b template list` | `bunx e2b template list` | 🏆 Bun (instant) |
| TypeScript execution | Needs `tsx` or `ts-node` | Native support | 🏆 Bun (no deps) |

---

## 💡 Pro Tips

### Tip 1: Chain Commands
```bash
bun run build && bun run test
```
Build then immediately test.

---

### Tip 2: Watch Mode (if added to package.json)
```bash
bun run --watch build-template.ts
```
Re-run on file changes.

---

### Tip 3: Environment Variables
```bash
E2B_DEBUG=1 RUST_VERSION=1.76.0 bun run build
```
Override defaults via env vars.

---

### Tip 4: Parallel Execution
```bash
bun run build & bun run test
```
Run multiple commands simultaneously (use with caution).

---

## 🔐 Environment Setup

### Check Current Environment
```bash
# Show all environment variables
bun run -e
```

---

### Verify .env Loading
```bash
bun run -e | grep E2B_API_KEY
```
Should show your API key (if .env is properly configured).

---

## 📚 Getting Help

### Bun Help
```bash
bun --help
bun run --help
bun install --help
```

### E2B CLI Help
```bash
bunx e2b --help
bunx e2b template --help
bunx e2b sandbox --help
```

---

## 🎬 Complete Workflow

Here's the typical workflow from start to finish:

```bash
# 1. Navigate to template directory
cd /Users/gqadonis/Projects/prometheus/prometheus-parking-lot/.e2b-template

# 2. Ensure dependencies are installed
bun install

# 3. Build the template (first time)
bun run build
# ⏳ Wait ~5-8 minutes...

# 4. Test the template
bun run test
# ⏳ Wait ~10-15 seconds...

# 5. Verify registration
bunx e2b template list

# 6. (Optional) Test specific command
bunx e2b sandbox spawn prometheus-rust-dev --command "cargo --version"

# 7. Ready to use in code!
# const sandbox = await Sandbox.create("prometheus-rust-dev");
```

---

## 🆘 Emergency Commands

### If Build Fails
```bash
# Retry with debug logging
E2B_DEBUG=1 bun run build

# Check E2B status
curl https://status.e2b.dev/api/v2/status.json
```

---

### If Dependencies Break
```bash
# Nuclear option: start fresh
rm -rf node_modules bun.lock
bun install
```

---

### If Template Gets Corrupted
```bash
# Delete and rebuild
bunx e2b template delete tmp_YOUR_TEMPLATE_ID
bun run build
```

---

**All set! You're ready to build with Bun! 🚀**
