The demo client is an offline protocol codec. Its public API prepares signed
query/cancel data and parses replies. It has no transport, host resolution,
submission method, mutable request fields, or live configuration switch.
The existing sealed `Venue` and refusing `PaperVenue` are unchanged.

The real risk crate, `ApprovedOrder`, and `RiskGate` are absent at base
`973cff653cde011b6fd7a5a58496a8802e2918f3`. New-order market/limit encoding is
therefore unit-test-only. This change does not integrate a submitting adapter
behind `Venue`. That integration remains blocked on #17: it must consume the
real risk-issued approval and check expiry and halt epoch immediately before
dispatch. No replacement approval type is introduced here.

Fixtures are frozen, hand-authored protocol examples and public RFC 4231
vectors, not captured exchange responses. Tests perform no network I/O.
Request fixture signatures were independently calculated with Python's standard
library HMAC/SHA-256. No real signing material was read or stored.

The execution manifest and Cargo.lock changes are required and need review:

| Direct execution dependency | Change | Purpose |
| --- | --- | --- |
| `serde` 1.0.229, derive | Added to runtime dependencies | Typed wire DTOs; rejects missing, wrongly typed, and duplicate known fields |
| `serde_json` 1.0.154 | Promoted from dev to runtime | Offline response decoding |
| `sha2` 0.11.1 | Promoted from dev to runtime | SHA-256 for RFC 2104 HMAC composition |
| `cucumber` 0.22.1 | Added to dev dependencies | Executes the eight Gherkin behavior scenarios |
| `futures` 0.3.34 | Added to dev dependencies | Runs the Gherkin future without a network runtime |

Every package/version was already in the workspace lockfile. The lockfile adds
only execution dependency edges to `serde`, `cucumber`, and `futures`; no package
version or checksum changes. No other protected path is changed.

Reviewer sign-off should cover:

- HMAC block normalization, inner/outer padding, lowercase signature encoding,
  and byte-for-byte agreement with public vectors and frozen request fixtures.
- Canonical fixed-point market/limit examples, stable `(intent, attempt)` IDs,
  and absence of automatic retries or implicit attempt increments.
- Response identity binding; supported one-way MARKET/LIMIT records; decimal,
  cumulative fill, timestamp, missing/duplicate field, and unknown-state checks.
  Parsed cumulative quantity is a snapshot, never an incremental fill event.
- Ambiguous errors remain Unknown; query NotFound is evidence only and does
  not bypass `NotFoundAuthority`; request refusal does not reject an existing
  lifecycle order. Rate limiting and bans confer no retry permission.
- Production exposes neither `Method::Post` nor `DemoClient::submit`; request
  fields are private, debug output redacts signing material, and the existing
  no-transport/source guards remain byte-identical and pass.
- The manifest/dependency justification above and the explicit absence of
  risk-backed submission integration. Reviewer sign-off is still required;
  this note does not claim it has been obtained.

Reproduce the offline execution checks with a compatible cached toolchain:

```sh
cargo test --offline --locked -p parangsae-execution
cargo clippy --offline --locked -p parangsae-execution --all-targets -- -D warnings
cargo fmt --all -- --check
cargo xtask spec generate --check
cargo xtask spec enforce
```
