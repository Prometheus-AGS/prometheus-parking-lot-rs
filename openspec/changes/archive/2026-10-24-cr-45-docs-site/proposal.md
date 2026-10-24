## Why

The operator requires a branded documentation site; there is none. know-me-system docs/knowme-ui-ux-standard.md defines the brand (Flat 2.0, Ember #FF6A3D / Cyan #00C2DC, dark and light token tables, typography, logo).

## What Changes

Scaffold website/ with the build-branded-docusaurus skill: pinned Docusaurus, know-me tokens as CSS variables (dark + light), logo from know-me branding assets, Mermaid, local search, sanitizer, SITE_URL/BASE_URL, GitHub Pages workflow, non-root container; verify build, links, responsive light/dark screenshots and accessibility.

## Capabilities

### New Capabilities

- `documentation-site`: Branded, verified documentation site; Published on GitHub Pages and linked from the repository

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M0; owner role `product-manager` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-48-workspace-split`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
