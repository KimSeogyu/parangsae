--------------------------- MODULE ArchitecturePolicy ---------------------------
\* Architecture policy for this workspace. This file is the only normative copy.
\*
\* The finite model checks set well-formedness: every referenced module is in
\* Modules, no dependency is a self-edge, each Type.field has one writer, each
\* capability and effect is in its vocabulary, FloatAllowed is a subset of
\* Modules, and each path owner is in Owners. It does not prove that Rust
\* matches a behavioral spec. Generated transition guards are follow-up M1.
\* There is no liveness property. The clock toggles so TLC deadlock checking
\* stays enabled; the clock is not policy state.
\*
\* Encoded so the current tree passes, and recorded here because they are not
\* the intent:
\* parangsae-strategies is in FloatAllowed because the first-batch families
\* compute with f64. parangsae-backtest stays in FloatAllowed: v1a removed
\* sharpe_f64 from the library, and tests and examples still spell f64, so
\* dropping it does not leave enforce clean.
\* parangsae-backtest is allowed process because examples/readpath_parity.rs
\* spawns a command. It does not submit an order.
\* parangsae-core, parangsae-signals, and parangsae-backtest are allowed net
\* because their tokio dev-dependency keeps Cargo default features, and tokio
\* defaults include net. parangsae-execution is allowed net and does not use it.
\* There is no execution order type. Writers are the sim ledger money fields.
\* network_order_submit and holdout_read have no Rust definitions yet. Holdout
\* is a window check in the backtest batch, not a tape-loader function.

EXTENDS FiniteSets

Modules == {
  "parangsae-backtest",
  "parangsae-collector",
  "parangsae-core",
  "parangsae-execution",
  "parangsae-regime",
  "parangsae-signals",
  "parangsae-sim",
  "parangsae-strategies",
  "xtask"
}

Owners == {
  "Backtest Builder",
  "Data Engineer",
  "Execution Builder",
  "Ops/Release",
  "Quant Researcher",
  "Risk/Portfolio",
  "Signal Builder"
}

Effects == {"env", "fs_write", "net", "process"}

Capabilities == {"holdout_read", "network_order_submit"}

FloatAllowed == {"parangsae-backtest", "parangsae-signals", "parangsae-strategies"}

AllowedDependencies == {
  <<"parangsae-backtest", "parangsae-core">>,
  <<"parangsae-backtest", "parangsae-signals">>,
  <<"parangsae-backtest", "parangsae-sim">>,
  <<"parangsae-backtest", "parangsae-strategies">>,
  <<"parangsae-collector", "parangsae-core">>,
  <<"parangsae-signals", "parangsae-core">>,
  <<"parangsae-sim", "parangsae-core">>,
  <<"parangsae-strategies", "parangsae-core">>,
  <<"parangsae-strategies", "parangsae-signals">>
}

AllowedStateWriters == {
  <<"parangsae-sim", "Ledger.cash">>,
  <<"parangsae-sim", "Ledger.fees_paid">>,
  <<"parangsae-sim", "Ledger.funding_net">>,
  <<"parangsae-sim", "Ledger.position">>
}

AllowedCapabilityCallers == {
  <<"parangsae-backtest", "holdout_read">>,
  <<"parangsae-execution", "network_order_submit">>
}

AllowedEffects == {
  <<"parangsae-backtest", "env">>,
  <<"parangsae-backtest", "fs_write">>,
  <<"parangsae-backtest", "net">>,
  <<"parangsae-backtest", "process">>,
  <<"parangsae-collector", "fs_write">>,
  <<"parangsae-collector", "net">>,
  <<"parangsae-collector", "process">>,
  <<"parangsae-core", "net">>,
  <<"parangsae-execution", "net">>,
  <<"parangsae-signals", "env">>,
  <<"parangsae-signals", "fs_write">>,
  <<"parangsae-signals", "net">>,
  <<"xtask", "env">>,
  <<"xtask", "fs_write">>,
  <<"xtask", "process">>
}

PathOwners == {
  <<".cargo/**", "Ops/Release">>,
  <<".cursor/**", "Ops/Release">>,
  <<".env.example", "Ops/Release">>,
  <<".github/**", "Ops/Release">>,
  <<".gitignore", "Ops/Release">>,
  <<"AGENTS.md", "Ops/Release">>,
  <<"Cargo.lock", "Ops/Release">>,
  <<"Cargo.toml", "Ops/Release">>,
  <<"README.md", "Ops/Release">>,
  <<"crates/backtest/**", "Backtest Builder">>,
  <<"crates/collector/**", "Data Engineer">>,
  <<"crates/core/**", "Data Engineer">>,
  <<"crates/execution/**", "Execution Builder">>,
  <<"crates/regime/**", "Signal Builder">>,
  <<"crates/signals/**", "Signal Builder">>,
  <<"crates/strategies/**", "Signal Builder">>,
  <<"crates/sim/**", "Risk/Portfolio">>,
  <<"docs/**", "Quant Researcher">>,
  <<"fixtures/backtest/**", "Backtest Builder">>,
  <<"fixtures/instrument/**", "Data Engineer">>,
  <<"fixtures/vision/**", "Data Engineer">>,
  <<"research/**", "Quant Researcher">>,
  <<"spec/**", "Ops/Release">>,
  <<"specs/**", "Ops/Release">>,
  <<"specs/features/backtest_engine.feature", "Backtest Builder">>,
  <<"specs/features/collector/**", "Data Engineer">>,
  <<"specs/features/instrument_rules.feature", "Data Engineer">>,
  <<"specs/features/kline_integrity.feature", "Data Engineer">>,
  <<"specs/features/signals/**", "Signal Builder">>,
  <<"tests/**", "Ops/Release">>,
  <<"xtask/**", "Ops/Release">>
}

WellFormed ==
  /\ \A dep \in AllowedDependencies : (
       dep[1] \in Modules /\ dep[2] \in Modules /\ dep[1] # dep[2])
  /\ \A w \in AllowedStateWriters : (
       w[1] \in Modules /\ Cardinality({ w2 \in AllowedStateWriters : w2[2] = w[2] }) = 1)
  /\ \A c \in AllowedCapabilityCallers : (
       c[1] \in Modules /\ c[2] \in Capabilities)
  /\ \A e \in AllowedEffects : (
       e[1] \in Modules /\ e[2] \in Effects)
  /\ FloatAllowed \subseteq Modules
  /\ \A p \in PathOwners : (p[2] \in Owners)
  /\ Effects = {"env", "fs_write", "net", "process"}

VARIABLE clock

Init == clock = 0

Next == (clock = 0 /\ clock' = 1) \/ (clock = 1 /\ clock' = 0)

Spec == Init /\ [][Next]_clock

=============================================================================
