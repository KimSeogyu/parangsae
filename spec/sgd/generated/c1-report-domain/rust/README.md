# Generated Rust domain

This dependency-free Rust 2021 crate owns one finite state per instance. Generation requires Rust 1.92.0 and its matching rustfmt; the generated rustfmt.toml fixes formatting settings. Implement `SyncPorts` and pass ports plus an owned payload to `ReportCustodySyncDomain::new` for synchronous operations. A sync port borrows the payload, receives a separate input, and returns a `Result`. Public events exclude operation roles.

The sync owner keeps its payload from construction through every ordinary operation return, including refusal and port failure. `payload()` borrows it; `into_parts()` consumes the owner and returns the payload, ports, and final snapshot. Payload fields with interior mutability can still change. Panic, abort, caller drop, unsafe code, and external effects are outside this guarantee. The checked Boolean model variable `resident` is true in every reachable graph state; the non-retaining async owner is omitted. 

This code does not establish cancellation, persistence, effect atomicity, eventual completion, application-wide coverage, or correctness of external effects. The graph is finite. The generated Rust, artifact checker, rustfmt, and Rust compiler remain trust boundaries separate from the Lean graph proof.
