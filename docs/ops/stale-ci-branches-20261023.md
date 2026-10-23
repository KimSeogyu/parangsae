# Stale branch triage — 2026-10-23

Local snapshot: `origin/main = 973cff653cde011b6fd7a5a58496a8802e2918f3`. No remote refresh was possible. Ahead/behind counts include all commits, including merges. This is an offline branch-content audit; remote review threads and CI status were not inspected.

Nine delete recommendations, three branches requiring Seogyu, zero unprotected rebase-and-land candidates. No branches were deleted or rebased and no protected files were changed. Delete recommendations are based on supersession, not a claim that the changes landed in main. Keep all three retained branches until their protected changes are resolved.

| Branch | Tip | Ahead | Behind | Already in main? | GitHub-hosted ci.yml? | Verdict and evidence |
| --- | --- | ---: | ---: | --- | --- | --- |
| `ci/guards-first-push-9` | `5874cf968603` | 30 | 75 | No | Yes | **delete** — Superseded by the later CI series in ci/item13-1; original core changes remain in core/causal-hardening. |
| `ci/hollow-check-guards` | `068f54ba7c2c` | 45 | 66 | No | No; self-hosted, stale | **delete** — Exact ancestor of ci/item13-1 (eight additional commits). |
| `ci/item13-1` | `6f6144a68c88` | 53 | 66 | No | No; self-hosted, stale | **needs Seogyu** — Latest guard/command-inventory series; protected CI, xtask, policy, floors, deny, and manifest changes. |
| `ci/stack-final-1` | `2de1a8a1307f` | 33 | 75 | No | Yes | **delete** — Ancestor of stack-final-2; both superseded by ci/item13-1, with core preserved separately. |
| `ci/stack-final-2` | `4c4d18b4dee1` | 34 | 75 | No | Yes | **delete** — All 24 CI-series commits paired by range-diff with the later series in ci/item13-1. |
| `ci/stack-final-mutation-1` | `53cc52c36b90` | 34 | 75 | No | Yes | **delete** — Negative probe: tip changes the fmt job name to fmu; parent stack is superseded. |
| `ci/stack-final-mutation-2` | `3f1cc4bbe45f` | 35 | 75 | No | Yes | **delete** — Negative probe: tip changes the fmt job name to fmu; parent stack is superseded. |
| `lint/workspace-lints` | `9d4eb70a12eb` | 5 | 75 | No | Yes | **delete** — Exact ancestor of retained core/causal-hardening; not landed in main. |
| `xtask/depgraph` | `b7923c39cc03` | 8 | 75 | No | Yes | **delete** — Exact ancestor of retained core/causal-hardening; not landed in main. |
| `core/causal-hardening` | `57d40dd8761d` | 10 | 75 | No | Yes | **needs Seogyu** — Retain: sealed Observation/decision clock is absent from main and was undone in the later CI stack; protected manifests and inherited CI/xtask/policy. |
| `data/collector-integrity` | `4f241b8bce38` | 18 | 75 | No | Yes | **needs Seogyu** — Retain: dataset version/receipt/audit implementation is absent from main; protected manifests, AGENTS.md, tools reference. |
| `data/dataset-manifest` | `ef56657566a0` | 9 | 75 | No | Yes | **delete** — Exact ancestor of retained data/collector-integrity (nine additional commits). |

All twelve branch tips are outside main ancestry. `git cherry origin/main origin/<branch>` marked every branch-only commit `+`, so there are no exact patch-equivalent commits in main either. Tree checks also found branch-added implementations/fixtures absent from main. ArchitecturePolicy on main overlaps dependency enforcement but does not establish equivalence to these branches.

The seven `ci/*` branches include five GitHub-hosted workflows and two self-hosted workflows. All twelve workflows differ from main. Main uses `[self-hosted, linux, x64, parangsae]` for every job. The other five non-CI branches also retain GitHub-hosted CI, including the data branches whose own commits did not edit ci.yml.

## Supersession evidence

- Exact ancestry: `lint/workspace-lints` → `xtask/depgraph` → `core/causal-hardening` → `ci/guards-first-push-9` → `ci/stack-final-1` → `ci/stack-final-2`. Retaining core preserves the original lint/depgraph/causal changes while removing duplicate older branch names.
- Exact ancestry: `ci/hollow-check-guards` → `ci/item13-1`; `data/dataset-manifest` → `data/collector-integrity`.
- `git range-diff --no-patch 57d40dd..origin/ci/stack-final-2 0f9bb17..origin/ci/hollow-check-guards` pairs all 24 old CI commits with the rewritten series: 14 identical patches and 10 edited patches, with no unmatched old CI commit. The later branch adds reconciliation work and is an ancestor of item13. The edited patches are treated as superseded versions, not byte-identical content.
- Mutation tips `53cc52c36b90` and `3f1cc4bbe45f` only change `.github/workflows/ci.yml` from `name: fmt` to `name: fmu`. These intentional negative probes must not land.
- Preserve `core/causal-hardening`: later CI commit `3d9daa543792` restored crate sources to then-main, removing the sealed core API and much of its enforcement. Main still exposes `Observation::new`, `Observation::value`, `Observation::with_value`, and `DecisionTime::from_micros`. Therefore item13 is not a replacement for the original causal hardening.
- Preserve `data/collector-integrity`: main lacks `crates/collector/src/version/`, `crates/collector/tests/dataset_manifest.rs`, `crates/audit/`, and `crates/core-consumer-ui/`. This is unlanded work, not a stale name for main content.

## Protected integration work for Seogyu

`ci/item13-1` already uses self-hosted runners, but its ci.yml omits main's `spec-arch` job and the execution `OrderLifecycle` TLC step. Its manifest/lock pins, command inventory, and test counts predate current main. Reconcile these with the current self-hosted workflow and current required checks; do not replace main's workflow wholesale. Its xtask entry points also compete with the current architecture engine.

`core/causal-hardening` changes producer/decision APIs and consumers, inherits the old lint/depgraph engine, and edits crate manifests and CI. It requires current-consumer integration and a deliberate reconciliation with ArchitecturePolicy.

`data/collector-integrity` changes timestamp receipts, adds audit/consumer crates and immutable dataset versions, and edits manifests, AGENTS.md, and a tools reference. Current ArchitecturePolicy does not list the added crates; integration may require a reviewed `.tla` policy change. No stored market data or holdout was opened for this audit.

Protected paths in the retained branches, measured against each branch's merge-base with main (these were inspected only):

### ci/item13-1 — 47 protected paths

```text
.cargo/config.toml
.github/workflows/ci.yml
.github/workflows/gate_results.py
.github/workflows/vision-smoke.yml
AGENTS.md
Cargo.toml
deny.toml
fixtures/causal/closed-gate/Cargo.toml
fixtures/deny/banned-reqwest/Cargo.toml
fixtures/depgraph/driver-clock-other/Cargo.toml
fixtures/depgraph/driver-clock-other/core/Cargo.toml
fixtures/depgraph/driver-clock-other/signals/Cargo.toml
fixtures/depgraph/test-support-dev/Cargo.toml
fixtures/depgraph/test-support-dev/core/Cargo.toml
fixtures/depgraph/test-support-dev/signals/Cargo.toml
fixtures/depgraph/test-support-normal/Cargo.toml
fixtures/depgraph/test-support-normal/core/Cargo.toml
fixtures/depgraph/test-support-normal/signals/Cargo.toml
fixtures/guards/unguarded-notional/Cargo.toml
fixtures/lint/missing-inheritance/Cargo.toml
fixtures/lint/unsafe-forbid/Cargo.toml
fixtures/lint/violation/Cargo.toml
policy/deps.toml
policy/features.toml
regressions/floors.toml
xtask/Cargo.toml
xtask/src/depgraph.rs
xtask/src/gate_fixture_step.txt
xtask/src/guards.rs
xtask/src/lib.rs
xtask/src/lints.rs
xtask/src/main.rs
xtask/src/path_filter_step.sh
xtask/src/pinned_jobs/changes.job
xtask/src/pinned_jobs/ci.header
xtask/src/pinned_jobs/clippy.job
xtask/src/pinned_jobs/cucumber.job
xtask/src/pinned_jobs/deny.job
xtask/src/pinned_jobs/depgraph.job
xtask/src/pinned_jobs/fmt.job
xtask/src/pinned_jobs/gate.job
xtask/src/pinned_jobs/guard-fixtures.job
xtask/src/pinned_jobs/guards.job
xtask/src/pinned_jobs/kani.job
xtask/src/pinned_jobs/lint-fixture.job
xtask/src/pinned_jobs/test.job
xtask/src/pinned_jobs/tla.job
```

### core/causal-hardening — 29 protected paths

```text
.cargo/config.toml
.github/workflows/ci.yml
AGENTS.md
Cargo.toml
crates/collector/Cargo.toml
crates/core/Cargo.toml
crates/execution/Cargo.toml
crates/regime/Cargo.toml
crates/signals/Cargo.toml
crates/sim/Cargo.toml
fixtures/causal/closed-gate/Cargo.toml
fixtures/depgraph/driver-clock-other/Cargo.toml
fixtures/depgraph/driver-clock-other/core/Cargo.toml
fixtures/depgraph/driver-clock-other/signals/Cargo.toml
fixtures/depgraph/test-support-dev/Cargo.toml
fixtures/depgraph/test-support-dev/core/Cargo.toml
fixtures/depgraph/test-support-dev/signals/Cargo.toml
fixtures/depgraph/test-support-normal/Cargo.toml
fixtures/depgraph/test-support-normal/core/Cargo.toml
fixtures/depgraph/test-support-normal/signals/Cargo.toml
fixtures/lint/missing-inheritance/Cargo.toml
fixtures/lint/unsafe-forbid/Cargo.toml
fixtures/lint/violation/Cargo.toml
policy/deps.toml
xtask/Cargo.toml
xtask/src/depgraph.rs
xtask/src/lib.rs
xtask/src/lints.rs
xtask/src/main.rs
```

### data/collector-integrity — 7 protected paths

```text
AGENTS.md
Cargo.toml
crates/audit/Cargo.toml
crates/collector/Cargo.toml
crates/core-consumer-ui/Cargo.toml
crates/core/Cargo.toml
tools/manifest_ref/merkle_ref.py
```

## Conflict preview

Read-only `git merge-tree <merge-base> origin/main origin/<branch>` previews below identify conflict-marker paths for tip integration. This is not an actual rebase; commit-by-commit replay can produce additional conflicts. A zero exit from this older merge-tree mode is not a clean-merge verdict.

- `ci/item13-1` (7): `.github/workflows/ci.yml`, `AGENTS.md`, `Cargo.lock`, `Cargo.toml`, `xtask/Cargo.toml`, `xtask/src/lib.rs`, `xtask/src/main.rs`.
- `core/causal-hardening` (13): `AGENTS.md`, `Cargo.lock`, `Cargo.toml`, `crates/core/src/lib.rs`, `crates/execution/src/lib.rs`, `crates/regime/Cargo.toml`, `crates/signals/Cargo.toml`, `crates/signals/src/lib.rs`, `crates/sim/Cargo.toml`, `crates/sim/src/lib.rs`, `xtask/Cargo.toml`, `xtask/src/lib.rs`, `xtask/src/main.rs`.
- `data/collector-integrity` (2): `Cargo.lock`, `Cargo.toml`.

## Validation and handoff

Checks ran against the unchanged main Rust tree, before adding this documentation-only report. No executable behavior changed.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS, exit 0 |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | BLOCKED, exit 101: `camino v1.2.6` absent from cache |
| `cargo test --offline --workspace` | BLOCKED, exit 101: same missing dependency; no tests executed |
| `CARGO_NET_OFFLINE=true cargo xtask spec inspect` | BLOCKED, exit 101: same missing dependency |
| TLC | NOT RUN; no local `specs/tla/tla2tools.jar` |
| Kani | NOT RUN; Kani 0.68.0 / CBMC 6.11.0 available, no proof result claimed |
| cucumber / proptest | NOT RUN; workspace test command stopped during dependency acquisition |
| architecture validate / generate / enforce | NOT RUN; xtask dependency acquisition is blocked |

The initial unqualified `cargo xtask spec inspect` attempted dependency downloads and failed on DNS. All later Cargo validation explicitly disabled network access. No successful network access occurred.

The supplied checkout at `/tmp/cx/ops` has read-only Git metadata; creating a branch there failed. The report is committed in the writable local shared clone `/tmp/cx/ops-triage-work` on `ops/stale-ci-triage-20261023`, based on the main SHA above. Only this Markdown report is changed; `.github/`, `xtask/`, `scripts/`, `policy/`, `AGENTS.md`, `regressions/`, `deny.toml`, `research/ledger-pins/`, `.cargo/`, `tools/`, and all crate manifests remain untouched.

Local handoff command from a writable repository (local filesystem access only):

```bash
git fetch /tmp/cx/ops-triage-work ops/stale-ci-triage-20261023:ops/stale-ci-triage-20261023
```

This report is not a green CI result. No code branch is prepared for landing; the three retained candidates require Seogyu before protected-path integration. Remote deletions and landing remain with the Engineering Lead.
