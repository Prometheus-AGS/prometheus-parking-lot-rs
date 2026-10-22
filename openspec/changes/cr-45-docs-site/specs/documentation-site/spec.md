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
