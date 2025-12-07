# Data Model: Prometheus Parking-Lot Scheduler & Distributed Parking Management Platform

## Entities

### ResourcePool
- **Fields**: `pool_id`, `name`, `max_units`, `max_queue_depth`, `default_timeout_secs`, `queue_backend`, `mailbox_backend`, `runtime_adapter`, `priority_policy`.
- **Relationships**: Owns many `ScheduledTask`; bound to one `TaskQueue` and one `Mailbox`.
- **Rules**: Active units must never exceed `max_units`; queue length capped at `max_queue_depth`.
- **State**: Active (accepting), Draining (finishing only), Disabled (rejecting).

### TaskMetadata / ScheduledTask
- **Fields**: `task_id`, `pool_id`, `tenant`, `priority`, `resource_kind`, `resource_units`, `deadline`, `created_at`, `mailbox_key`, `status` (submitted|queued|running|completed|failed|expired|rejected|dropped), `reason` (for failure/reject/expire).
- **Relationships**: Belongs to one `ResourcePool`; optionally references one `Mailbox`.
- **Rules**: Reject if pool queue is full; expire if `deadline` passed before start; never start if it would exceed pool capacity.
- **State transitions**: submitted → running | queued | rejected; queued → running | expired | dropped; running → completed | failed | expired.

### TaskQueue
- **Fields**: `queue_id`, `pool_id`, ordered entries of `ScheduledTask` with priority and deadline indexes.
- **Relationships**: One per `ResourcePool` (backend-abstracted).
- **Rules**: Enqueue only if under `max_queue_depth`; dequeue respects priority/fairness; prune expired tasks.

### Mailbox / MailboxMessage
- **Fields** (MailboxMessage): `message_id`, `task_id`, `mailbox_key` (tenant, user_id?, session_id?), `status`, `payload`, `created_at`.
- **Relationships**: One `Mailbox` per pool config; messages indexed by `mailbox_key`.
- **Rules**: Deliver outcomes even if client disconnected; retrieval must be bounded by `since` and `limit`; durability depends on backend selection but consistent behavior required.

### SchedulerConfig / PoolConfig
- **Fields**: `pools[]` of `ResourcePool` definitions; defaults for timeouts, backends, and capacity units.
- **Rules**: Config must load before runtime; validation ensures numeric limits and backend compatibility.

### ParkingDomain Entities
- **Vehicle**: `vehicle_id`, `plate`, `size` (small/medium/large), `type` (car/truck/van/motorcycle), immutable identity.
- **ParkingSpot**: `spot_id`, `kind` (compact/regular/oversized), `location` (e.g., level/row/position), `state` (free/occupied/reserved), `current_vehicle?`.
- **Ticket**: `ticket_id`, `vehicle_id`, `spot_id`, `entry_time`, `exit_time?`, `pricing_strategy`, `amount?`, `status` (open/closed/void).
- **ParkingManager**: Manages inventory, allocation strategy, pricing strategy, spot/vehicle maps.
- **PricingStrategy**: Rules for time-based tiers and surge multipliers.
- **Relationships**: Vehicle ↔ Ticket (one-to-many), ParkingSpot ↔ Ticket (one-to-many over time), ParkingManager orchestrates allocation of Vehicle to ParkingSpot and issuance of Ticket.
- **Rules**: Compatibility checks per spot kind and vehicle size; contiguous allocation for large vehicles when no oversized spots; pricing uses defined strategy (time-based with optional surge).
