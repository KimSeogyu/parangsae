# Generated closed partition data fold

Import this primitive crate as `sgd_fact_authority` and include `adapter.rs` privately in the source-audited application. Call `fold_partition(baseline, incoming)?.into_report()?` using one of the sealed generated Fact types. The returned Write, Noop, or Refused report is ordinary unbound data, not a storage permission.

The primitive package is `sgd-symbolic-factwriteenvelope-281ebc3217c8`. Add this application dependency, adjusting the relative path:

```toml
[dependencies]
sgd_fact_authority = { package = "sgd-symbolic-factwriteenvelope-281ebc3217c8", path = "../generated/rust" }
```

Run `cargo test --manifest-path /app/generated/rust/Cargo.toml` for the primitive crate, then run the application's native checks. In workspace mode, include the generated crate in the application's workspace members. Keep the original private adapter include outside generated output and regenerate after changing application sources.

Metadata binds each Fact's one to three raw signed i64 key fields, exact symbol, and nonnegative i64 microsecond time. All supplied baseline and incoming rows must share one symbol and UTC day index. Keys use signed lexicographic ordering, independently of identity encoding. The private row owners dispatch exact Insert/Same/Clash observations to a private finite controller. Closing and consuming follow the actual validated graph. Counts use checked u64 arithmetic. Noop and Refused preserve the exact original baseline; Write materializes last-wins baseline rows in signed key order. Every Clash retains a complete pair, including repeated identical state stutters.

The fold binds neither a selected dataset/path nor a loaded baseline or writer. It does not establish persistence, effect atomicity, global source/sink coverage, cross-process isolation, or a collection theorem beyond the admitted model and native runtime checks. Keep the original source binding mandatory.
