---
name: tdd-rust
description: Change parangsae Rust behavior by writing a failing cargo test, proptest, or cucumber scenario first, then making that check pass. Use when fixing a bug or adding a behavior in the Rust crates.
---

# TDD for parangsae Rust

Use this when a crate's observable behavior changes. Do not use it for a comment-only edit.

## Order

1. Name the observable result: a value, an error enum, or a Gherkin `Then`.
2. Add the smallest check that fails on the current code:
   - a `#[test]` or a proptest in the crate that owns the behavior
   - or a scenario in `specs/features/` with a cucumber step in `crates/core/tests/`
3. Run that check and read the failure. If it cannot fail, it does not count.
4. Change the production code until that check passes. Do not weaken the check.
5. Run `cargo test --workspace` for the crates you touched, then `cargo clippy --workspace --all-targets -- -D warnings` when the change is ready.

A test that only records call order, or that asserts a private helper's intermediate state, is worse than no test. Delete it.

`cargo test` is the runner in this repo. Do not add a second harness.

## Boundaries

- Signal tests go through `Causal`. A test that builds a signal input by skipping `causal_at` is a broken test.
- Do not add a network client to `crates/execution` to make a test pass.
- A bug fix that touches live gates, keys, mainnet, CI requirements, branch protection, force-push, or stored data stops for a human.
- Do not read or run the `python-legacy` tag.
