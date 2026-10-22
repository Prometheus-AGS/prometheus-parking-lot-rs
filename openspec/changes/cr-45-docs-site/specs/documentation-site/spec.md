## ADDED Requirements

### Requirement: Branded, verified documentation site

The project SHALL publish a Docusaurus site whose theme uses the know-me brand tokens and Flat 2.0 rules in light and dark modes and passes build, link and accessibility checks.

#### Scenario: Production build

- **WHEN** the site builds with a frozen install
- **THEN** the build exits 0 with no broken links

#### Scenario: Brand fidelity

- **WHEN** light and dark screenshots of the home and a docs page are taken
- **THEN** canvas, surface, text, Ember and Cyan values match the know-me token table; no borders, gradients or decorative shadows

#### Scenario: Accessibility

- **WHEN** an automated accessibility check runs on representative routes
- **THEN** no serious or critical violations; keyboard focus is visible

### Requirement: Published on GitHub Pages and linked from the repository

A GitHub Actions workflow SHALL build the site on every pull request and deploy it to GitHub Pages from main, and the repository About section SHALL link to the published site (operator request 2026-10-22).

#### Scenario: PR build

- **WHEN** a pull request changes website/
- **THEN** the docs workflow builds the site and fails the check on a build or link error, without deploying

#### Scenario: Deploy from main

- **WHEN** a commit lands on main
- **THEN** the workflow deploys with actions/upload-pages-artifact and actions/deploy-pages, and https://prometheus-ags.github.io/prometheus-parking-lot-rs/ serves the new build (HTTP 200, expected title)

#### Scenario: About link

- **WHEN** the repository page is viewed
- **THEN** the About homepage is https://prometheus-ags.github.io/prometheus-parking-lot-rs/
