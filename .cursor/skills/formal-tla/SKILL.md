---
name: formal-tla
description: Add or change a TLA+ model under specs/tla and check it with TLC. Use when specifying a collector, order, or gate state machine, or when a model and the code disagree.
---

# TLA+ for parangsae

TLC is the checker. A spec that has not been model-checked is a note, not a spec. Do not vendor the GPL formal-verify toolkit; the CI job runs `tlc2.TLC` from the pinned `tla2tools.jar` (TLA+ 1.7.4, TLC 2.19).

## When a model is worth writing

Write one when a state machine can accept a bad event: a sequence gap, a duplicate write, a paper-to-live transition. Keep the model small enough that TLC finishes in CI. Finite ids and one feed are enough for the ingest segment.

The first checked model is `specs/tla/IngestSegment.tla`. It is not the Rust collector. `crates/collector/src/segment.rs` only names the phases. Say that in the module comment when you add the next model.

## A change

1. Write the invariant in words, then in TLA+. An invariant that restates an action's `UNCHANGED` clause without an independent statement does not count.
2. Put `SPECIFICATION`, `INVARIANT`, and `PROPERTY` in the `.cfg`. Use one TLC worker. TLC 2.19 can miss a liveness violation when several workers run; `-workers 1` stays in CI.
3. Run TLC from `specs/tla` with `-metadir` outside the repo:

```bash
java -Xmx512m -cp /path/to/tla2tools.jar tlc2.TLC \
  -workers 1 -metadir /tmp/tlc-ingest \
  -config IngestSegment.cfg IngestSegment.tla
```

4. Before trusting a new invariant, break it on purpose (append during `gap`, or accept a duplicate id). TLC must exit non-zero and name the invariant. Restore the spec. Do not weaken the invariant to match a bug.
5. Record, in the module comment, what the finite model omits. If the Rust transitions start to exist, either update the model in the same change or write that the model was not updated and why.

## Results

Keep TLC's verdict. `No error` is the success line. A violation is a finding, not a reason to delete the property. CI fails when TLC exits non-zero.
