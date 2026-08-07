## 1. Coordinator storage foundation

- [x] 1.1 Add the coordinator workspace service, configuration loader, public `/health` and compatible `/nodes` routes, and production container entrypoint.
- [x] 1.2 Implement versioned SQLite migrations for sessions, payment intents/redemptions, generations, desired peers, and enrollment tokens with all required pragmas and constraints.
- [x] 1.3 Add token hashing and versioned authenticated encryption using secret-file inputs, ensuring logs and database indexes never contain raw session tokens.
- [x] 1.4 Test migration/restart persistence, unknown-schema rejection, uniqueness constraints, concurrent immediate transactions, and encrypted-token recovery.

## 2. Durable payment and lifecycle engine

- [x] 2.1 Implement generation registration, health renewal, atomic promotion, drain state, and safe-retirement status in the coordinator store.
- [x] 2.2 Implement durable payment-intent creation and unique transaction redemption with same-session lost-response retries and mismatched replay rejection.
- [x] 2.3 Implement paid-session creation, hashed lookup, atomic usage refresh, status, heartbeat, pause, expiry, and terminal resource release.
- [x] 2.4 Implement logical-node `/24` address allocation that preserves paused reservations and prevents concurrent duplicate assignment.
- [x] 2.5 Test restart persistence, atomic balance accounting, payment idempotency, concurrent address allocation, expiration, and administrative termination.

## 3. Generation peer ownership and drain

- [x] 3.1 Implement paused-session claim, `activating` transition, desired-peer revision creation, activation acknowledgement, and failure rollback.
- [x] 3.2 Implement `releasing` transitions for pause, stale lease, exhaustion, and expiry; block reassignment until peer removal acknowledgement or lease expiry.
- [x] 3.3 Implement complete desired-peer snapshots, applied-revision acknowledgements, actual-peer counts, and idempotent reconciliation status.
- [x] 3.4 Test uninterrupted blue ownership during green promotion, paused resume on green, changed WireGuard metadata, stale leases, and every safe-drain deletion gate.

## 4. Private API and node integration

- [x] 4.1 Add versioned coordinator request/response types and private routes for enrollment, generations, payments, sessions, reconciliation, promotion, and drain status.
- [x] 4.2 Add offline-root/online-intermediate mTLS configuration, single-use enrollment, generation-scoped authorization, certificate renewal, and operator authorization tests.
- [x] 4.3 Add coordinator-mode node configuration and client calls while retaining explicit local in-memory mode for isolated development tests.
- [x] 4.4 Route fixed purchase and lifecycle handlers through the coordinator, preserve public response compatibility, and return retryable unavailable/transitional failures.
- [x] 4.5 Add the node reconciliation loop that applies only managed desired peers, renews leases, removes locally expired peers, and reports revision/count acknowledgements.
- [x] 4.6 Test coordinator outage behavior, node restart reconciliation, WireGuard command compensation, challenge-key overlap, and private authorization failures.

## 5. Client and workflow compatibility

- [x] 5.1 Add Linux regression tests proving paused resume consumes the latest server public key/endpoint while retaining the logical node URL and local private key.
- [x] 5.2 Add macOS regression tests proving Network Extension profiles refresh generation metadata while private keys remain in the Keychain.
- [ ] 5.3 Run unchanged Linux, macOS, registry discovery, and agent purchase/connect/status/disconnect workflows against coordinator mode.
- [x] 5.4 Update node, client, operator, and agent documentation for logical-node ownership, retryable transitions, active-only drain, and rollback limitations.
