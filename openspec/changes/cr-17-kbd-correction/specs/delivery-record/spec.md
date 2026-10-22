## ADDED Requirements

### Requirement: Truthful delivery record

The canonical KBD audit SHALL record, for every prior change whose completion overstated delivery, a superseding decision naming the gap and the finishing change.

#### Scenario: Audit shows correction

- **WHEN** `prometheus kbd audit` is read after this change
- **THEN** decisions for CR-03, CR-07, CR-13 and CR-14 each reference the finishing change in this phase
