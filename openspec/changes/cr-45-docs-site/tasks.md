Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-45-docs-site / <task id>`.

## 1. Branded Docusaurus documentation site (know-me brand, Flat 2.0)

- [ ] 1.1 Scaffold website/ with build-branded-docusaurus; pin versions; map know-me dark/light tokens, typography and logo
- [ ] 1.2 Add Mermaid, local search, sanitizer (no private paths/secrets/raw wiki), SITE_URL/BASE_URL, container
- [ ] 1.3 Vendor the build-branded-docusaurus verify script as `npm run verify` (build, links, light/dark screenshots, a11y) and hand platform-engineer the GitHub Pages workflow and verify job
- [ ] 1.4 Independent review at the merge gate (brand fidelity, a11y, sanitizer)
- [ ] 1.5 Add .github/workflows/docs.yml: build website/ on pull requests (no deploy); on main build with SITE_URL=https://prometheus-ags.github.io BASE_URL=/prometheus-parking-lot-rs/ and deploy via actions/upload-pages-artifact + actions/deploy-pages (permissions pages: write, id-token: write; concurrency group pages)
- [ ] 1.6 Enable GitHub Pages with source "GitHub Actions" (gh api repos/Prometheus-AGS/prometheus-parking-lot-rs/pages); after the first successful deploy returns HTTP 200, set the repository About homepage to the docs URL (gh repo edit --homepage) and record both in execution.md
