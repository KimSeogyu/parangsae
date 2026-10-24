# Offline paper execution

`PaperEngine` routes scripted order data through the actual `RiskGate`, private
approval consumption, the order lifecycle, and the fixed-point `sim::Ledger`.
The engine owns all four states. Its public surface accepts `PaperCommand` data
and returns copied outcomes and snapshots.

## Session contract

Supply the symbol, flat initial cash, matching initial equity, causal initial
mark, injected monotonic and UTC clocks, explicit costs, Risk limits, and
Execution bounds. Initial equity remains the session loss anchor. Limits and
costs remain frozen for that session. There is no automatic reset or day rollover.

Active paper valuation requires event, availability and receipt times at or before
the decision. Initial and command marks are checked before constructing authority
or computing equity. Availability is checked first with `RiskError::FutureMark`;
a later event returns `FutureMarkEvent`, and a later receipt returns
`FutureMarkReceipt`. Command refusal latches once and preserves existing economic
state and reservations. Normally delayed marks are usable after receipt. No
ordering between the three source clocks is imposed. Their values remain
unchanged. This boundary applies to active paper inputs; historical research Tape
visibility remains separate.

Every admitted command mark is an independent portfolio observation, including
an exact receipt replay, an existing plan or a stale resume request. A duplicate
fill changes no economics; its fresh valuation can still latch session loss or
notional risk. A later recovered mark cannot clear a recorded session loss.

The first scope supports one BTCUSDT market-order stream. `Plan` creates a real
opaque approval and reservation. `Dispatch` consumes that approval through a
fresh Risk check. CID intent and attempt remain lossless, and the attempt must
fit the configured budget. Cancellation before dispatch releases the unsent
reservation through an atomic checked terminal update.

`Deliver` accepts scripted paper acknowledgments, priced fills and terminal
reports for already dispatched orders. Fill reference prices receive the
configured slippage and taker fee once. The engine prepares lifecycle, ledger
and Risk candidates before committing economic changes. A complete identical
fill identity is an ingress no-op. A changed payload under that identity halts
without changing the recorded economics. Receipt identity includes the complete
CID and fill ID. Original event and availability times remain significant;
later receipt-time provenance does not create another fill.

Cancellation confirmation retains any unresolved reservation. `RaceFill` can
still account for a late canceled-order fill. A complete terminal cut must name
every priced receipt and match cumulative quantity before residual exposure is
released. A definitive paper rejection releases its unfilled reservation.

Timeout creates `Unknown`, unknown exposure and a halt. A terminal recovery cut
must supply complete priced receipts; cumulative quantity alone cannot invent
price, fee or cash. `ReconcilePortfolio` compares the engine's separately tracked
orders and ledger with Risk. `Resume` must carry the caller's explicitly observed
`expected_epoch`. A stale value returns `EpochMismatch` without resuming, even
after a fresh matching reconciliation. Empty Resume records are refused by the
unreleased journal codec. Session loss remains latched.

Every lifecycle result of `Unknown` commits the matching Risk reservation phase
and halt before publishing the lifecycle result. This includes cancellation
denial before any acknowledgment or fill. Earlier approvals become stale, and
new exposure-increasing plans remain blocked until complete reconciliation and
explicit resume. Cancel denial after an acknowledgment or partial fill keeps
its existing known state and reservation.

The current lifecycle forbids late fills from `Expired`. A terminal Expired cut
with unaccounted priced receipts returns `ExpiredReceiptRecoveryUnsupported`,
retains conservative exposure and halts. Extending this boundary requires a
lifecycle specification change. Existing known receipts can finalize an Expired
cut.

`ReconcilePortfolio` refuses terminal lifecycle orders without a complete
finalized cut or definitive local refusal. An unsupported Expired recovery
cannot use that command and `Resume` to discard its outstanding reservation.

`Limits.capacity` bounds retained order identities, receipts, terminal evidence
and settlement batches. A terminal batch needs capacity for every new receipt
plus its terminal entry. Exact known replays remain possible at full receipt
retention. Evidence is never evicted to admit another event.

## Durable replay

`backtest::offline_replay::OfflineReplay` fsyncs typed ingress before applying it.
Recovery rebuilds real approvals and reservations by replaying the initial
session and inputs at their original injected times. The journal serializes
order data, never an approval credential. Continuation checks the exact frozen
session and already durable input prefix.

Each frame is limited to 1 MiB. A journal holds at most 64 MiB and 16,384 records,
including its session header. Recovery discards an incomplete final frame. A
complete damaged, out-of-order or schema-invalid record prevents recovery.
An uncertain append poisons its handle until reopening. One process must own a
journal; the format has no cross-process lock. The scope covers process restart
with the same engine protocol. It provides no power-loss or multi-writer proof.

Run the synthetic fixture from the repository root:

```bash
cargo run --locked -p parangsae-backtest --bin paper_replay -- \
  run fixtures/backtest/paper-replay/full-fill.json /tmp/parangsae-paper.journal \
  --stop-after 4
cargo run --locked -p parangsae-backtest --bin paper_replay -- \
  resume fixtures/backtest/paper-replay/full-fill.json /tmp/parangsae-paper.journal
```

The sample starts at 10,000 USDT and buys 0.004 BTC at a 30,000 USDT reference
price, with 5 bps fees and 2 bps slippage. Its final cash is 9,879.915988 USDT;
fees are 0.060012 USDT. This engineering fixture supplies no strategy research or
funding settlements. It reads no operational market data.

## Evidence

`specs/features/paper_replay.feature` exercises accounting, partial and duplicate
fills, conflicting evidence, delayed delivery, cancellation races, Unknown,
approval expiry, halt epochs, session loss and restart equivalence. Native tests
also cover an input committed before engine application, torn-frame recovery,
complete corruption refusal, attempt binding, capacity and the Expired boundary.

The existing Risk and OrderLifecycle TLA+ models supply bounded model evidence.
They do not prove this complete engine, arbitrary batches or filesystem effects.
Existing Kani harnesses cover arithmetic, and the native engine, Gherkin and
journal checks cover their stated runtime observations. The execution source
guards continue to refuse network clients and environment reads.
