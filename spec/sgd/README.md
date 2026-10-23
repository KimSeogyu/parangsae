# Fact write pilot

This pilot compiles `FactWriteEnvelope.tla` into whole-row authority used by the collector. The binding derives payload identity from the actual Rust Fact equality and keeps excluded metadata and normalized raw fields in provenance. Native results use the source action identifiers `Insert`, `Same`, and `Clash`. The original `ManifestWrite.tla` remains the abstract payload contract.

## Install and verify

Use Node.js 22 or later, Cargo with rustfmt, and Java. CI uses Java 21. The pinned source SDK prepares its JavaScript during installation and builds its locked Rust source analyzer on the first whole-row check. Lifecycle scripts must be enabled. No agent installation or global configuration is changed.

Run from the application root:

```sh
npm --prefix spec/sgd ci
npm --prefix spec/sgd run fetch-tlc
npm --prefix spec/sgd run verify
# A relative explicit JAR path is also resolved from the application root:
npm --prefix spec/sgd run verify -- --jar specs/tla/tla2tools.jar
```

The SDK dependency is fixed to a complete Git commit in `package.json`. The committed npm lockfile fixes the resolved dependency tree. Installation uses that source revision and its `prepare` script, rather than an adjacent SGD checkout or the historical diagnostic-only prerelease. The TLC downloader accepts only the official 1.7.4 release and verifies the installed SDK's JAR pin before publishing the file. An existing mismatched JAR fails instead of being replaced.

`verify` reruns controlled TLC builds, the finite input oracle, supported symbolic orbit checks, native source analysis, row-copy binding, and generation. It checks the existing managed output and receipts with `build --check`. It then checks the explicit abstract-to-concrete erasure with `refine --check` and compiles both generated TypeScript targets with the installed SDK's exact TypeScript 7.0.2 and `--noEmit`.

Verification does not rewrite tracked generated output. It creates disposable analyzer/TLC work and a fresh Pele view at `.runtime/sgd-view/index.html`. The view contains the configured finite witnesses and supported symbolic relations for both models. The checked erasure is recorded separately in `generated/fact-erasure/refinement.json`; a visual link alone is not a refinement proof.

## Change a contract

After changing supported model or source semantics, regenerate explicitly:

```sh
npm --prefix spec/sgd run update
npm --prefix spec/sgd run verify
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Review the source, model, binding, generated native code, and receipt changes together. Each output directory has its own `.sgd-generated.json` ownership inventory:

| Output | Contract |
| --- | --- |
| `generated/fact-write` | Actual Rust Fact binding and complete payload/provenance owner |
| `generated/manifest-write` | Original per-key payload model |
| `generated/fact-erasure` | Enabled-input, action, and complete projected successor erasure |
| `.runtime/sgd-view` at the application root | Disposable offline Pele rendering |

For an SDK upgrade, change the exact Git revision, update the npm lockfile, install in a clean dependency directory, run `update`, and review all generated differences before running `verify`. To roll back, restore the previously reviewed SDK pin, lockfile, and generated artifacts together, then run `npm ci` and `verify`.

## Proposed CI activation

The command is available locally; it is not a required CI check yet. The proposed insertion into the existing `spec-arch` job is Node setup, locked npm installation, and this command after the existing checksum-pinned TLC download:

```yaml
- uses: actions/setup-node@v4
  with:
    node-version: "24"
    cache: npm
    cache-dependency-path: spec/sgd/package-lock.json
- name: Install pinned SGD SDK
  run: npm --prefix spec/sgd ci
- name: Check Fact source binding, models, erasure, and Pele
  run: npm --prefix spec/sgd run verify -- --jar specs/tla/tla2tools.jar
```

The complete unapplied change is in [ci-activation.patch](ci-activation.patch). The job also needs the rustfmt component on its Rust toolchain step. This insertion changes the behavior of a required CI check. Root `AGENTS.md` principle 12 requires human approval before activating it; branch protection is not changed by this proposal. `spec/arch/*.tla` and `xtask/**` changes also require Ops/Release review. Do not activate the workflow by hiding the command inside an existing tool or lifecycle hook.

## Local verification record

The complete workspace tests passed after the direct source-action integration. Collector temporary-Parquet and Gherkin checks preserve first rows, raw schema and receipt time, all conflict observations, unchanged counts, and whole-partition rollback. Workspace Clippy with denied warnings and formatting passed. ArchitecturePolicy passed TLC, generation freshness, and source enforcement. Existing core and instrument Kani harnesses passed; those harnesses do not prove generated row ownership.

Independent implementation and architecture review found no concrete behavior regression. The architecture change adds the pure generated crate and its collector dependency without adding effects, capabilities, or writer grants. Archive ObjectManifest and ManifestLog behavior is unchanged from the selected base. This branch depends on pending Kline schema PR #5; coverage PRs #3 and #6 remain separate integration work.

On 2026-10-24, the pinned Git dependency installed with its preparation step, and a subsequent `npm ci` succeeded using an empty task-local npm cache with SSH explicitly disabled. The installed SDK completed `update` and `verify` against this application. The verification command rejected a modified generated abstract owner with `TLC104`; after restoration it passed and preserved every managed artifact. The installed-SDK update preserved the existing v5 native code and stable artifacts. Pele rendered all four model views.

This trial used macOS arm64, Node.js 26.0.0, npm 11.12.1, and the already installed Java 17.0.9. The proposed CI environment uses Node.js 24 and Java 21; that workflow remains unactivated and this local trial does not establish its result or a fully fresh operating-system installation.

## Guarantee boundary

The v5 source binding covers inventoried production Rust libraries and audited local type dependencies. It checks actual native equality, complete owned row identities, permitted transition entry, and paired payload/provenance copying. Unsupported source or model constructs fail. Generated rows are prepared before committing their symbolic transition.

The erasure check covers the supported equality-and-copy programs built in this invocation. Fresh build receipts establish source and tool provenance separately. This does not prove arbitrary TLA+, all Cargo targets or features, every writer path, partition commit effects, persistent identity, storage atomicity, or crash recovery. The collector's handwritten partition fold and Parquet effects retain their separate native behavior tests. Existing workspace, cucumber, architecture, TLC, and Kani checks remain necessary; their current passing state alone does not establish sustainable adoption.
