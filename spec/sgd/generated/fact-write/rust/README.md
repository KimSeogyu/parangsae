# Generated whole-row Rust authority

Keep the primitive crate separate. Import its package under the dependency alias `sgd_fact_authority`. Include `adapter.rs` as a private module in the audited source crate; it resolves the closed Fact paths from the checked binding. It is not a module of the primitive authority crate.

WriteEffect variants preserve the exact source action identity. There is no configurable classification or action rename table. The private application module exposes `FactOwner<T>::new()`, `write(T) -> Result<WriteEffect, RowError>`, `stored() -> Option<&T>`, `rejected() -> Option<&T>`, and `read()`. Only the analyzed concrete Fact types implement its sealed bound. A write moves the complete owned input row, computes payload/provenance together, prepares the generated primitive transition, lifts complete rows from the receipt origins, and checks the proposed relation before committing. Clones, encoding, validation and errors finish before commit. After commit only audited infallible field moves remain. Safe immutable references cannot mutate the retained closed rows.

Identity encoding is identical to the portable TypeScript target: decimal byte length, colon, lowercase hexadecimal bytes, semicolon for each component. Header components are UTF-8 `sgd-row-v1`, Fact name and `payload` or `provenance`. Each ordered field appends name, kind and exact value bytes: signed two's-complement big-endian i64, big-endian u32, one-byte bool, UTF-8 string, or UTF-8 enum variant. Payload finite u32 classes use their minimum member; provenance retains the original value. The adapter retains complete original rows, never reconstructed rows from normalized identities.

An exhaustive struct pattern detects new fields at native compilation. The source analyzer separately checks equality, codecs, field classification, and the immutable representation closure. Keep that gate mandatory. This per-live-object contract does not establish application-wide coverage, a partition write permission, persistence, restore, reset prevention, or external effects. The application module and generated source remain trusted; safe Rust and the audited no-custom-Drop/no-interior-mutation closure are required.

## Primitive crate reference

# Generated symbolic-owned Rust authority

This standalone crate is `sgd-symbolic-factwriteenvelope-281ebc3217c8`. Keep it as a separate dependency:

```toml
[dependencies]
sgd_fact_authority = { package = "sgd-symbolic-factwriteenvelope-281ebc3217c8", path = "../generated/rust" }
```

Import `sgd_fact_authority::FactEnvelopeMachine`. Create a live owner with `new()`, copy its complete state with `read()`, and submit actual UTF-8 input data through `send(&str, &str)` in model input order (v: Values, provenance: Receipts). The generated guards choose the action. There is no caller action enum, equality callback, generic data identity, or mutable input alias. Data are tagged with their domain and stay distinct from literal scalar strings and other domains, even when their text matches. Equality is exact Rust string equality; input bytes are not normalized.

The receipt contains `action`, `before`, `after`, and copied IR assignment origins in `updates`, including the source parameter on each input assignment. All guards and assignments read the old state. A rejected input preserves state and the checked u64 revision. Snapshots own their values; editing them cannot alter the authority. JSON integers and revision remain decimal strings. `modelRevision` identifies the canonical symbolic program.

Use `prepare(&str, &str)` when an adapter must validate and copy application values before committing. A successful `PreparedTransition<'_>` exclusively borrows its originating owner. `receipt()` borrows the complete proposed receipt. `commit(self)` consumes the token and returns that receipt after infallible field moves; dropping the token leaves state and revision unchanged. The token has no Clone, Deserialize, raw constructor, owner selector, or mutation API. The borrow checker prevents reads, sends, and another preparation on that owner while the token remains live. `send` uses this same prepare/commit path.

Generation validates the complete equality/copy orbit space, invariants, and action properties. This profile supports equality, finite scalar membership, domain tests, Boolean logic, and simultaneous literal/input/state copying. It does not establish application-wide coverage or external-effect atomicity. The authority has no Clone, Deserialize, raw constructor, setter, or restore API. This crate forbids unsafe code and requires safe Rust callers. Trust the generated crate and dependencies. Each `new()` creates a distinct live object; persistent identity, reset prevention, restart, recovery, and database effects require separate contracts.
