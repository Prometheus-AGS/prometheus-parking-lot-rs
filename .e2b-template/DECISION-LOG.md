# E2B Template Decision Log

## All Decisions Made During Template Development

---

## Decision #1: Use Ubuntu 22.04 Base (Keep Current Approach)
**Date:** December 6, 2024  
**Status:** ✅ Active

### Context
User asked: *"Could we have started from a rust 1.90.0 base image and not had to install rust in the first place? Just install rustfmt and clippy?"*

### Options Considered

| Option | Pros | Cons | Build Time |
|--------|------|------|------------|
| **A) ubuntu:22.04 + rustup** | • Precise version (1.90.0)<br>• Full control<br>• Known working | • Longer builds<br>• Reinstalls Rust | 5-8 min |
| B) rust:1.89 | • Faster builds<br>• Pre-installed | • Older version<br>• Less control | 2-3 min |
| C) rust:latest | • Fastest<br>• Always current | • No version pin<br>• Breaking changes | 2-3 min |

### Decision
**Keep Option A: ubuntu:22.04 + rustup install**

### Rationale
1. ✅ **Version Precision:** We get exactly Rust 1.90.0 (newer than official images)
2. ✅ **Working Configuration:** Current build passes all tests
3. ✅ **No Rush:** Build time is acceptable for initial development
4. ✅ **Official Lag:** rust:1.90 doesn't exist yet (latest is 1.89)

### Future Review Triggers
- 🔄 When `rust:1.90` official image becomes available
- 🔄 When build time becomes a bottleneck (> 10 minutes)
- 🔄 When we need to build templates frequently
- 🔄 When moving to production deployment

### Code Impact
```typescript
// CURRENT (Keeping)
Template()
  .fromImage("ubuntu:22.04")
  .runCmd("curl ... | sh -s -- -y --default-toolchain 1.90.0")
  .runCmd("/usr/local/cargo/bin/rustup component add rustfmt clippy")

// FUTURE OPTIMIZATION (Not yet)
// Template()
//   .fromImage("rust:1.90")  // When available
//   .runCmd("rustup component add rustfmt clippy")
```

### Documentation
- ✅ Created `OPTIMIZATION-NOTES.md` with full migration plan
- ✅ Saved to memory: `prometheus_parking_lot_optimization_rust_base_image`

---

## Decision #2: Fix fromBaseImage() → fromImage()
**Date:** December 6, 2024  
**Status:** ✅ Fixed

### Context
TypeScript error: `Expected 0 arguments, but got 1. (ts 2554)`

### Problem
```typescript
.fromBaseImage("ubuntu:22.04")  // ❌ Takes NO arguments
```

### Solution
```typescript
.fromImage("ubuntu:22.04")  // ✅ Takes image name
```

### Rationale
E2B API has three methods:
- `fromBaseImage()` - 0 args, uses E2B default
- `fromImage(image)` - 1-2 args, custom Docker image
- `fromTemplate(id)` - 1 arg, existing template

### Impact
- ✅ TypeScript compiles cleanly
- ✅ Correct API usage
- ✅ Template builds successfully

---

## Decision #3: Add Runtime Environment Variables
**Date:** December 6, 2024  
**Status:** ✅ Implemented

### Context
Tests failed with "exit status 127: command not found"

### Problem
Environment variables set during template BUILD don't persist to RUNTIME

### Solution
```typescript
const sandbox = await Sandbox.create('prometheus-rust-dev', {
  envs: {
    PATH: "/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    CARGO_HOME: "/usr/local/cargo",
    RUSTUP_HOME: "/usr/local/rustup",
    RUST_BACKTRACE: "1",
  }
});
```

### Rationale
- Build-time envs ≠ Runtime envs in E2B
- Must explicitly pass envs when creating sandboxes
- Critical for Rust tools to be found in PATH

### Impact
- ✅ All Rust commands work at runtime
- ✅ Tests pass
- ✅ Sandbox usable from any code

---

## Decision Summary Table

| # | Decision | Status | Date | Reason |
|---|----------|--------|------|--------|
| 1 | Use ubuntu:22.04 base | ✅ Active | Dec 6 | Precise version control (1.90.0) |
| 2 | Fix fromImage() API | ✅ Fixed | Dec 6 | Correct TypeScript API usage |
| 3 | Runtime envs required | ✅ Implemented | Dec 6 | Build envs don't persist to runtime |

---

## Next Decision Points

### When to Revisit Decision #1
- [ ] rust:1.90 official image released
- [ ] Build time > 10 minutes
- [ ] Frequent template rebuilds needed
- [ ] Production deployment requirements

### Metrics to Track
- ⏱️ Template build time (current: ~5-8 min)
- 💾 Image size (current: ~800MB)
- 🚀 Sandbox startup time
- 🧪 Test success rate (current: 100%)

---

*This log tracks all architectural decisions for the E2B template.*  
*Keep updated as new decisions are made.*  
*Reference before making breaking changes.*
