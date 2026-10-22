---
title: Working on this site
description: Build, preview and verify the documentation site.
---

# Working on this site

The site lives in `website/` and uses Docusaurus with pinned dependencies.
Node.js 20 or newer is required.

## Build and preview

```bash
cd website
npm ci            # frozen install from package-lock.json
npm start         # live preview at http://localhost:3000
npm run build     # sanitizer, then production build into website/build
```

The build fails on a broken internal link or anchor.

## Publication URL

`SITE_URL` and `BASE_URL` select where the site is served. Local builds default
to `http://localhost:3000` and `/`. The GitHub Pages build uses:

```bash
SITE_URL=https://prometheus-ags.github.io \
BASE_URL=/prometheus-parking-lot-rs/ \
npm run build
```

## Verify

```bash
npm run verify
```

`verify` runs a frozen install, a sanitizer negative control, the production
build, a scan of the output for private material, an internal link check over
the built HTML, and browser checks of representative routes in light and dark
mode: brand tokens, Flat 2.0 (no borders, separator lines, gradients or
shadows), visible keyboard focus and an automated accessibility scan that fails
on serious or critical violations. Screenshots are written to
`website/verify-output/screenshots/`.

## Content rules

The sanitizer (`npm run sanitize`) fails the build when site sources contain
machine-local paths, secret-like values or raw agent wiki content. Write
documentation for a public reader; do not paste session logs or private notes.

## Container

`website/Dockerfile` builds the site and serves it as a non-root user on port 8080:

```bash
docker build -t prometheus-parking-lot-docs website
docker run --rm -p 8080:8080 prometheus-parking-lot-docs
```
