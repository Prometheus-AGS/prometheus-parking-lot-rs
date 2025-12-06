# 🎯 Issue #7: fromBaseImage() vs fromImage() API

## Problem

TypeScript error on line 31 of `template.ts`:

```
Expected 0 arguments, but got 1. (ts 2554)
.fromBaseImage("ubuntu:22.04")
```

## Root Cause

**E2B has TWO different methods for specifying base images:**

### 1. `fromBaseImage()` - NO arguments
```typescript
Template().fromBaseImage()
```
- Uses E2B's **default base image**
- Takes **0 arguments**
- Good for simple use cases

### 2. `fromImage(baseImage: string)` - ONE argument
```typescript
Template().fromImage("ubuntu:22.04")
```
- Uses a **custom Docker image**
- Takes **1 argument** (Docker image name)
- Required for specific base images like Ubuntu 22.04

## The Fix

### Before (❌ Wrong)
```typescript
export const template = Template()
  .fromBaseImage("ubuntu:22.04")  // ❌ fromBaseImage() takes NO args!
```

### After (✅ Correct)
```typescript
export const template = Template()
  .fromImage("ubuntu:22.04")  // ✅ fromImage() takes image name!
```

## API Reference

From `/node_modules/e2b/dist/index.d.ts`:

```typescript
interface TemplateBuilder {
  /**
   * Start from E2B's default base image.
   * NO arguments.
   */
  fromBaseImage(): TemplateBuilder;

  /**
   * Start from a custom Docker image.
   * @param baseImage Docker image name (e.g., "ubuntu:22.04")
   * @param credentials Optional credentials for private registries
   */
  fromImage(
    baseImage: string, 
    credentials?: { username: string; password: string }
  ): TemplateBuilder;

  /**
   * Start from an existing E2B template.
   * @param template E2B template ID or alias
   */
  fromTemplate(template: string): TemplateBuilder;
}
```

## Examples

### Use E2B Default
```typescript
Template()
  .fromBaseImage()  // No arguments
  .runCmd(["apt-get update"])
```

### Use Custom Docker Image
```typescript
Template()
  .fromImage("ubuntu:22.04")  // With image name
  .runCmd(["apt-get update"])
```

### Use Custom Docker Image from Private Registry
```typescript
Template()
  .fromImage("myregistry.com/myimage:latest", {
    username: "user",
    password: "pass"
  })
  .runCmd(["echo Hello"])
```

### Use Existing E2B Template
```typescript
Template()
  .fromTemplate("existing-template-id")
  .runCmd(["echo Building on existing template"])
```

## Why This Matters

This is a **common mistake** when working with E2B v2 API:

| Method | Arguments | Use Case |
|--------|-----------|----------|
| `fromBaseImage()` | **0** | Use E2B's default |
| `fromImage(image)` | **1-2** | Use custom Docker image |
| `fromTemplate(id)` | **1** | Build on existing E2B template |

**The names are confusing!**
- `fromBaseImage()` sounds like it should take an image name
- But it doesn't - it uses E2B's default
- Use `fromImage()` for custom images instead

## Verification

After fix, TypeScript should compile without errors:

```bash
cd .e2b-template
bun run build
# ✅ Should compile successfully
```

## Related Issues

This is **Issue #7** in our sequence:

1. ✅ TemplateBuilder not exported
2. ✅ Package version mismatches
3. ✅ Permission denied (apt-get)
4. ✅ PATH not set (exit 127)
5. ✅ cargo-clippy command name
6. ✅ Runtime environment variables
7. ✅ **fromBaseImage() vs fromImage()** ← YOU ARE HERE

---

**Status:** ✅ FIXED  
**Date:** December 6, 2024  
**Credit:** User's excellent TypeScript error checking!  
**File Modified:** `template.ts` line 31  
**Change:** `.fromBaseImage("ubuntu:22.04")` → `.fromImage("ubuntu:22.04")`
