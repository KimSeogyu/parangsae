# C1 report domain verification

The report domain uses a separate SGD SDK installation. It does not replace the
legacy SDK pin under `spec/sgd`, which still verifies the collector and existing
generated contracts. The project references the original ReportCustody model
and retains `BatchEvidence` in the generated Rust owner.

The SDK archive is pinned to the `v0.3.0-alpha.2` release and its integrity is
recorded in `package-lock.json`. Install dependencies with `npm ci` in this directory. The
Rust backend requires Rust 1.92.0 and its matching rustfmt. Install that
toolchain and select it before verification:

```sh
rustup toolchain install 1.92.0 --profile minimal --component rustfmt
export RUSTUP_TOOLCHAIN=1.92.0
```

Set `SGD_LEAN_BINARY` to an absolute path to Lean 4.35.0. Place the pinned TLC JAR at
`specs/tla/tla2tools.jar`, or pass its path relative to the application root:

```sh
npm --prefix spec/sgd/report-domain run verify -- --jar specs/tla/tla2tools.jar
```

`verify` runs controlled TLC and Lean and compares all managed output with the
tracked generated files. It does not update them. To regenerate after reviewing
a change, run `npm --prefix spec/sgd/report-domain run update` and then `verify`.

The proof covers the finite model and an abstract payload token. The Rust
artifact check compares specific emitted source structures to that model; it is
not a proof of native Rust, filesystem effects, crash durability, or unchanged
payload contents under interior mutation. The application custody tests check
the original batch and report failure behavior separately.
