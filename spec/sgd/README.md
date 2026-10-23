# Fact write pilot

This pilot compiles the v6 `partition.json` composition into the collector's closed partition data fold. The referenced v5 `fact-write.json` binds `FactWriteEnvelope.tla` to actual Rust Fact equality, preserving excluded metadata and normalized raw fields in provenance. `PartitionMergeControl.tla` governs every Insert/Same/Clash observation, close choice, and one-time consumption. `PartitionFamilies.tla` supplies literal key, symbol, timestamp, calendar, and dataset metadata. The original `ManifestWrite.tla` remains the abstract payload contract.

The generated fold owns row comparison, key grouping, observation counts, complete conflicts, and whole-trial refusal. Its Write, Noop, and Refused reports are unbound data. The application still chooses physical paths, loads Parquet, writes accepted rows, and appends conflict logs. This stage does not establish loaded-resource or writer authority.

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

`verify` reruns controlled TLC builds, the finite input oracle, supported symbolic orbit checks, native source analysis, row-copy binding, literal family metadata admission, complete row/control composition checks, and generation. It checks the existing managed output and receipts with `build --check`. It then checks the explicit abstract-to-concrete erasure with `refine --check` and compiles both generated TypeScript targets with the installed SDK's exact TypeScript 7.0.2 and `--noEmit`.

Verification does not rewrite tracked generated output. It creates disposable analyzer/TLC work and a fresh Pele view at `.runtime/sgd-view/index.html`. The view contains the configured finite witnesses and supported symbolic relations for the abstract payload, complete-row, and finite partition-control models. Yellow dashed cards list numbered self-transition observations; they are annotations, not additional model states. The checked erasure is recorded separately in `generated/fact-erasure/refinement.json`; a visual link alone is not a refinement proof.

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
| `generated/fact-write` | Closed unbound partition fold, actual Fact binding, and complete row/control evidence |
| `generated/fact-write/models/row` and `models/control` | Controlled model evidence compiled within the single owned v6 output |
| `generated/manifest-write` | Original per-key payload model |
| `generated/fact-erasure` | Enabled-input, action, and complete projected successor erasure |
| `.runtime/sgd-view` at the application root | Disposable offline Pele rendering |

For an SDK upgrade, change the exact Git revision, update the npm lockfile, install in a clean dependency directory, and run `verify` first. An upgrade that changes only the viewer should preserve every tracked generated artifact. If an intended compiler change requires regeneration, run `update` separately and review all generated differences before running `verify` again. To roll back, restore the previously reviewed SDK pin, lockfile, and generated artifacts together, then run `npm ci` and `verify`.

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

### Earlier v5 trials

The complete workspace tests passed after the direct source-action integration. Collector temporary-Parquet and Gherkin checks preserve first rows, raw schema and receipt time, all conflict observations, unchanged counts, and whole-partition rollback. Workspace Clippy with denied warnings and formatting passed. ArchitecturePolicy passed TLC, generation freshness, and source enforcement. Existing core and instrument Kani harnesses passed; those harnesses do not prove generated row ownership.

Independent implementation and architecture review found no concrete behavior regression. The architecture change adds the pure generated crate and its collector dependency without adding effects, capabilities, or writer grants. Archive ObjectManifest and ManifestLog behavior is unchanged from the selected base. This branch depends on pending Kline schema PR #5; coverage PRs #3 and #6 remain separate integration work.

On 2026-10-24, the pinned Git dependency installed with its preparation step, and a subsequent `npm ci` succeeded using an empty task-local npm cache with SSH explicitly disabled. The installed SDK completed `update` and `verify` against this application. The verification command rejected a modified generated abstract owner with `TLC104`; after restoration it passed and preserved every managed artifact. The installed-SDK update preserved the existing v5 native code and stable artifacts. Pele rendered all four model views.

This trial used macOS arm64, Node.js 26.0.0, npm 11.12.1, and the already installed Java 17.0.9. The proposed CI environment uses Node.js 24 and Java 21; that workflow remains unactivated and this local trial does not establish its result or a fully fresh operating-system installation.

A subsequent maintenance trial upgraded the installed SDK to the then-pinned viewer-fix revision, repeated locked installation, and ran default `verify` without `update`. Every tracked generated artifact retained its exact bytes, including native code, the source-binding plan, build receipts, and erasure evidence. The freshly rendered viewer kept the exact four input models, laid out `Same` and repeated `Clash` observations in measured annotation cards, and retained intrinsic SVG dimensions without responsive width clamping. Application source and models were unchanged. SDK CI was pending when this local trial completed. The pinned source revision subsequently passed [its full Linux SDK workflow](https://github.com/KimSeogyu/specification-guided-development/actions/runs/40022244563), including the copied native plugin checks.

The initial verification-entry-point trial needed one TypeScript 7 integration correction: the package exports do not expose `typescript/bin/tsc`. The command now resolves the exported package metadata and uses its declared `bin.tsc` entry point. A later diagnostic check exercised a child failure reported only on stdout and confirmed that the entry point retains that output. These interventions changed the verification integration, not the application contract. This maintenance trial exercised the application's pinned SDK workflow; installed Codex and Claude Code plugin upgrades remain separate acceptance work.

### Closed-fold migration

The installed pinned v6 SDK completed `update` and readonly `verify` against this application. The gate checked the complete row source binding, finite controller, literal family metadata, composition, payload erasure, generated TypeScript 7.0.2, and fresh Pele output. The view contains the abstract payload witness and symbolic relation, the complete-row witness and symbolic relation, and the partition controller.

`cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `cargo test --workspace` passed after the migration and count-preflight correction. This includes the actual collector Parquet integration tests, the new baseline-group Gherkin scenario, complete-row property checks, repeated conflict observations, schema equivalence, first receipt preservation, and conflict-log error behavior. `cargo xtask spec validate --jar specs/tla/tla2tools.jar`, `cargo xtask spec generate --check`, and `cargo xtask spec enforce` also passed. These checks do not establish writer authority or sustainable adoption.

The new symbol and UTC-day baseline regression tests also ran against the prior committed collector, core types, and generated row authority in an isolated three-package workspace with a fresh Cargo target. Both failed because the old key-only fold accepted a new row and returned inserted one. Both pass with the generated closed fold, which refuses the mismatched supplied groups and preserves the actual target Parquet bytes without conflict-log writes. A shared-target attempt was discarded because Cargo reused current artifacts; only the fresh-target failure supports the before-and-after result.

Independent source review confirmed that ObjectManifest, ManifestLog, Parquet codecs, and conflict-log formatting remain unchanged. Count conversion and checked cross-day addition now finish before writer or log effects. Required-CI activation remains unapplied and awaits human approval.

## Guarantee boundary

The v5 source binding covers inventoried production Rust libraries and audited local type dependencies. It checks actual native equality, complete owned row identities, permitted transition entry, and paired payload/provenance copying. Unsupported source or model constructs fail. Generated rows are prepared before committing their symbolic transition.

The erasure check covers the supported equality-and-copy programs built in this invocation. Fresh build receipts establish source and tool provenance separately. This does not prove arbitrary TLA+, all Cargo targets or features, every writer path, partition commit effects, persistent identity, storage atomicity, or crash recovery. The v6 generator checks complete row roles and finite control laws, and generates the signed key fold, grouping checks, checked counts, retained conflict pairs, close, and consuming report. Native collection behavior still needs actual tests; the finite controller alone does not prove arbitrary maps, batch lengths, counters, or ordering. Parquet paths, loaded baselines, writes, and conflict-log effects remain ordinary application code with separate native behavior tests. Existing workspace, cucumber, architecture, TLC, and Kani checks remain necessary; their current passing state alone does not establish sustainable adoption.

## Closed-fold behavior changes

All four production Fact families now use generated sealed projections. The storage wrapper no longer supplies day or lookup-key callbacks, and it no longer reimplements row-action classification or partition folding. Its remaining key-text and payload callbacks format conflict logs only. Cross-partition count addition and conversion of conflict counts fail on u64 overflow. Both checks finish before that partition's writer or conflict-log effects.

The fold refuses supplied baseline and incoming rows that disagree on exact symbol or nonnegative UTC-microsecond day index before returning a report. This adds refusal for a malformed loaded baseline that the previous key-only fold could merge. The check compares supplied rows with each other; it does not authenticate a path, dataset selection, baseline source, or writer. Dataset names in literal metadata describe the admitted family vocabulary without changing the public storage API's dataset argument.

A Noop report now retains the exact original baseline order and duplicate rows. The old private helper returned a normalized last-wins vector even without an insertion; the public physical Noop already left Parquet unchanged. A real Write still materializes the last complete baseline row for each key, in signed key order. Refused trials retain the original baseline, every complete conflict pair including repeated state stutters, the complete Same count, and inserted zero.

Only Write reports reach the ordinary Parquet writer. Noop returns counts without writing; Refused appends conflicts in encounter order without rewriting the partition. A log failure returns an error immediately. Earlier log lines may already exist; logging is not atomic, and there is no cross-day or multi-file atomicity claim.
