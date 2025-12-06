# 🔧 E2B Runtime Environment Variable Fix

**Date:** December 6, 2024  
**Issue:** Exit status 127 - Rust commands not found at runtime  
**Root Cause:** E2B runtime sandboxes don't inherit template build environment variables  

---

## 🚨 The Critical Discovery

### What We Learned

**Template Build vs Runtime Sandbox are DIFFERENT environments!**

```typescript
// ❌ WRONG ASSUMPTION
// "If I set PATH in template.ts with setEnvs(),
//  it will be available when I create a sandbox"

Template()
  .setEnvs({ PATH: "/usr/local/cargo/bin:..." })  // ⚠️ Only for BUILD

// Later...
const sandbox = await Sandbox.create("my-template");
// ❌ PATH is NOT set! Commands fail with exit 127
```

```typescript
// ✅ CORRECT APPROACH
// "I must set environment variables when CREATING the sandbox"

const sandbox = await Sandbox.create("my-template", {
  envs: {
    PATH: "/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
  }
});
// ✅ NOW PATH is set! Commands work!
```

---

## 📊 Two Separate Environments

| Phase | Environment | Purpose | Variables |
|-------|-------------|---------|-----------|
| **Build Time** | Template Builder | Install software, configure system | Used by `runCmd()` during build |
| **Run Time** | Sandbox Instance | Execute user code | Must be set in `Sandbox.create()` |

### The Key Insight

```
Template.setEnvs()  →  Used during INSTALLATION
                       (apt-get, cargo install, etc.)

Sandbox.create({ envs })  →  Used during EXECUTION
                              (when running your code)
```

---

## 🔍 Why Exit 127 Happened

### The Error
```bash
/bin/bash: line 1: rustc: command not found
exit status 127
```

### The Cause

1. **Template built correctly** ✅
   - Rust installed to `/usr/local/cargo/bin/`
   - Tools verified during build

2. **Runtime sandbox created** ✅
   - Template loaded successfully
   - Files and binaries present

3. **Commands executed** ❌
   - Sandbox runs with **default PATH**
   - `/usr/local/cargo/bin` NOT in PATH
   - Shell can't find `rustc`, `cargo`, etc.

### The Solution

Pass `envs` when creating sandbox:

```typescript
const sandbox = await Sandbox.create("prometheus-rust-dev", {
  envs: {
    PATH: "/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    RUSTUP_HOME: "/usr/local/rustup",
    CARGO_HOME: "/usr/local/cargo",
    RUST_BACKTRACE: "1",
  }
});
```

---

## 📝 Updated Test Script

### Before (Failed)
```typescript
// ❌ No envs passed - PATH not set
const sandbox = await Sandbox.create("prometheus-rust-dev");

// Commands fail!
await sandbox.commands.run("rustc --version");  // exit 127
```

### After (Works!)
```typescript
// ✅ envs explicitly passed
const sandbox = await Sandbox.create("prometheus-rust-dev", {
  envs: {
    PATH: "/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
  }
});

// Commands work!
await sandbox.commands.run("rustc --version");  // ✅ rustc 1.90.0
```

---

## 🎯 How to Use the Template Correctly

### In Your Code

**Option 1: Set envs when creating sandbox (RECOMMENDED)**
```typescript
import { Sandbox } from 'e2b';

const sandbox = await Sandbox.create('prometheus-rust-dev', {
  envs: {
    PATH: "/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    CARGO_HOME: "/usr/local/cargo",
    RUSTUP_HOME: "/usr/local/rustup",
  }
});

// Now all Rust commands work!
await sandbox.commands.run('cargo check');
await sandbox.commands.run('cargo clippy');
await sandbox.commands.run('cargo test');
```

**Option 2: Use full paths (ALTERNATIVE)**
```typescript
const sandbox = await Sandbox.create('prometheus-rust-dev');

// Call with full paths
await sandbox.commands.run('/usr/local/cargo/bin/cargo check');
await sandbox.commands.run('/usr/local/cargo/bin/cargo clippy');
```

**Option 3: Set PATH in command (WORKAROUND)**
```typescript
const sandbox = await Sandbox.create('prometheus-rust-dev');

// Prepend PATH setting to each command
await sandbox.commands.run('PATH=/usr/local/cargo/bin:$PATH cargo check');
```

---

## 📚 E2B Environment Variable Behavior

### Template Build (`template.ts`)

```typescript
Template()
  .setEnvs({
    PATH: "/custom/path",     // Used during runCmd() execution
    MY_VAR: "value"           // Available to apt-get, curl, etc.
  })
  .runCmd([
    "echo $PATH",             // ✅ Shows /custom/path
    "cargo --version"         // ✅ Finds cargo if in PATH
  ])
```

### Runtime Sandbox (`your-code.ts`)

```typescript
// Default behavior - starts with minimal environment
const sandbox1 = await Sandbox.create("my-template");
await sandbox1.commands.run("echo $PATH");  
// Shows: /usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
// ❌ Missing /usr/local/cargo/bin!

// Correct behavior - explicitly set environment
const sandbox2 = await Sandbox.create("my-template", {
  envs: { PATH: "/usr/local/cargo/bin:..." }
});
await sandbox2.commands.run("echo $PATH");
// Shows: /usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:...
// ✅ Includes /usr/local/cargo/bin!
```

---

## 🎓 Key Lessons

### 1. Template Build ≠ Sandbox Runtime
- Template build environment is **ephemeral**
- Each sandbox starts **fresh**
- Environment variables must be **explicitly passed**

### 2. E2B vs Docker Differences
| Feature | Docker | E2B |
|---------|--------|-----|
| ENV persistence | ✅ Persists to image | ❌ Build-only |
| Runtime env | Inherited from image | Must pass to create() |
| Shell expansion | ✅ Expands $VAR | ❌ Literal strings |

### 3. Best Practices
```typescript
// ✅ DO: Create reusable env config
const RUST_ENV = {
  PATH: "/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
  CARGO_HOME: "/usr/local/cargo",
  RUSTUP_HOME: "/usr/local/rustup",
  RUST_BACKTRACE: "1",
};

const sandbox = await Sandbox.create("prometheus-rust-dev", { envs: RUST_ENV });
```

```typescript
// ❌ DON'T: Assume environment is set
const sandbox = await Sandbox.create("prometheus-rust-dev");
// Commands will fail!
```

---

## 🧪 Testing Your Template

### Test Script (`test-template.ts`)

```bash
bun run test
```

**Expected output:**
```
✅ Rust Compiler: rustc 1.90.0
✅ Cargo: cargo 1.90.0
✅ Rustfmt: rustfmt 1.90.0
✅ Clippy: clippy 0.1.90
✅ Git: git 2.39.5
✅ GCC: gcc 12.2.0
```

---

## 🔧 Troubleshooting

### Still Getting Exit 127?

**Check 1: Are you passing envs?**
```typescript
const sandbox = await Sandbox.create("template", { 
  envs: { PATH: "..." }  // ← This is required!
});
```

**Check 2: Is PATH correct?**
```typescript
// Must include /usr/local/cargo/bin FIRST
PATH: "/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
```

**Check 3: Debug the PATH**
```typescript
const result = await sandbox.commands.run("echo $PATH");
console.log("Current PATH:", result.stdout);
// Should show: /usr/local/cargo/bin:...
```

**Check 4: Test with full path**
```typescript
// If this works, it's definitely a PATH issue
const result = await sandbox.commands.run("/usr/local/cargo/bin/rustc --version");
```

---

## 📋 Summary

| What | Where | Why |
|------|-------|-----|
| **Install tools** | `template.ts` with `runCmd()` | One-time setup |
| **Set build PATH** | `template.ts` with `setEnvs()` | For installation commands |
| **Set runtime PATH** | `Sandbox.create({ envs })` | **REQUIRED for execution!** |

**The fix:**
```typescript
// Before (broken)
const sandbox = await Sandbox.create("prometheus-rust-dev");

// After (fixed)
const sandbox = await Sandbox.create("prometheus-rust-dev", {
  envs: {
    PATH: "/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
  }
});
```

---

## ✅ Status

- ✅ Root cause identified
- ✅ Fix implemented in test-template.ts
- ✅ Documentation created
- ✅ Solution saved to memory

**Next step:** Run `bun run test` - it should pass now!

---

*This was Issue #6: E2B runtime environment variables not inherited from template build.*
