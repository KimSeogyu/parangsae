# Paper risk authority

`RiskGate` owns a single BTCUSDT paper session's approvals, reservations, settled
position, and halt epoch. Its only normal dependency is `parangsae-core`.
Execution owns the accounting ledger and lifecycle records. A caller snapshot
cannot replace the gate's position or reservations.

`check` reserves an immutable `(intent, attempt, symbol, side, quantity,
reduce_only)` request. Its approval binds a private issuer instance, issue time,
ticket, and halt epoch. Approvals cannot be cloned or deserialized.
An initial risk refusal also retains an immutable order identity. Capacity is a
shared explicit bound for retained order identities (including refusals), unique
priced receipts, settlement batch entries, terminal receipt keys, and a complete
reconcile cut. Capacity exhaustion refuses new evidence without evicting old
identities or releasing reservations. It cannot create a new refusal tombstone
when identity storage is already full. Exact retained receipts remain no-ops at
full capacity when the submitted batch itself stays within the configured bound.
`prepare_dispatch` consumes the approval and rechecks the current gate and dated
instrument rules. A legitimate unsafe approval yields a staged unsent refusal.
The caller commits that refusal together with its lifecycle change. Dropping a
prepared operation changes no economic state and keeps its reservation held.

`prepare_settlement` checks an entire batch of priced receipts and terminal cuts
on a private draft. The reported candidate ledger position must equal the
position calculated from the bound orders. The candidate equity is evaluated
against the initial paper equity supplied in the frozen run configuration.
Only infallible assignments remain at commit. The execution engine must prepare
its complete ledger and lifecycle candidates before it commits this draft.

Buy and sell reservations use independent low/high position endpoints. Opposing
orders cannot cancel each other's reserved exposure. Notional upper bounds round
up. Partial fills update position and remaining reservation together. Unknown
orders retain their reservation. A cancellation acknowledgment alone does not
provide a complete terminal receipt cut. Reconciliation compares a complete
paper cut; it never adopts or repairs a mismatch. Resume is a separate explicit
operation and cannot revive an older epoch's approval.

Every explicit halt increments the epoch. Session loss stays latched after
recovery, with no automatic day reset. A moving causal mark can breach a bound
and latch risk. Fresh, instrument-valid, order-size-bounded reduce-only orders
remain possible when they strictly reduce every possible ordering of reserved
fills without crossing zero. That exception permits protective closes; it does
not assert that an already-breached moving-price cap now holds. Unknown exposure
does not receive this exception. Loss limits are halt triggers, not guaranteed
maximum realized losses.

The earlier `origin/risk/v0-spec` model and its independent bounded BFS record
cover a fixed positive price, full fills, finite intents, and no reset. They do
not prove this runtime, moving marks, approval age, partial fills, receipt
reconciliation, or restart. Runtime Gherkin, properties, and authority checks
exercise those interfaces. End-to-end execution and durable ingress replay need
their own integration evidence. This crate grants no network or storage effect.
