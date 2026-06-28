# V9 Falsification Lineage Governance V10 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a paper-only V10 governance loop that seals V8/V9 artifact lineage, validates committed V9 kill-state invariants, and records V9-falsified families as negative controls without generating alpha or changing trading/readiness state.

**Architecture:** Extend the V9 strict falsification module with small governance helpers and a V10 result/writer. Wire one cache-free CLI command through `quantctl`, add focused tests first, then generate a new V10 artifact directory. Existing V8/V9 artifacts are treated as immutable inputs; the new V10 artifact records hashes and post-mortem state instead of editing prior reports.

**Tech Stack:** Python, pandas CSV artifacts, `uv run pytest`, `ruff`, `pyright`, existing `prepare_loop_output_dir` artifact guard.

---

### Task 1: Add Failing Governance Tests

**Files:**
- Modify: `tests/test_multi_asset_strict_falsification.py`

- [ ] **Step 1: Add tests for lineage and governance APIs**

Add imports for the new functions/constants:

```python
from parangsae.research.multi_asset_strict_falsification import (
    V9_FALSIFICATION_GOVERNANCE_REQUIRED_FILES,
    build_artifact_hash_manifest,
    evaluate_v9_falsification_lineage_governance,
    validate_v9_falsification_artifact_invariants,
    write_v9_falsification_lineage_governance_artifacts,
)
```

Add tests that assert:
- hash manifests are deterministic and include `sha256`;
- committed V9 artifact invariants pass and preserve `NO_FAMILY_SURVIVES_STRICT_FALSIFICATION`;
- mutated V9 next queue fails closed;
- V10 governance result records killed families as negative controls and disallowed revival entries;
- V10 writer emits all required files;
- V10 CLI is wired and does not read credentials.

- [ ] **Step 2: Run test to verify RED**

Run:

```bash
uv run pytest tests/test_multi_asset_strict_falsification.py -q
```

Expected: FAIL on missing V10 imports.

### Task 2: Implement V10 Governance Helpers

**Files:**
- Modify: `src/parangsae/research/multi_asset_strict_falsification.py`

- [ ] **Step 1: Add constants and result dataclass**

Add V10 schema, slug, namespace, default V9 artifact path, suite decision values, required files, CSV column constants, and `V9FalsificationGovernanceResult`.

- [ ] **Step 2: Add deterministic hash helpers**

Implement:

```python
def build_artifact_hash_manifest(artifact_dir: str | Path, filenames: Iterable[str], *, source_role: str) -> list[dict[str, Any]]:
    ...
```

Rows include `source_role`, `relative_path`, `sha256`, `byte_count`, and `exists`.

- [ ] **Step 3: Add invariant validator**

Implement:

```python
def validate_v9_falsification_artifact_invariants(v9_artifact_path: str | Path) -> list[dict[str, Any]]:
    ...
```

It validates required V9 files, safety fields, empty next queue when all families killed, no forbidden labels, expected CSV schemas, and V8 source path presence.

- [ ] **Step 4: Add V10 evaluator and writer**

Implement:

```python
def evaluate_v9_falsification_lineage_governance(...): ...
def write_v9_falsification_lineage_governance_artifacts(...): ...
```

The evaluator reads V9 and source V8, computes manifests, builds invariant rows, records negative controls for falsified families, and sets conservative paper-only safety fields.

- [ ] **Step 5: Run targeted test to verify GREEN**

Run:

```bash
uv run pytest tests/test_multi_asset_strict_falsification.py -q
```

Expected: PASS.

### Task 3: Wire CLI

**Files:**
- Modify: `src/parangsae/apps/cli/quantctl.py`
- Modify: `tests/test_multi_asset_strict_falsification.py`

- [ ] **Step 1: Add parser and command**

Add `multi-asset-v9-falsification-lineage-governance-run` with `--source-v9-artifact`, `--source-v8-artifact`, `--reports-root`, `--output-dir`, `--overwrite`, `--command`, and `--changed-file`.

- [ ] **Step 2: Run CLI test**

Run:

```bash
uv run pytest tests/test_multi_asset_strict_falsification.py::test_v5_v6_v7_v8_v9_and_v10_commands_are_wired -q
```

Expected: PASS.

### Task 4: Generate V10 Artifact and Docs

**Files:**
- Modify: `README.md`
- Modify: `docs/backtesting-paper-trading.md`
- Add: `reports/loops/20260628-v9-falsification-lineage-negative-control-governance-v10/`

- [ ] **Step 1: Update docs**

State that V10 is lineage/governance only, freezes V9 kill evidence, does not generate alpha, does not download data, and does not permit strategy/readiness/live/testnet claims.

- [ ] **Step 2: Run V10 CLI**

Run:

```bash
uv run quantctl multi-asset-v9-falsification-lineage-governance-run --overwrite
```

Expected: artifact directory is written with `suite_decision=V9_KILL_STATE_GOVERNANCE_LOCKED`.

### Task 5: Verify, Commit, Push

**Files:**
- All changed files.

- [ ] **Step 1: Run required validation**

Run:

```bash
uv run pytest tests/test_multi_asset_strict_falsification.py -q
uv run pytest tests/test_alpha_signal_factory.py -q
uv run pytest tests/test_public_multi_asset_panel_coverage.py -q
uv run pytest tests/test_tsmom_loop.py -q
uv run pytest -q
uv run ruff check .
uv run --with pyright pyright src/parangsae/research/multi_asset_strict_falsification.py src/parangsae/apps/cli/quantctl.py tests/test_multi_asset_strict_falsification.py
git diff --check
uv run --with pyright pyright src tests
```

Expected: required targeted/full tests, ruff, changed-file Pyright, and diff check pass. Full-repo Pyright may still fail only on pre-existing unrelated debt.

- [ ] **Step 2: Commit and push**

Run:

```bash
git add README.md docs/backtesting-paper-trading.md src/parangsae/apps/cli/quantctl.py src/parangsae/research/multi_asset_strict_falsification.py tests/test_multi_asset_strict_falsification.py reports/loops/20260628-v9-falsification-lineage-negative-control-governance-v10 docs/superpowers/plans/2026-06-28-v9-falsification-lineage-governance-v10.md
git commit -m "Add V9 falsification lineage governance loop"
git push
```

Expected: branch pushes cleanly; no raw data files staged.

## Self-Review

- Spec coverage: covers lineage hashes, committed artifact invariants, CSV schemas, negative-control governance, post-mortem artifact, CLI, docs, validation, and commit/push.
- Placeholder scan: no task relies on TODO/TBD placeholders.
- Boundary check: no alpha generation, no V8/V9 rerun, no data download/backfill, no strategy, no portfolio, no live/testnet/readiness claims.
