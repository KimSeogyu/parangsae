# Report custody lower contract

The model and configuration are copied from the SGD producer example in
`examples/retained-operation/`.
They retain the supplied `ReportCustody` model, guards, actions and
`PayloadRetained` invariant without changes. The application project selects
only the Rust target and keeps the source model and binding names aligned.
The producer revision and generated output are admitted separately by the
application SDK pin and its controlled build receipts.

This finite contract records preparation, report start, report success or
failure, and retained residency. Its configured instance starts ready. The
model does not prove ownership of an application value, filesystem effects,
crash durability, asynchronous cancellation, or financial correctness.
The generated retained-operation API and the actual Backtest caller must
supply that separate native ownership and error-path evidence.

The controlled output root is `spec/sgd/generated/c1-report-custody`, with a
separate Rust crate under `rust/`. It sits outside existing workspace members;
the root workspace excludes that exact crate path. Backtest's dependency and
the resolved generated package identity are checked in the combined change.
Generated source must come from the admitted producer and must not be edited
by hand. The application's readonly verifier checks this output without
regenerating tracked files.

After changing this model, regenerate explicitly from the application root:

```sh
node spec/sgd/node_modules/specification-guided-development/dist/cli.js build \
  --project spec/sgd/report-custody/project.json \
  --jar specs/tla/tla2tools.jar --out spec/sgd/generated/c1-report-custody
npm --prefix spec/sgd run verify
```
