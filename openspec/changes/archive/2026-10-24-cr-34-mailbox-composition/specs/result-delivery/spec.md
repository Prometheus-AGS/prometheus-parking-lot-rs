## ADDED Requirements

### Requirement: Storage before notification

A result SHALL be durably stored before any notification, and a notifier failure SHALL NOT remove or block the stored result.

#### Scenario: Notifier down

- **WHEN** the notifier fails every call
- **THEN** results are still stored and consumable
