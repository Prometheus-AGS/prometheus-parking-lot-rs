# iggy-events Specification

## Purpose
Publish lifecycle events and mailbox notifications through Iggy while keeping it separate from task and result storage.

## Requirements

### Requirement: Event delivery via Iggy

With the iggy feature, lifecycle events and mailbox-ready notifications SHALL be published to Iggy with deterministic message IDs, without blocking dispatch.

#### Scenario: Subscriber receives

- **WHEN** a subscriber reads the stream after a task completes
- **THEN** it receives mailbox.ready for that task and can replay from an earlier offset
