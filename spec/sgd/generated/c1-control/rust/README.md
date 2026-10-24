# Generated finite-owned Rust authority

This standalone library crate is named `sgd-owned-c1opportunitycontrol-50dd0338899d`. Keep it as a separate Cargo dependency so Rust enforces its private state boundary. Build it with `cargo build` and run its revision-boundary test with `cargo test`.

Add a path dependency to the application:

```toml
[dependencies]
sgd-owned-c1opportunitycontrol-50dd0338899d = { path = "../generated/rust" }
```

Use `C1ControlMachine::new()` to create an owner, `read()` to copy its full concrete snapshot, and `send(Event)` to request a transition. Import these types from `sgd_owned_c1opportunitycontrol_50dd0338899d`. `Event::from_model_name` maps an exact model action label to its typed event; `Event::model_name` returns that label, and `ALL_EVENTS` lists all supported events. Encoded Rust identifiers preserve a distinct mapping for every supported label.

A send succeeds only when the exact current concrete node has an edge for that action. It returns a receipt containing the event and the before/after snapshots. Rejection returns a reason and copied current state; state and revision stay unchanged. Dispatch is synchronous and runs no application callback. Snapshot values are tagged string, boolean, or integer values. Integers and the checked u64 revision serialize as decimal strings. Editing a copied snapshot cannot change the owner. `modelRevision` identifies the source DOT.

The owner has private node and revision fields, no Clone or Deserialize implementation, no mutable state references, and no arbitrary-state constructor or restore API. The generated crate forbids unsafe code. Callers must use safe Rust and trust the generated crate and its dependencies. Changing generated source, replacing the crate, or using unsafe or foreign code is outside this boundary.

This is a per-live-object finite transition contract. Each `new()` creates a separate object at Init. Persistent entity identity, reset prevention, restart, restore, database effects, effect atomicity, liveness, application-wide coverage, and application refinement require separate contracts. The graph covers only the finite model that TLC explored.
