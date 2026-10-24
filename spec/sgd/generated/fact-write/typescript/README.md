# Generated closed partition data fold

From this generated TypeScript directory, run `npm install` and `npm run build`. The private package pins TypeScript 7.0.2; import its row factories and fold functions from `./dist/machine.js`. Regenerate after changing original models or application sources.

Create authenticated complete rows with `create<Fact>Row(data)`, then call `fold<Fact>({ baseline, incoming })`. Successful results carry the frozen closed handle in `value`. Its single-use `intoReport()` returns a frozen unbound report in `value`. A report is data; it grants no path, writer, resource or persistence permission.

Rows must share one exact symbol and nonnegative UTC-microsecond day index. Arrays must be dense own-data arrays without accessors or extra properties. Forged, copied, cross-Fact or cross-module row tokens are refused. Baseline duplicate keys use the last complete row. Write rows use signed i64 tuple order. Noop and Refused retain the original baseline order and duplicates; Refused retains every complete conflict pair and reports inserted zero. Counts use checked u64 bigint arithmetic.

The row and aggregate authorities stay private to the fold. Close and consume steps come from the admitted complete finite graph. This protects one synchronous trial in a trusted JavaScript realm. It does not authenticate the baseline against disk, establish application-wide coverage, or prove arbitrary collections or persistence.
