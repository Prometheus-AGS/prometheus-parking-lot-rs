# E2B Template Setup Guide

Complete step-by-step instructions for registering the Prometheus Parking Lot E2B template.

---

## Overview

This guide will walk you through:
1. Creating an E2B account
2. Getting your API key
3. Installing dependencies
4. Building the template
5. Testing the template
6. Using the template in development

**Time required:** ~15 minutes (including 5-minute build)

---

## Step 1: Create E2B Account

### 1.1 Visit E2B Website

Go to: **https://e2b.dev**

### 1.2 Sign Up

Click "Sign Up" or "Get Started"

Options:
- Sign up with GitHub (recommended)
- Sign up with Google
- Sign up with email

### 1.3 Verify Account

Check your email for verification link (if using email signup)

### 1.4 Access Dashboard

After signup, you should be redirected to: **https://e2b.dev/dashboard**

---

## Step 2: Get API Key

### 2.1 Navigate to API Keys

In the E2B Dashboard sidebar, find and click **"API Keys"**

### 2.2 Create New Key

Click the **"Create API Key"** button

### 2.3 Name Your Key

Give it a descriptive name: `prometheus-parking-lot`

### 2.4 Copy the Key

**⚠️ IMPORTANT:** The API key is shown only ONCE!

- Copy the entire key (starts with `e2b_`)
- Save it somewhere safe temporarily
- Example format: `e2b_abc123def456ghi789jkl012mno345`

---

## Step 3: Configure Environment

### 3.1 Open Terminal

Open your terminal application

### 3.2 Navigate to Template Directory

```bash
cd /Users/gqadonis/Projects/prometheus/prometheus-parking-lot/.e2b-template
```

### 3.3 Create .env File

```bash
cp .env.example .env
```

### 3.4 Edit .env File

Open `.env` in your editor:

```bash
# Using nano
nano .env

# Or using vim
vim .env

# Or using VS Code
code .env
```

### 3.5 Add Your API Key

Replace `your_api_key_here` with your actual E2B API key:

```
E2B_API_KEY=e2b_abc123def456ghi789jkl012mno345
```

**Important:**
- No spaces around the `=`
- No quotes around the key
- Key must start with `e2b_`

### 3.6 Save and Close

- nano: `Ctrl+X`, then `Y`, then `Enter`
- vim: `Esc`, then `:wq`, then `Enter`
- VS Code: `Cmd+S` (Mac) or `Ctrl+S` (Windows/Linux)

### 3.7 Verify .env File

```bash
cat .env
```

Should show:
```
E2B_API_KEY=e2b_your_actual_key
```

---

## Step 4: Install Dependencies

### 4.1 Check Node.js Version

```bash
node --version
```

Should show `v18.0.0` or higher. If not installed:

**macOS:**
```bash
brew install node
```

**Linux:**
```bash
curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
sudo apt-get install -y nodejs
```

**Windows:**
Download from https://nodejs.org/

### 4.2 Install npm Packages

```bash
npm install
```

**Expected output:**
```
added 45 packages, and audited 46 packages in 3s
found 0 vulnerabilities
```

### 4.3 Verify Installation

```bash
ls node_modules
```

Should show directories including:
- `e2b/`
- `dotenv/`
- `tsx/`
- `typescript/`

---

## Step 5: Build the Template

### 5.1 Run Build Command

```bash
npm run build
```

**⏳ This takes ~5 minutes on first build**

### 5.2 Watch the Output

You'll see progress messages:

```
🚀 Building Prometheus Rust Development Template...

📦 Building template...
⏳ This may take 3-5 minutes on first build...

⠋ Installing system packages...
⠋ Installing Rust toolchain...
⠋ Configuring environment...
```

### 5.3 Success Output

When complete, you'll see:

```
✅ Template built successfully!

📋 Template Details:
   Template ID: tmp_abc123xyz456
   Build ID: bld_def789uvw012
   Alias: prometheus-rust-dev

🎉 You can now create sandboxes with:
   const sandbox = await Sandbox.create("prometheus-rust-dev")

💡 Or test from CLI:
   npx e2b sandbox spawn prometheus-rust-dev --command "cargo --version"

📝 Save your Template ID: tmp_abc123xyz456
   You'll need this to create sandboxes!
```

### 5.4 Save Your Template ID

**IMPORTANT:** Copy and save the Template ID!

Example: `tmp_abc123xyz456`

You'll need this to create sandboxes.

---

## Step 6: Verify Template

### 6.1 List Your Templates

```bash
npx e2b template list
```

**Expected output:**
```
┌─────────────────────┬──────────────────────┬─────────────────────┐
│ Template ID         │ Alias                │ Created             │
├─────────────────────┼──────────────────────┼─────────────────────┤
│ tmp_abc123xyz456    │ prometheus-rust-dev  │ 2024-01-15 10:30:00 │
└─────────────────────┴──────────────────────┴─────────────────────┘
```

You should see:
- Your Template ID
- Alias: `prometheus-rust-dev`
- Creation timestamp

---

## Step 7: Test the Template

### 7.1 Test Rust Installation

```bash
npx e2b sandbox spawn prometheus-rust-dev --command "cargo --version"
```

**Expected output:**
```
Creating sandbox from template 'prometheus-rust-dev'...
Sandbox created: sbx_abc123
Running command: cargo --version

cargo 1.75.0 (1d8b05cdd 2024-01-10)

Sandbox closed.
```

✅ If you see the cargo version, SUCCESS!

### 7.2 Test Full Toolchain

```bash
npx e2b sandbox spawn prometheus-rust-dev --command "rustc --version && cargo --version && rustfmt --version && cargo-clippy --version"
```

**Expected output:**
```
rustc 1.75.0 (82e1608df 2024-01-12)
cargo 1.75.0 (1d8b05cdd 2024-01-10)
rustfmt 1.7.0-stable (82e1608df 2024-01-12)
clippy 0.1.75 (82e1608df 2024-01-12)
```

✅ All tools should report their versions!

### 7.3 Test Compilation

```bash
npx e2b sandbox spawn prometheus-rust-dev --command "cd /workspace && cargo init test-project && cd test-project && cargo check"
```

**Expected output:**
```
Created binary (application) `test-project` package
    Checking test-project v0.1.0 (/workspace/test-project)
    Finished dev [unoptimized + debuginfo] target(s) in 0.45s
```

✅ Cargo should successfully check a new project!

---

## Step 8: Integration with Development

### 8.1 Using in TypeScript/JavaScript

Create a test file `test-sandbox.js`:

```javascript
import { Sandbox } from "e2b";

async function test() {
  console.log("Creating sandbox...");
  
  const sandbox = await Sandbox.create("prometheus-rust-dev");
  
  console.log("Testing Rust...");
  const result = await sandbox.commands.run("cargo --version");
  console.log(result.stdout);
  
  console.log("Closing sandbox...");
  await sandbox.close();
  
  console.log("✅ Test complete!");
}

test().catch(console.error);
```

Run it:
```bash
node test-sandbox.js
```

### 8.2 Using in Agent Workflow

The template is now ready to use in our 5-phase development workflow!

Sandboxes will be created in ~3 seconds instead of ~135 seconds.

---

## Troubleshooting

### Error: "E2B_API_KEY not found"

**Problem:** `.env` file is missing or incorrect

**Solution:**
```bash
# Verify .env exists
ls -la .env

# Check contents
cat .env

# Should show:
# E2B_API_KEY=e2b_...

# If wrong, edit it:
nano .env
```

---

### Error: "Authentication failed"

**Problem:** API key is invalid or expired

**Solution:**
1. Go to https://e2b.dev/dashboard
2. Navigate to API Keys
3. Delete old key
4. Create new key
5. Update `.env` with new key
6. Try build again

---

### Error: "npm: command not found"

**Problem:** Node.js is not installed

**Solution:**

**macOS:**
```bash
brew install node
```

**Linux:**
```bash
curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
sudo apt-get install -y nodejs
```

**Windows:**
Download from https://nodejs.org/

---

### Error: "Build failed"

**Problem:** Network issue or E2B service problem

**Solution:**
1. Check internet connection
2. Check E2B status: https://e2b.dev/status
3. Wait a few minutes and retry
4. Try with debug mode:
   ```bash
   E2B_DEBUG=1 npm run build
   ```

---

### Error: "Template not found"

**Problem:** Template wasn't registered or wrong alias

**Solution:**
1. Verify template exists:
   ```bash
   npx e2b template list
   ```
2. Use Template ID directly:
   ```bash
   npx e2b sandbox spawn tmp_your_template_id --command "cargo --version"
   ```

---

## Verification Checklist

Go through this checklist to confirm everything is working:

- [ ] E2B account created
- [ ] API key obtained
- [ ] `.env` file created with API key
- [ ] Dependencies installed (`npm install`)
- [ ] Template built successfully (`npm run build`)
- [ ] Template ID saved
- [ ] Template appears in `npx e2b template list`
- [ ] Test command shows cargo version
- [ ] All Rust tools (rustc, cargo, clippy, rustfmt) available
- [ ] Test compilation works

If all items are checked, you're ready to go! ✅

---

## Next Steps

1. **Update Agent Workflow** - Integrate template into execution loop
2. **Test Quality Gates** - Verify cargo check/clippy/test work
3. **Resume Development** - Continue with library implementation
4. **Enjoy Speed** - No more waiting for Rust installation! ⚡

---

## Quick Reference

```bash
# Navigate to template directory
cd /Users/gqadonis/Projects/prometheus/prometheus-parking-lot/.e2b-template

# Setup
cp .env.example .env          # Create .env
nano .env                     # Add API key
npm install                   # Install dependencies
npm run build                 # Build template (5 min)

# Verification
npx e2b template list         # List templates
npx e2b sandbox spawn prometheus-rust-dev --command "cargo --version"  # Test

# Usage
const sandbox = await Sandbox.create("prometheus-rust-dev");
```

---

## Resources

- **E2B Dashboard**: https://e2b.dev/dashboard
- **E2B Documentation**: https://e2b.dev/docs
- **E2B Status**: https://e2b.dev/status
- **Template Guide**: https://e2b.dev/docs/template/quickstart

---

**Questions?** Check the README.md in this directory or consult E2B documentation.

**Ready to develop!** 🚀
