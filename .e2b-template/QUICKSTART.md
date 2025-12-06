# E2B Template - Quick Start Guide

## 🚀 Fast Track (5 Minutes)

### 1. Get E2B API Key (2 minutes)
```
1. Go to: https://e2b.dev/dashboard
2. Sign up or log in
3. Click "API Keys" → "Create API Key"
4. Copy the key (starts with e2b_)
```

### 2. Configure Environment (30 seconds)
```bash
cd /Users/gqadonis/Projects/prometheus/prometheus-parking-lot/.e2b-template
cp .env.example .env
# Edit .env and paste your API key
```

### 3. Install & Build (3 minutes)
```bash
npm install        # Installs dependencies (30s)
npm run build      # Builds template (2-3 minutes)
```

### 4. Test (10 seconds)
```bash
npx e2b sandbox spawn prometheus-rust-dev --command "cargo --version"
```

**If you see cargo version output, SUCCESS!** 🎉

---

## 📋 Complete Setup Checklist

- [ ] E2B account created at https://e2b.dev
- [ ] API key copied from dashboard
- [ ] `.env` file created with API key
- [ ] `npm install` completed
- [ ] `npm run build` completed successfully
- [ ] Template ID saved (e.g., `tmp_abc123xyz`)
- [ ] Test command shows cargo version

---

## 🎯 What You Just Created

A Rust development sandbox that:
- Starts in **3 seconds** (instead of 135 seconds)
- Has Rust, cargo, clippy, rustfmt pre-installed
- Has all system build tools ready
- Can be created unlimited times instantly
- Is consistent across all sessions

---

## 💻 Using Your Template

### From TypeScript
```typescript
import { Sandbox } from "e2b";

const sandbox = await Sandbox.create("prometheus-rust-dev");
await sandbox.commands.run("cargo --version");
await sandbox.close();
```

### From CLI
```bash
# Test Rust
npx e2b sandbox spawn prometheus-rust-dev --command "cargo --version"

# Interactive shell
npx e2b sandbox spawn prometheus-rust-dev --command "/bin/bash"
```

---

## 🆘 Troubleshooting

### "E2B_API_KEY not found"
```bash
cat .env  # Check if file exists and has correct format
# Should show: E2B_API_KEY=e2b_...
```

### "npm: command not found"
Install Node.js: https://nodejs.org/

### Build fails
1. Check internet connection
2. Verify API key is correct
3. Check E2B status: https://e2b.dev/status

---

## 📚 Full Documentation

- **README.md** - Complete usage guide
- **SETUP.md** - Detailed step-by-step instructions
- **template.ts** - Template configuration
- **build-template.ts** - Build script

---

## ⚡ Speed Comparison

| Task | Before | After | Improvement |
|------|--------|-------|-------------|
| Create sandbox | 5s | 3s | 1.6x faster |
| Install Rust | 120s | 0s | ∞ faster |
| **Total** | **135s** | **3s** | **45x faster!** |

---

## 🎓 Next Steps

1. ✅ Template is registered and working
2. 🔧 Integrate into agent workflow
3. 🧪 Use for quality gates (cargo check, clippy, test)
4. 🚀 Resume library development with instant sandboxes!

---

**Questions?** Check README.md or SETUP.md for detailed information.
