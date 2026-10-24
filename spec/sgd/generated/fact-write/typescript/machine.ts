// Generated portable closed DTO authority from an audited Rust whole-row schema.
// It does not prove TypeScript application source coverage or persistence.
import { createFactEnvelopeMachine as createPrimitiveAuthority, type FactEnvelopeMachineValue as PrimitiveValue, type FactEnvelopeMachineExpression as PrimitiveExpression, type FactEnvelopeMachineSnapshot as PrimitiveSnapshot, type FactEnvelopeMachineReceipt as PrimitiveReceipt, type FactEnvelopeMachineRejection as PrimitiveRejection } from './primitive.js';
import { createFactEnvelopeMachineControl as createPrivateControl, type FactEnvelopeMachineControl as PrivateControl, type FactEnvelopeMachineControlEvent as ControlEvent, type FactEnvelopeMachineControlNodeId as ControlNode } from './primitive-control.js';
const rowBrand: unique symbol = Symbol('sgd-row');
const utf8 = new TextEncoder();
type WriteEffect = "Clash" | "Insert" | "Same";
function isSourceAction(value: string): value is WriteEffect { return value === "Clash" || value === "Insert" || value === "Same"; }
interface Identity { readonly payload: string; readonly provenance: string }
function unicode(value: string): boolean {
  for (const character of value) { const code = character.codePointAt(0); if (code !== undefined && code >= 0xd800 && code <= 0xdfff) return false; }
  return true;
}
function descriptor(input: object, name: string): unknown {
  const value = Object.getOwnPropertyDescriptor(input, name);
  if (!value || !('value' in value)) throw new TypeError('ROW303: row accessors are outside the checked DTO profile');
  return value.value;
}
function part(bytes: Uint8Array): string { return bytes.length.toString() + ':' + [...bytes].map(byte => byte.toString(16).padStart(2, '0')).join('') + ';'; }
function header(row: string, flavor: string): string { return part(utf8.encode('sgd-row-v1')) + part(utf8.encode(row)) + part(utf8.encode(flavor)); }
function integer64(value: bigint): Uint8Array { const bytes = new Uint8Array(8); new DataView(bytes.buffer).setBigInt64(0, value, false); return bytes; }
function integer32(value: number): Uint8Array { const bytes = new Uint8Array(4); new DataView(bytes.buffer).setUint32(0, value, false); return bytes; }
function same(left: PrimitiveValue | undefined, right: PrimitiveValue): boolean { return left !== undefined && left.kind === right.kind && left.value === right.value && (left.kind !== 'data' || right.kind === 'data' && left.domain === right.domain); }
function valueAt(snapshot: PrimitiveSnapshot, variable: string): PrimitiveValue | undefined {
  switch (variable) {
    case "phase": return snapshot.values["phase"];
    case "rejected": return snapshot.values["rejected"];
    case "rejectedProvenance": return snapshot.values["rejectedProvenance"];
    case "stored": return snapshot.values["stored"];
    case "storedProvenance": return snapshot.values["storedProvenance"];
    default: return undefined;
  }
}
function matches(row: Identity | null, snapshot: PrimitiveSnapshot, payload: string, provenance: string, emptyPayload: PrimitiveValue, emptyProvenance: PrimitiveValue): boolean {
  const payloadValue: PrimitiveValue = row === null ? emptyPayload : { kind: 'data', domain: "Values", value: row.payload };
  const provenanceValue: PrimitiveValue = row === null ? emptyProvenance : { kind: 'data', domain: "Receipts", value: row.provenance };
  return same(valueAt(snapshot, payload), payloadValue) && same(valueAt(snapshot, provenance), provenanceValue);
}
function lift<T extends Identity>(receipt: PrimitiveReceipt, payload: string, provenance: string, incoming: T, oldStored: T | null, oldRejected: T | null, emptyPayload: PrimitiveValue, emptyProvenance: PrimitiveValue): { readonly ok: true; readonly row: T | null } | { readonly ok: false } {
  const a: PrimitiveExpression | undefined = receipt.updates[payload];
  const b: PrimitiveExpression | undefined = receipt.updates[provenance];
  if (a?.kind === 'literal' && b?.kind === 'literal' && same(a.value, emptyPayload) && same(b.value, emptyProvenance)) return { ok: true, row: null };
  if (a?.kind === 'input' && b?.kind === 'input' && a.parameter === "v" && b.parameter === "provenance") return { ok: true, row: incoming };
  if (a?.kind === 'state' && b?.kind === 'state' && a.variable === "stored" && b.variable === "storedProvenance") return { ok: true, row: oldStored };
  if (a?.kind === 'state' && b?.kind === 'state' && a.variable === "rejected" && b.variable === "rejectedProvenance") return { ok: true, row: oldRejected };
  return { ok: false };
}

const closedBrand: unique symbol = Symbol('sgd-closed-partition');
const maxCount = 18446744073709551615n;
export interface PartitionCounts { readonly inserted: bigint; readonly unchanged: bigint }
export interface PartitionGroup { readonly symbol: string; readonly dayIndex: bigint }
export type PartitionFailureReason = 'invalid-batch' | 'invalid-row' | 'invalid-calendar' | 'wrong-group' | 'broken-binding' | 'row-rejected' | 'controller-rejected' | 'count-overflow' | 'invalid-closed' | 'already-consumed';
export type PartitionResult<T> = { readonly ok: true; readonly value: T } | { readonly ok: false; readonly reason: PartitionFailureReason };
function failed(reason: PartitionFailureReason): { readonly ok: false; readonly reason: PartitionFailureReason } { return Object.freeze({ ok: false, reason }); }
function succeeded<T>(value: T): { readonly ok: true; readonly value: T } { return Object.freeze({ ok: true, value }); }
class PartitionBoundaryError extends Error {
  constructor(readonly reason: 'invalid-batch' | 'invalid-row') { super(reason); }
}
function snapshotBatch(input: unknown): { readonly baseline: unknown; readonly incoming: unknown } {
  if (typeof input !== 'object' || input === null || Array.isArray(input)) throw new PartitionBoundaryError('invalid-batch');
  const keys = Reflect.ownKeys(input);
  if (keys.length !== 2 || !keys.includes('baseline') || !keys.includes('incoming')) throw new PartitionBoundaryError('invalid-batch');
  const baseline = Object.getOwnPropertyDescriptor(input, 'baseline');
  const incoming = Object.getOwnPropertyDescriptor(input, 'incoming');
  if (baseline === undefined || incoming === undefined || !('value' in baseline) || !('value' in incoming)) throw new PartitionBoundaryError('invalid-batch');
  return { baseline: baseline.value, incoming: incoming.value };
}
function snapshotArray<T>(input: unknown, authenticate: (value: unknown) => T | undefined): readonly T[] {
  if (!Array.isArray(input)) throw new PartitionBoundaryError('invalid-batch');
  const descriptor = Object.getOwnPropertyDescriptor(input, 'length');
  const length: unknown = descriptor?.value;
  if (typeof length !== 'number' || !Number.isInteger(length) || length < 0 || length > 4294967295) throw new PartitionBoundaryError('invalid-batch');
  const keys = Reflect.ownKeys(input);
  if (keys.length !== length + 1 || keys.some(key => key !== 'length' && (typeof key !== 'string' || !/^(0|[1-9][0-9]*)$/.test(key) || Number(key) >= length))) throw new PartitionBoundaryError('invalid-batch');
  const copy: T[] = [];
  for (let index = 0; index < length; index += 1) {
    const item = Object.getOwnPropertyDescriptor(input, index.toString());
    if (item === undefined || !('value' in item)) throw new PartitionBoundaryError('invalid-batch');
    const row = authenticate(item.value);
    if (row === undefined) throw new PartitionBoundaryError('invalid-row');
    copy.push(row);
  }
  return Object.freeze(copy);
}
function keyIdentity(key: readonly bigint[]): string { return key.map(value => part(integer64(value))).join(''); }
function compareKeys(left: readonly bigint[], right: readonly bigint[]): number {
  for (let index = 0; index < left.length; index += 1) {
    const a = left[index]; const b = right[index];
    if (a === undefined || b === undefined) throw new Error('Corrupt generated partition key');
    if (a < b) return -1;
    if (a > b) return 1;
  }
  return 0;
}
const closeSteps = new Map<ControlNode, { readonly event: ControlEvent; readonly next: ControlNode; readonly kind: 'Write' | 'Noop' | 'Refused' }>([
  ["s1", { event: "CloseNoop", next: "s2", kind: "Noop" }],
  ["s4", { event: "CloseWrite", next: "s5", kind: "Write" }],
  ["s7", { event: "CloseRefused", next: "s8", kind: "Refused" }],
  ["s10", { event: "CloseRefused", next: "s11", kind: "Refused" }],
]);
const consumeSteps = new Map<ControlNode, { readonly event: ControlEvent; readonly next: ControlNode }>([
  ["s11", { event: "ConsumeRefused", next: "s9" }],
  ["s2", { event: "ConsumeNoop", next: "s0" }],
  ["s5", { event: "ConsumeWrite", next: "s3" }],
  ["s8", { event: "ConsumeRefused", next: "s6" }],
]);

export interface KlineFactRow {
  readonly [rowBrand]: "KlineFact";
  readonly "symbol": string;
  readonly "interval_us": bigint;
  readonly "open_time": bigint;
  readonly "close_time": bigint;
  readonly "open_ticks": bigint;
  readonly "high_ticks": bigint;
  readonly "low_ticks": bigint;
  readonly "close_ticks": bigint;
  readonly "volume": bigint;
  readonly "quote_volume": bigint;
  readonly "count": bigint;
  readonly "taker_buy_volume": bigint;
  readonly "taker_buy_quote_volume": bigint;
  readonly "event_ts": bigint;
  readonly "available_ts": bigint;
  readonly "received_ts": bigint;
  readonly "schema_version": number;
  readonly "tick_domain": "Price" | "Ratio";
}
interface KlineFactIdentity { readonly row: KlineFactRow; readonly payload: string; readonly provenance: string }
const rows0 = new WeakMap<object, KlineFactIdentity>();
function identifyKlineFact(row: KlineFactRow): KlineFactIdentity {
  let payload = header("KlineFact", 'payload');
  let provenance = header("KlineFact", 'provenance');
  payload += part(utf8.encode("symbol")) + part(utf8.encode("string")) + part(utf8.encode(row["symbol"]));
  payload += part(utf8.encode("interval_us")) + part(utf8.encode("i64")) + part(integer64(row["interval_us"]));
  payload += part(utf8.encode("open_time")) + part(utf8.encode("i64")) + part(integer64(row["open_time"]));
  payload += part(utf8.encode("close_time")) + part(utf8.encode("i64")) + part(integer64(row["close_time"]));
  payload += part(utf8.encode("open_ticks")) + part(utf8.encode("i64")) + part(integer64(row["open_ticks"]));
  payload += part(utf8.encode("high_ticks")) + part(utf8.encode("i64")) + part(integer64(row["high_ticks"]));
  payload += part(utf8.encode("low_ticks")) + part(utf8.encode("i64")) + part(integer64(row["low_ticks"]));
  payload += part(utf8.encode("close_ticks")) + part(utf8.encode("i64")) + part(integer64(row["close_ticks"]));
  payload += part(utf8.encode("volume")) + part(utf8.encode("i64")) + part(integer64(row["volume"]));
  payload += part(utf8.encode("quote_volume")) + part(utf8.encode("i64")) + part(integer64(row["quote_volume"]));
  payload += part(utf8.encode("count")) + part(utf8.encode("i64")) + part(integer64(row["count"]));
  payload += part(utf8.encode("taker_buy_volume")) + part(utf8.encode("i64")) + part(integer64(row["taker_buy_volume"]));
  payload += part(utf8.encode("taker_buy_quote_volume")) + part(utf8.encode("i64")) + part(integer64(row["taker_buy_quote_volume"]));
  payload += part(utf8.encode("event_ts")) + part(utf8.encode("i64")) + part(integer64(row["event_ts"]));
  payload += part(utf8.encode("available_ts")) + part(utf8.encode("i64")) + part(integer64(row["available_ts"]));
  payload += part(utf8.encode("schema_version")) + part(utf8.encode("u32")) + part(integer32((row["schema_version"] === 1 || row["schema_version"] === 2 ? 1 : row["schema_version"])));
  payload += part(utf8.encode("tick_domain")) + part(utf8.encode("enum")) + part(utf8.encode(row["tick_domain"]));
  provenance += part(utf8.encode("received_ts")) + part(utf8.encode("i64")) + part(integer64(row["received_ts"]));
  provenance += part(utf8.encode("schema_version")) + part(utf8.encode("u32")) + part(integer32(row["schema_version"]));
  return Object.freeze({ row, payload, provenance });
}
/** Parse and copy the complete row before it can enter a live owner. Accessors are refused. */
export function createKlineFactRow(input: unknown): KlineFactRow {
  if (typeof input !== 'object' || input === null || Array.isArray(input)) throw new TypeError('ROW303: expected a complete data object');
  const keys = Reflect.ownKeys(input);
  if (keys.length !== 18 || keys.some(key => typeof key !== 'string' || !["symbol","interval_us","open_time","close_time","open_ticks","high_ticks","low_ticks","close_ticks","volume","quote_volume","count","taker_buy_volume","taker_buy_quote_volume","event_ts","available_ts","received_ts","schema_version","tick_domain"].includes(key))) throw new TypeError('ROW303: row fields do not match the checked schema');
  const field0: unknown = descriptor(input, "symbol");
  if (typeof field0 !== 'string' || !unicode(field0)) throw new TypeError("ROW303: KlineFact.symbol requires string");
  const field1: unknown = descriptor(input, "interval_us");
  if (typeof field1 !== 'bigint' || field1 < -9223372036854775808n || field1 > 9223372036854775807n) throw new TypeError("ROW303: KlineFact.interval_us requires i64");
  const field2: unknown = descriptor(input, "open_time");
  if (typeof field2 !== 'bigint' || field2 < -9223372036854775808n || field2 > 9223372036854775807n) throw new TypeError("ROW303: KlineFact.open_time requires i64");
  const field3: unknown = descriptor(input, "close_time");
  if (typeof field3 !== 'bigint' || field3 < -9223372036854775808n || field3 > 9223372036854775807n) throw new TypeError("ROW303: KlineFact.close_time requires i64");
  const field4: unknown = descriptor(input, "open_ticks");
  if (typeof field4 !== 'bigint' || field4 < -9223372036854775808n || field4 > 9223372036854775807n) throw new TypeError("ROW303: KlineFact.open_ticks requires i64");
  const field5: unknown = descriptor(input, "high_ticks");
  if (typeof field5 !== 'bigint' || field5 < -9223372036854775808n || field5 > 9223372036854775807n) throw new TypeError("ROW303: KlineFact.high_ticks requires i64");
  const field6: unknown = descriptor(input, "low_ticks");
  if (typeof field6 !== 'bigint' || field6 < -9223372036854775808n || field6 > 9223372036854775807n) throw new TypeError("ROW303: KlineFact.low_ticks requires i64");
  const field7: unknown = descriptor(input, "close_ticks");
  if (typeof field7 !== 'bigint' || field7 < -9223372036854775808n || field7 > 9223372036854775807n) throw new TypeError("ROW303: KlineFact.close_ticks requires i64");
  const field8: unknown = descriptor(input, "volume");
  if (typeof field8 !== 'bigint' || field8 < -9223372036854775808n || field8 > 9223372036854775807n) throw new TypeError("ROW303: KlineFact.volume requires i64");
  const field9: unknown = descriptor(input, "quote_volume");
  if (typeof field9 !== 'bigint' || field9 < -9223372036854775808n || field9 > 9223372036854775807n) throw new TypeError("ROW303: KlineFact.quote_volume requires i64");
  const field10: unknown = descriptor(input, "count");
  if (typeof field10 !== 'bigint' || field10 < -9223372036854775808n || field10 > 9223372036854775807n) throw new TypeError("ROW303: KlineFact.count requires i64");
  const field11: unknown = descriptor(input, "taker_buy_volume");
  if (typeof field11 !== 'bigint' || field11 < -9223372036854775808n || field11 > 9223372036854775807n) throw new TypeError("ROW303: KlineFact.taker_buy_volume requires i64");
  const field12: unknown = descriptor(input, "taker_buy_quote_volume");
  if (typeof field12 !== 'bigint' || field12 < -9223372036854775808n || field12 > 9223372036854775807n) throw new TypeError("ROW303: KlineFact.taker_buy_quote_volume requires i64");
  const field13: unknown = descriptor(input, "event_ts");
  if (typeof field13 !== 'bigint' || field13 < -9223372036854775808n || field13 > 9223372036854775807n) throw new TypeError("ROW303: KlineFact.event_ts requires i64");
  const field14: unknown = descriptor(input, "available_ts");
  if (typeof field14 !== 'bigint' || field14 < -9223372036854775808n || field14 > 9223372036854775807n) throw new TypeError("ROW303: KlineFact.available_ts requires i64");
  const field15: unknown = descriptor(input, "received_ts");
  if (typeof field15 !== 'bigint' || field15 < -9223372036854775808n || field15 > 9223372036854775807n) throw new TypeError("ROW303: KlineFact.received_ts requires i64");
  const field16: unknown = descriptor(input, "schema_version");
  if (typeof field16 !== 'number' || !Number.isInteger(field16) || field16 < 0 || field16 > 4294967295) throw new TypeError("ROW303: KlineFact.schema_version requires u32");
  const field17: unknown = descriptor(input, "tick_domain");
  if (field17 !== "Price" && field17 !== "Ratio") throw new TypeError("ROW303: KlineFact.tick_domain requires enum");
  const row: KlineFactRow = Object.freeze<KlineFactRow>({ [rowBrand]: "KlineFact", "symbol": field0, "interval_us": field1, "open_time": field2, "close_time": field3, "open_ticks": field4, "high_ticks": field5, "low_ticks": field6, "close_ticks": field7, "volume": field8, "quote_volume": field9, "count": field10, "taker_buy_volume": field11, "taker_buy_quote_volume": field12, "event_ts": field13, "available_ts": field14, "received_ts": field15, "schema_version": field16, "tick_domain": field17 });
  rows0.set(row, identifyKlineFact(row));
  return row;
}
interface KlineFactObservation {
  readonly stored: KlineFactRow | null;
  readonly rejected: KlineFactRow | null;
  readonly state: PrimitiveSnapshot;
}
type KlineFactWriteResult =
  | { readonly ok: true; readonly effect: WriteEffect; readonly receipt: PrimitiveReceipt }
  | { readonly ok: false; readonly reason: 'invalid-row' | 'broken-binding' | PrimitiveRejection['reason']; readonly state: PrimitiveSnapshot };
interface KlineFactOwner {
  readonly read: () => KlineFactObservation;
  readonly write: (row: KlineFactRow) => KlineFactWriteResult;
}
/** The authenticated token binds payload and provenance from the same complete immutable DTO. */
function createKlineFactOwner(): KlineFactOwner {
  const authority = createPrimitiveAuthority();
  let stored: KlineFactIdentity | null = null;
  let rejected: KlineFactIdentity | null = null;
  const read = Object.freeze((): KlineFactObservation => Object.freeze({ stored: stored?.row ?? null, rejected: rejected?.row ?? null, state: authority.read() }));
  const reject = (reason: 'invalid-row' | 'broken-binding'): KlineFactWriteResult => Object.freeze({ ok: false, reason, state: authority.read() });
  const write = Object.freeze((row: KlineFactRow): KlineFactWriteResult => {
    if (typeof row !== 'object' || row === null) return reject('invalid-row');
    const incoming = rows0.get(row);
    if (incoming === undefined) return reject('invalid-row');
    const before = authority.read();
    if (!matches(stored, before, "stored", "storedProvenance", {"kind":"string","value":"none"}, {"kind":"string","value":"none"}) || !matches(rejected, before, "rejected", "rejectedProvenance", {"kind":"string","value":"none"}, {"kind":"string","value":"none"})) return reject('broken-binding');
    const pending = authority.prepare(incoming.payload, incoming.provenance);
    if (!pending.ok) return pending;
    const nextStored = lift(pending.receipt, "stored", "storedProvenance", incoming, stored, rejected, {"kind":"string","value":"none"}, {"kind":"string","value":"none"});
    const nextRejected = lift(pending.receipt, "rejected", "rejectedProvenance", incoming, stored, rejected, {"kind":"string","value":"none"}, {"kind":"string","value":"none"});
    if (!nextStored.ok || !nextRejected.ok) return reject('broken-binding');
    if (!matches(nextStored.row, pending.receipt.after, "stored", "storedProvenance", {"kind":"string","value":"none"}, {"kind":"string","value":"none"}) || !matches(nextRejected.row, pending.receipt.after, "rejected", "rejectedProvenance", {"kind":"string","value":"none"}, {"kind":"string","value":"none"})) return reject('broken-binding');
    const effect = pending.receipt.action;
    if (!isSourceAction(effect)) return reject('broken-binding');
    // Authenticated frozen rows and primitive identities run no application code here.
    const committed = pending.commit();
    if (!committed.ok) return committed;
    stored = nextStored.row;
    rejected = nextRejected.row;
    return Object.freeze({ ok: true, effect, receipt: pending.receipt });
  });
  return Object.freeze({ read, write });
}

export type KlineFactPartitionReport =
  | { readonly kind: 'Write' | 'Noop'; readonly rows: readonly KlineFactRow[]; readonly counts: PartitionCounts; readonly group: PartitionGroup | null }
  | { readonly kind: 'Refused'; readonly original: readonly KlineFactRow[]; readonly conflicts: readonly KlineFactConflict[]; readonly counts: PartitionCounts; readonly group: PartitionGroup | null };
export interface KlineFactConflict { readonly stored: KlineFactRow; readonly rejected: KlineFactRow }
export interface KlineFactClosedPartition {
  readonly [closedBrand]: "KlineFact";
  readonly counts: PartitionCounts;
  readonly group: PartitionGroup | null;
  readonly intoReport: (this: KlineFactClosedPartition) => PartitionResult<KlineFactPartitionReport>;
}
interface KlineFactClosedState {
  readonly controller: PrivateControl;
  readonly report: KlineFactPartitionReport;
  consumed: boolean;
}
const closed0 = new WeakMap<object, KlineFactClosedState>();
function keyKlineFact(row: KlineFactRow): readonly bigint[] { return [row["open_time"]]; }
function authenticateKlineFact(value: unknown): KlineFactRow | undefined {
  return typeof value === 'object' && value !== null ? rows0.get(value)?.row : undefined;
}
/** Complete authenticated rows enter one private trial; this function grants no filesystem authority. */
export function foldKlineFact(input: { readonly baseline: readonly KlineFactRow[]; readonly incoming: readonly KlineFactRow[] }): PartitionResult<KlineFactClosedPartition> {
  let baseline: readonly KlineFactRow[];
  let incoming: readonly KlineFactRow[];
  try {
    const batch = snapshotBatch(input);
    baseline = snapshotArray(batch.baseline, authenticateKlineFact);
    incoming = snapshotArray(batch.incoming, authenticateKlineFact);
  } catch (error) { return failed(error instanceof PartitionBoundaryError ? error.reason : 'invalid-batch'); }
  let group: PartitionGroup | null = null;
  for (const row of [...baseline, ...incoming]) {
    const time = row["open_time"];
    if (time < 0n) return failed('invalid-calendar');
    const observed = Object.freeze({ symbol: row["symbol"], dayIndex: time / 86_400_000_000n });
    if (group === null) group = observed;
    else if (group.symbol !== observed.symbol || group.dayIndex !== observed.dayIndex) return failed('wrong-group');
  }
  const controller = createPrivateControl();
  const owners = new Map<string, { readonly key: readonly bigint[]; readonly owner: KlineFactOwner }>();
  // Baseline replay is private and uncounted. A later duplicate baseline key replaces its owner.
  for (const row of baseline) {
    const owner = createKlineFactOwner();
    const result = owner.write(row);
    if (!result.ok || result.effect !== 'Insert') return failed('row-rejected');
    const key = keyKlineFact(row);
    owners.set(keyIdentity(key), { key, owner });
  }
  let inserted = 0n;
  let unchanged = 0n;
  const conflicts: KlineFactConflict[] = [];
  for (const row of incoming) {
    const key = keyKlineFact(row);
    const identity = keyIdentity(key);
    let entry = owners.get(identity);
    if (entry === undefined) { entry = { key, owner: createKlineFactOwner() }; owners.set(identity, entry); }
    const before = entry.owner.read().stored;
    const result = entry.owner.write(row);
    if (!result.ok) return failed('row-rejected');
    const observed = controller.send(result.effect);
    if (!observed.ok) return failed('controller-rejected');
    switch (result.effect) {
      case 'Insert': if (inserted === maxCount) return failed('count-overflow'); inserted += 1n; break;
      case 'Same': if (unchanged === maxCount) return failed('count-overflow'); unchanged += 1n; break;
      case 'Clash':
        if (before === null) return failed('broken-binding');
        conflicts.push(Object.freeze({ stored: before, rejected: row }));
        break;
    }
  }
  const step = closeSteps.get(controller.read().nodeId);
  if (step === undefined) return failed('broken-binding');
  const closure = controller.send(step.event);
  if (!closure.ok || closure.after.nodeId !== step.next) return failed('controller-rejected');
  const counts = Object.freeze({ inserted: step.kind === 'Refused' ? 0n : inserted, unchanged });
  let report: KlineFactPartitionReport;
  switch (step.kind) {
    case 'Write': {
      const rows: KlineFactRow[] = [];
      for (const entry of [...owners.values()].sort((left, right) => compareKeys(left.key, right.key))) {
        const row = entry.owner.read().stored;
        if (row === null) return failed('broken-binding');
        rows.push(row);
      }
      report = Object.freeze({ kind: 'Write', rows: Object.freeze(rows), counts, group });
      break;
    }
    case 'Noop': report = Object.freeze({ kind: 'Noop', rows: baseline, counts, group }); break;
    case 'Refused': report = Object.freeze({ kind: 'Refused', original: baseline, conflicts: Object.freeze(conflicts), counts, group }); break;
  }
  const intoReport = Object.freeze(function(this: unknown): PartitionResult<KlineFactPartitionReport> {
    if (typeof this !== 'object' || this === null) return failed('invalid-closed');
    const state = closed0.get(this);
    if (state === undefined) return failed('invalid-closed');
    if (state.consumed) return failed('already-consumed');
    const consume = consumeSteps.get(state.controller.read().nodeId);
    if (consume === undefined) return failed('broken-binding');
    const result = state.controller.send(consume.event);
    if (!result.ok || result.after.nodeId !== consume.next) return failed('controller-rejected');
    state.consumed = true;
    return succeeded(state.report);
  });
  const closed: KlineFactClosedPartition = Object.freeze<KlineFactClosedPartition>({ [closedBrand]: "KlineFact", counts, group, intoReport });
  closed0.set(closed, { controller, report, consumed: false });
  return succeeded(closed);
}


export interface FundingFactRow {
  readonly [rowBrand]: "FundingFact";
  readonly "symbol": string;
  readonly "raw_calc_time": bigint;
  readonly "funding_time": bigint;
  readonly "interval_hours": bigint;
  readonly "rate": bigint;
  readonly "event_ts": bigint;
  readonly "available_ts": bigint;
  readonly "received_ts": bigint;
  readonly "schema_version": number;
}
interface FundingFactIdentity { readonly row: FundingFactRow; readonly payload: string; readonly provenance: string }
const rows1 = new WeakMap<object, FundingFactIdentity>();
function identifyFundingFact(row: FundingFactRow): FundingFactIdentity {
  let payload = header("FundingFact", 'payload');
  let provenance = header("FundingFact", 'provenance');
  payload += part(utf8.encode("symbol")) + part(utf8.encode("string")) + part(utf8.encode(row["symbol"]));
  payload += part(utf8.encode("raw_calc_time")) + part(utf8.encode("i64")) + part(integer64(row["raw_calc_time"]));
  payload += part(utf8.encode("funding_time")) + part(utf8.encode("i64")) + part(integer64(row["funding_time"]));
  payload += part(utf8.encode("interval_hours")) + part(utf8.encode("i64")) + part(integer64(row["interval_hours"]));
  payload += part(utf8.encode("rate")) + part(utf8.encode("i64")) + part(integer64(row["rate"]));
  payload += part(utf8.encode("event_ts")) + part(utf8.encode("i64")) + part(integer64(row["event_ts"]));
  payload += part(utf8.encode("available_ts")) + part(utf8.encode("i64")) + part(integer64(row["available_ts"]));
  payload += part(utf8.encode("schema_version")) + part(utf8.encode("u32")) + part(integer32(row["schema_version"]));
  provenance += part(utf8.encode("received_ts")) + part(utf8.encode("i64")) + part(integer64(row["received_ts"]));
  return Object.freeze({ row, payload, provenance });
}
/** Parse and copy the complete row before it can enter a live owner. Accessors are refused. */
export function createFundingFactRow(input: unknown): FundingFactRow {
  if (typeof input !== 'object' || input === null || Array.isArray(input)) throw new TypeError('ROW303: expected a complete data object');
  const keys = Reflect.ownKeys(input);
  if (keys.length !== 9 || keys.some(key => typeof key !== 'string' || !["symbol","raw_calc_time","funding_time","interval_hours","rate","event_ts","available_ts","received_ts","schema_version"].includes(key))) throw new TypeError('ROW303: row fields do not match the checked schema');
  const field0: unknown = descriptor(input, "symbol");
  if (typeof field0 !== 'string' || !unicode(field0)) throw new TypeError("ROW303: FundingFact.symbol requires string");
  const field1: unknown = descriptor(input, "raw_calc_time");
  if (typeof field1 !== 'bigint' || field1 < -9223372036854775808n || field1 > 9223372036854775807n) throw new TypeError("ROW303: FundingFact.raw_calc_time requires i64");
  const field2: unknown = descriptor(input, "funding_time");
  if (typeof field2 !== 'bigint' || field2 < -9223372036854775808n || field2 > 9223372036854775807n) throw new TypeError("ROW303: FundingFact.funding_time requires i64");
  const field3: unknown = descriptor(input, "interval_hours");
  if (typeof field3 !== 'bigint' || field3 < -9223372036854775808n || field3 > 9223372036854775807n) throw new TypeError("ROW303: FundingFact.interval_hours requires i64");
  const field4: unknown = descriptor(input, "rate");
  if (typeof field4 !== 'bigint' || field4 < -9223372036854775808n || field4 > 9223372036854775807n) throw new TypeError("ROW303: FundingFact.rate requires i64");
  const field5: unknown = descriptor(input, "event_ts");
  if (typeof field5 !== 'bigint' || field5 < -9223372036854775808n || field5 > 9223372036854775807n) throw new TypeError("ROW303: FundingFact.event_ts requires i64");
  const field6: unknown = descriptor(input, "available_ts");
  if (typeof field6 !== 'bigint' || field6 < -9223372036854775808n || field6 > 9223372036854775807n) throw new TypeError("ROW303: FundingFact.available_ts requires i64");
  const field7: unknown = descriptor(input, "received_ts");
  if (typeof field7 !== 'bigint' || field7 < -9223372036854775808n || field7 > 9223372036854775807n) throw new TypeError("ROW303: FundingFact.received_ts requires i64");
  const field8: unknown = descriptor(input, "schema_version");
  if (typeof field8 !== 'number' || !Number.isInteger(field8) || field8 < 0 || field8 > 4294967295) throw new TypeError("ROW303: FundingFact.schema_version requires u32");
  const row: FundingFactRow = Object.freeze<FundingFactRow>({ [rowBrand]: "FundingFact", "symbol": field0, "raw_calc_time": field1, "funding_time": field2, "interval_hours": field3, "rate": field4, "event_ts": field5, "available_ts": field6, "received_ts": field7, "schema_version": field8 });
  rows1.set(row, identifyFundingFact(row));
  return row;
}
interface FundingFactObservation {
  readonly stored: FundingFactRow | null;
  readonly rejected: FundingFactRow | null;
  readonly state: PrimitiveSnapshot;
}
type FundingFactWriteResult =
  | { readonly ok: true; readonly effect: WriteEffect; readonly receipt: PrimitiveReceipt }
  | { readonly ok: false; readonly reason: 'invalid-row' | 'broken-binding' | PrimitiveRejection['reason']; readonly state: PrimitiveSnapshot };
interface FundingFactOwner {
  readonly read: () => FundingFactObservation;
  readonly write: (row: FundingFactRow) => FundingFactWriteResult;
}
/** The authenticated token binds payload and provenance from the same complete immutable DTO. */
function createFundingFactOwner(): FundingFactOwner {
  const authority = createPrimitiveAuthority();
  let stored: FundingFactIdentity | null = null;
  let rejected: FundingFactIdentity | null = null;
  const read = Object.freeze((): FundingFactObservation => Object.freeze({ stored: stored?.row ?? null, rejected: rejected?.row ?? null, state: authority.read() }));
  const reject = (reason: 'invalid-row' | 'broken-binding'): FundingFactWriteResult => Object.freeze({ ok: false, reason, state: authority.read() });
  const write = Object.freeze((row: FundingFactRow): FundingFactWriteResult => {
    if (typeof row !== 'object' || row === null) return reject('invalid-row');
    const incoming = rows1.get(row);
    if (incoming === undefined) return reject('invalid-row');
    const before = authority.read();
    if (!matches(stored, before, "stored", "storedProvenance", {"kind":"string","value":"none"}, {"kind":"string","value":"none"}) || !matches(rejected, before, "rejected", "rejectedProvenance", {"kind":"string","value":"none"}, {"kind":"string","value":"none"})) return reject('broken-binding');
    const pending = authority.prepare(incoming.payload, incoming.provenance);
    if (!pending.ok) return pending;
    const nextStored = lift(pending.receipt, "stored", "storedProvenance", incoming, stored, rejected, {"kind":"string","value":"none"}, {"kind":"string","value":"none"});
    const nextRejected = lift(pending.receipt, "rejected", "rejectedProvenance", incoming, stored, rejected, {"kind":"string","value":"none"}, {"kind":"string","value":"none"});
    if (!nextStored.ok || !nextRejected.ok) return reject('broken-binding');
    if (!matches(nextStored.row, pending.receipt.after, "stored", "storedProvenance", {"kind":"string","value":"none"}, {"kind":"string","value":"none"}) || !matches(nextRejected.row, pending.receipt.after, "rejected", "rejectedProvenance", {"kind":"string","value":"none"}, {"kind":"string","value":"none"})) return reject('broken-binding');
    const effect = pending.receipt.action;
    if (!isSourceAction(effect)) return reject('broken-binding');
    // Authenticated frozen rows and primitive identities run no application code here.
    const committed = pending.commit();
    if (!committed.ok) return committed;
    stored = nextStored.row;
    rejected = nextRejected.row;
    return Object.freeze({ ok: true, effect, receipt: pending.receipt });
  });
  return Object.freeze({ read, write });
}

export type FundingFactPartitionReport =
  | { readonly kind: 'Write' | 'Noop'; readonly rows: readonly FundingFactRow[]; readonly counts: PartitionCounts; readonly group: PartitionGroup | null }
  | { readonly kind: 'Refused'; readonly original: readonly FundingFactRow[]; readonly conflicts: readonly FundingFactConflict[]; readonly counts: PartitionCounts; readonly group: PartitionGroup | null };
export interface FundingFactConflict { readonly stored: FundingFactRow; readonly rejected: FundingFactRow }
export interface FundingFactClosedPartition {
  readonly [closedBrand]: "FundingFact";
  readonly counts: PartitionCounts;
  readonly group: PartitionGroup | null;
  readonly intoReport: (this: FundingFactClosedPartition) => PartitionResult<FundingFactPartitionReport>;
}
interface FundingFactClosedState {
  readonly controller: PrivateControl;
  readonly report: FundingFactPartitionReport;
  consumed: boolean;
}
const closed1 = new WeakMap<object, FundingFactClosedState>();
function keyFundingFact(row: FundingFactRow): readonly bigint[] { return [row["funding_time"]]; }
function authenticateFundingFact(value: unknown): FundingFactRow | undefined {
  return typeof value === 'object' && value !== null ? rows1.get(value)?.row : undefined;
}
/** Complete authenticated rows enter one private trial; this function grants no filesystem authority. */
export function foldFundingFact(input: { readonly baseline: readonly FundingFactRow[]; readonly incoming: readonly FundingFactRow[] }): PartitionResult<FundingFactClosedPartition> {
  let baseline: readonly FundingFactRow[];
  let incoming: readonly FundingFactRow[];
  try {
    const batch = snapshotBatch(input);
    baseline = snapshotArray(batch.baseline, authenticateFundingFact);
    incoming = snapshotArray(batch.incoming, authenticateFundingFact);
  } catch (error) { return failed(error instanceof PartitionBoundaryError ? error.reason : 'invalid-batch'); }
  let group: PartitionGroup | null = null;
  for (const row of [...baseline, ...incoming]) {
    const time = row["funding_time"];
    if (time < 0n) return failed('invalid-calendar');
    const observed = Object.freeze({ symbol: row["symbol"], dayIndex: time / 86_400_000_000n });
    if (group === null) group = observed;
    else if (group.symbol !== observed.symbol || group.dayIndex !== observed.dayIndex) return failed('wrong-group');
  }
  const controller = createPrivateControl();
  const owners = new Map<string, { readonly key: readonly bigint[]; readonly owner: FundingFactOwner }>();
  // Baseline replay is private and uncounted. A later duplicate baseline key replaces its owner.
  for (const row of baseline) {
    const owner = createFundingFactOwner();
    const result = owner.write(row);
    if (!result.ok || result.effect !== 'Insert') return failed('row-rejected');
    const key = keyFundingFact(row);
    owners.set(keyIdentity(key), { key, owner });
  }
  let inserted = 0n;
  let unchanged = 0n;
  const conflicts: FundingFactConflict[] = [];
  for (const row of incoming) {
    const key = keyFundingFact(row);
    const identity = keyIdentity(key);
    let entry = owners.get(identity);
    if (entry === undefined) { entry = { key, owner: createFundingFactOwner() }; owners.set(identity, entry); }
    const before = entry.owner.read().stored;
    const result = entry.owner.write(row);
    if (!result.ok) return failed('row-rejected');
    const observed = controller.send(result.effect);
    if (!observed.ok) return failed('controller-rejected');
    switch (result.effect) {
      case 'Insert': if (inserted === maxCount) return failed('count-overflow'); inserted += 1n; break;
      case 'Same': if (unchanged === maxCount) return failed('count-overflow'); unchanged += 1n; break;
      case 'Clash':
        if (before === null) return failed('broken-binding');
        conflicts.push(Object.freeze({ stored: before, rejected: row }));
        break;
    }
  }
  const step = closeSteps.get(controller.read().nodeId);
  if (step === undefined) return failed('broken-binding');
  const closure = controller.send(step.event);
  if (!closure.ok || closure.after.nodeId !== step.next) return failed('controller-rejected');
  const counts = Object.freeze({ inserted: step.kind === 'Refused' ? 0n : inserted, unchanged });
  let report: FundingFactPartitionReport;
  switch (step.kind) {
    case 'Write': {
      const rows: FundingFactRow[] = [];
      for (const entry of [...owners.values()].sort((left, right) => compareKeys(left.key, right.key))) {
        const row = entry.owner.read().stored;
        if (row === null) return failed('broken-binding');
        rows.push(row);
      }
      report = Object.freeze({ kind: 'Write', rows: Object.freeze(rows), counts, group });
      break;
    }
    case 'Noop': report = Object.freeze({ kind: 'Noop', rows: baseline, counts, group }); break;
    case 'Refused': report = Object.freeze({ kind: 'Refused', original: baseline, conflicts: Object.freeze(conflicts), counts, group }); break;
  }
  const intoReport = Object.freeze(function(this: unknown): PartitionResult<FundingFactPartitionReport> {
    if (typeof this !== 'object' || this === null) return failed('invalid-closed');
    const state = closed1.get(this);
    if (state === undefined) return failed('invalid-closed');
    if (state.consumed) return failed('already-consumed');
    const consume = consumeSteps.get(state.controller.read().nodeId);
    if (consume === undefined) return failed('broken-binding');
    const result = state.controller.send(consume.event);
    if (!result.ok || result.after.nodeId !== consume.next) return failed('controller-rejected');
    state.consumed = true;
    return succeeded(state.report);
  });
  const closed: FundingFactClosedPartition = Object.freeze<FundingFactClosedPartition>({ [closedBrand]: "FundingFact", counts, group, intoReport });
  closed1.set(closed, { controller, report, consumed: false });
  return succeeded(closed);
}


export interface MetricsFactRow {
  readonly [rowBrand]: "MetricsFact";
  readonly "symbol": string;
  readonly "create_time": bigint;
  readonly "sum_open_interest": bigint;
  readonly "sum_open_interest_value": bigint;
  readonly "count_toptrader_long_short_ratio": bigint;
  readonly "sum_toptrader_long_short_ratio": bigint;
  readonly "count_long_short_ratio": bigint;
  readonly "sum_taker_long_short_vol_ratio": bigint;
  readonly "event_ts": bigint;
  readonly "available_ts": bigint;
  readonly "received_ts": bigint;
  readonly "available_lag_verified": boolean;
  readonly "schema_version": number;
}
interface MetricsFactIdentity { readonly row: MetricsFactRow; readonly payload: string; readonly provenance: string }
const rows2 = new WeakMap<object, MetricsFactIdentity>();
function identifyMetricsFact(row: MetricsFactRow): MetricsFactIdentity {
  let payload = header("MetricsFact", 'payload');
  let provenance = header("MetricsFact", 'provenance');
  payload += part(utf8.encode("symbol")) + part(utf8.encode("string")) + part(utf8.encode(row["symbol"]));
  payload += part(utf8.encode("create_time")) + part(utf8.encode("i64")) + part(integer64(row["create_time"]));
  payload += part(utf8.encode("sum_open_interest")) + part(utf8.encode("i64")) + part(integer64(row["sum_open_interest"]));
  payload += part(utf8.encode("sum_open_interest_value")) + part(utf8.encode("i64")) + part(integer64(row["sum_open_interest_value"]));
  payload += part(utf8.encode("count_toptrader_long_short_ratio")) + part(utf8.encode("i64")) + part(integer64(row["count_toptrader_long_short_ratio"]));
  payload += part(utf8.encode("sum_toptrader_long_short_ratio")) + part(utf8.encode("i64")) + part(integer64(row["sum_toptrader_long_short_ratio"]));
  payload += part(utf8.encode("count_long_short_ratio")) + part(utf8.encode("i64")) + part(integer64(row["count_long_short_ratio"]));
  payload += part(utf8.encode("sum_taker_long_short_vol_ratio")) + part(utf8.encode("i64")) + part(integer64(row["sum_taker_long_short_vol_ratio"]));
  payload += part(utf8.encode("event_ts")) + part(utf8.encode("i64")) + part(integer64(row["event_ts"]));
  payload += part(utf8.encode("available_ts")) + part(utf8.encode("i64")) + part(integer64(row["available_ts"]));
  payload += part(utf8.encode("available_lag_verified")) + part(utf8.encode("bool")) + part(new Uint8Array([row["available_lag_verified"] ? 1 : 0]));
  payload += part(utf8.encode("schema_version")) + part(utf8.encode("u32")) + part(integer32(row["schema_version"]));
  provenance += part(utf8.encode("received_ts")) + part(utf8.encode("i64")) + part(integer64(row["received_ts"]));
  return Object.freeze({ row, payload, provenance });
}
/** Parse and copy the complete row before it can enter a live owner. Accessors are refused. */
export function createMetricsFactRow(input: unknown): MetricsFactRow {
  if (typeof input !== 'object' || input === null || Array.isArray(input)) throw new TypeError('ROW303: expected a complete data object');
  const keys = Reflect.ownKeys(input);
  if (keys.length !== 13 || keys.some(key => typeof key !== 'string' || !["symbol","create_time","sum_open_interest","sum_open_interest_value","count_toptrader_long_short_ratio","sum_toptrader_long_short_ratio","count_long_short_ratio","sum_taker_long_short_vol_ratio","event_ts","available_ts","received_ts","available_lag_verified","schema_version"].includes(key))) throw new TypeError('ROW303: row fields do not match the checked schema');
  const field0: unknown = descriptor(input, "symbol");
  if (typeof field0 !== 'string' || !unicode(field0)) throw new TypeError("ROW303: MetricsFact.symbol requires string");
  const field1: unknown = descriptor(input, "create_time");
  if (typeof field1 !== 'bigint' || field1 < -9223372036854775808n || field1 > 9223372036854775807n) throw new TypeError("ROW303: MetricsFact.create_time requires i64");
  const field2: unknown = descriptor(input, "sum_open_interest");
  if (typeof field2 !== 'bigint' || field2 < -9223372036854775808n || field2 > 9223372036854775807n) throw new TypeError("ROW303: MetricsFact.sum_open_interest requires i64");
  const field3: unknown = descriptor(input, "sum_open_interest_value");
  if (typeof field3 !== 'bigint' || field3 < -9223372036854775808n || field3 > 9223372036854775807n) throw new TypeError("ROW303: MetricsFact.sum_open_interest_value requires i64");
  const field4: unknown = descriptor(input, "count_toptrader_long_short_ratio");
  if (typeof field4 !== 'bigint' || field4 < -9223372036854775808n || field4 > 9223372036854775807n) throw new TypeError("ROW303: MetricsFact.count_toptrader_long_short_ratio requires i64");
  const field5: unknown = descriptor(input, "sum_toptrader_long_short_ratio");
  if (typeof field5 !== 'bigint' || field5 < -9223372036854775808n || field5 > 9223372036854775807n) throw new TypeError("ROW303: MetricsFact.sum_toptrader_long_short_ratio requires i64");
  const field6: unknown = descriptor(input, "count_long_short_ratio");
  if (typeof field6 !== 'bigint' || field6 < -9223372036854775808n || field6 > 9223372036854775807n) throw new TypeError("ROW303: MetricsFact.count_long_short_ratio requires i64");
  const field7: unknown = descriptor(input, "sum_taker_long_short_vol_ratio");
  if (typeof field7 !== 'bigint' || field7 < -9223372036854775808n || field7 > 9223372036854775807n) throw new TypeError("ROW303: MetricsFact.sum_taker_long_short_vol_ratio requires i64");
  const field8: unknown = descriptor(input, "event_ts");
  if (typeof field8 !== 'bigint' || field8 < -9223372036854775808n || field8 > 9223372036854775807n) throw new TypeError("ROW303: MetricsFact.event_ts requires i64");
  const field9: unknown = descriptor(input, "available_ts");
  if (typeof field9 !== 'bigint' || field9 < -9223372036854775808n || field9 > 9223372036854775807n) throw new TypeError("ROW303: MetricsFact.available_ts requires i64");
  const field10: unknown = descriptor(input, "received_ts");
  if (typeof field10 !== 'bigint' || field10 < -9223372036854775808n || field10 > 9223372036854775807n) throw new TypeError("ROW303: MetricsFact.received_ts requires i64");
  const field11: unknown = descriptor(input, "available_lag_verified");
  if (typeof field11 !== 'boolean') throw new TypeError("ROW303: MetricsFact.available_lag_verified requires bool");
  const field12: unknown = descriptor(input, "schema_version");
  if (typeof field12 !== 'number' || !Number.isInteger(field12) || field12 < 0 || field12 > 4294967295) throw new TypeError("ROW303: MetricsFact.schema_version requires u32");
  const row: MetricsFactRow = Object.freeze<MetricsFactRow>({ [rowBrand]: "MetricsFact", "symbol": field0, "create_time": field1, "sum_open_interest": field2, "sum_open_interest_value": field3, "count_toptrader_long_short_ratio": field4, "sum_toptrader_long_short_ratio": field5, "count_long_short_ratio": field6, "sum_taker_long_short_vol_ratio": field7, "event_ts": field8, "available_ts": field9, "received_ts": field10, "available_lag_verified": field11, "schema_version": field12 });
  rows2.set(row, identifyMetricsFact(row));
  return row;
}
interface MetricsFactObservation {
  readonly stored: MetricsFactRow | null;
  readonly rejected: MetricsFactRow | null;
  readonly state: PrimitiveSnapshot;
}
type MetricsFactWriteResult =
  | { readonly ok: true; readonly effect: WriteEffect; readonly receipt: PrimitiveReceipt }
  | { readonly ok: false; readonly reason: 'invalid-row' | 'broken-binding' | PrimitiveRejection['reason']; readonly state: PrimitiveSnapshot };
interface MetricsFactOwner {
  readonly read: () => MetricsFactObservation;
  readonly write: (row: MetricsFactRow) => MetricsFactWriteResult;
}
/** The authenticated token binds payload and provenance from the same complete immutable DTO. */
function createMetricsFactOwner(): MetricsFactOwner {
  const authority = createPrimitiveAuthority();
  let stored: MetricsFactIdentity | null = null;
  let rejected: MetricsFactIdentity | null = null;
  const read = Object.freeze((): MetricsFactObservation => Object.freeze({ stored: stored?.row ?? null, rejected: rejected?.row ?? null, state: authority.read() }));
  const reject = (reason: 'invalid-row' | 'broken-binding'): MetricsFactWriteResult => Object.freeze({ ok: false, reason, state: authority.read() });
  const write = Object.freeze((row: MetricsFactRow): MetricsFactWriteResult => {
    if (typeof row !== 'object' || row === null) return reject('invalid-row');
    const incoming = rows2.get(row);
    if (incoming === undefined) return reject('invalid-row');
    const before = authority.read();
    if (!matches(stored, before, "stored", "storedProvenance", {"kind":"string","value":"none"}, {"kind":"string","value":"none"}) || !matches(rejected, before, "rejected", "rejectedProvenance", {"kind":"string","value":"none"}, {"kind":"string","value":"none"})) return reject('broken-binding');
    const pending = authority.prepare(incoming.payload, incoming.provenance);
    if (!pending.ok) return pending;
    const nextStored = lift(pending.receipt, "stored", "storedProvenance", incoming, stored, rejected, {"kind":"string","value":"none"}, {"kind":"string","value":"none"});
    const nextRejected = lift(pending.receipt, "rejected", "rejectedProvenance", incoming, stored, rejected, {"kind":"string","value":"none"}, {"kind":"string","value":"none"});
    if (!nextStored.ok || !nextRejected.ok) return reject('broken-binding');
    if (!matches(nextStored.row, pending.receipt.after, "stored", "storedProvenance", {"kind":"string","value":"none"}, {"kind":"string","value":"none"}) || !matches(nextRejected.row, pending.receipt.after, "rejected", "rejectedProvenance", {"kind":"string","value":"none"}, {"kind":"string","value":"none"})) return reject('broken-binding');
    const effect = pending.receipt.action;
    if (!isSourceAction(effect)) return reject('broken-binding');
    // Authenticated frozen rows and primitive identities run no application code here.
    const committed = pending.commit();
    if (!committed.ok) return committed;
    stored = nextStored.row;
    rejected = nextRejected.row;
    return Object.freeze({ ok: true, effect, receipt: pending.receipt });
  });
  return Object.freeze({ read, write });
}

export type MetricsFactPartitionReport =
  | { readonly kind: 'Write' | 'Noop'; readonly rows: readonly MetricsFactRow[]; readonly counts: PartitionCounts; readonly group: PartitionGroup | null }
  | { readonly kind: 'Refused'; readonly original: readonly MetricsFactRow[]; readonly conflicts: readonly MetricsFactConflict[]; readonly counts: PartitionCounts; readonly group: PartitionGroup | null };
export interface MetricsFactConflict { readonly stored: MetricsFactRow; readonly rejected: MetricsFactRow }
export interface MetricsFactClosedPartition {
  readonly [closedBrand]: "MetricsFact";
  readonly counts: PartitionCounts;
  readonly group: PartitionGroup | null;
  readonly intoReport: (this: MetricsFactClosedPartition) => PartitionResult<MetricsFactPartitionReport>;
}
interface MetricsFactClosedState {
  readonly controller: PrivateControl;
  readonly report: MetricsFactPartitionReport;
  consumed: boolean;
}
const closed2 = new WeakMap<object, MetricsFactClosedState>();
function keyMetricsFact(row: MetricsFactRow): readonly bigint[] { return [row["create_time"]]; }
function authenticateMetricsFact(value: unknown): MetricsFactRow | undefined {
  return typeof value === 'object' && value !== null ? rows2.get(value)?.row : undefined;
}
/** Complete authenticated rows enter one private trial; this function grants no filesystem authority. */
export function foldMetricsFact(input: { readonly baseline: readonly MetricsFactRow[]; readonly incoming: readonly MetricsFactRow[] }): PartitionResult<MetricsFactClosedPartition> {
  let baseline: readonly MetricsFactRow[];
  let incoming: readonly MetricsFactRow[];
  try {
    const batch = snapshotBatch(input);
    baseline = snapshotArray(batch.baseline, authenticateMetricsFact);
    incoming = snapshotArray(batch.incoming, authenticateMetricsFact);
  } catch (error) { return failed(error instanceof PartitionBoundaryError ? error.reason : 'invalid-batch'); }
  let group: PartitionGroup | null = null;
  for (const row of [...baseline, ...incoming]) {
    const time = row["create_time"];
    if (time < 0n) return failed('invalid-calendar');
    const observed = Object.freeze({ symbol: row["symbol"], dayIndex: time / 86_400_000_000n });
    if (group === null) group = observed;
    else if (group.symbol !== observed.symbol || group.dayIndex !== observed.dayIndex) return failed('wrong-group');
  }
  const controller = createPrivateControl();
  const owners = new Map<string, { readonly key: readonly bigint[]; readonly owner: MetricsFactOwner }>();
  // Baseline replay is private and uncounted. A later duplicate baseline key replaces its owner.
  for (const row of baseline) {
    const owner = createMetricsFactOwner();
    const result = owner.write(row);
    if (!result.ok || result.effect !== 'Insert') return failed('row-rejected');
    const key = keyMetricsFact(row);
    owners.set(keyIdentity(key), { key, owner });
  }
  let inserted = 0n;
  let unchanged = 0n;
  const conflicts: MetricsFactConflict[] = [];
  for (const row of incoming) {
    const key = keyMetricsFact(row);
    const identity = keyIdentity(key);
    let entry = owners.get(identity);
    if (entry === undefined) { entry = { key, owner: createMetricsFactOwner() }; owners.set(identity, entry); }
    const before = entry.owner.read().stored;
    const result = entry.owner.write(row);
    if (!result.ok) return failed('row-rejected');
    const observed = controller.send(result.effect);
    if (!observed.ok) return failed('controller-rejected');
    switch (result.effect) {
      case 'Insert': if (inserted === maxCount) return failed('count-overflow'); inserted += 1n; break;
      case 'Same': if (unchanged === maxCount) return failed('count-overflow'); unchanged += 1n; break;
      case 'Clash':
        if (before === null) return failed('broken-binding');
        conflicts.push(Object.freeze({ stored: before, rejected: row }));
        break;
    }
  }
  const step = closeSteps.get(controller.read().nodeId);
  if (step === undefined) return failed('broken-binding');
  const closure = controller.send(step.event);
  if (!closure.ok || closure.after.nodeId !== step.next) return failed('controller-rejected');
  const counts = Object.freeze({ inserted: step.kind === 'Refused' ? 0n : inserted, unchanged });
  let report: MetricsFactPartitionReport;
  switch (step.kind) {
    case 'Write': {
      const rows: MetricsFactRow[] = [];
      for (const entry of [...owners.values()].sort((left, right) => compareKeys(left.key, right.key))) {
        const row = entry.owner.read().stored;
        if (row === null) return failed('broken-binding');
        rows.push(row);
      }
      report = Object.freeze({ kind: 'Write', rows: Object.freeze(rows), counts, group });
      break;
    }
    case 'Noop': report = Object.freeze({ kind: 'Noop', rows: baseline, counts, group }); break;
    case 'Refused': report = Object.freeze({ kind: 'Refused', original: baseline, conflicts: Object.freeze(conflicts), counts, group }); break;
  }
  const intoReport = Object.freeze(function(this: unknown): PartitionResult<MetricsFactPartitionReport> {
    if (typeof this !== 'object' || this === null) return failed('invalid-closed');
    const state = closed2.get(this);
    if (state === undefined) return failed('invalid-closed');
    if (state.consumed) return failed('already-consumed');
    const consume = consumeSteps.get(state.controller.read().nodeId);
    if (consume === undefined) return failed('broken-binding');
    const result = state.controller.send(consume.event);
    if (!result.ok || result.after.nodeId !== consume.next) return failed('controller-rejected');
    state.consumed = true;
    return succeeded(state.report);
  });
  const closed: MetricsFactClosedPartition = Object.freeze<MetricsFactClosedPartition>({ [closedBrand]: "MetricsFact", counts, group, intoReport });
  closed2.set(closed, { controller, report, consumed: false });
  return succeeded(closed);
}


export interface BookFactRow {
  readonly [rowBrand]: "BookFact";
  readonly "symbol": string;
  readonly "timestamp": bigint;
  readonly "percentage": bigint;
  readonly "depth": bigint;
  readonly "notional": bigint;
  readonly "level_schema": number;
  readonly "event_ts": bigint;
  readonly "available_ts": bigint;
  readonly "received_ts": bigint;
  readonly "schema_version": number;
}
interface BookFactIdentity { readonly row: BookFactRow; readonly payload: string; readonly provenance: string }
const rows3 = new WeakMap<object, BookFactIdentity>();
function identifyBookFact(row: BookFactRow): BookFactIdentity {
  let payload = header("BookFact", 'payload');
  let provenance = header("BookFact", 'provenance');
  payload += part(utf8.encode("symbol")) + part(utf8.encode("string")) + part(utf8.encode(row["symbol"]));
  payload += part(utf8.encode("timestamp")) + part(utf8.encode("i64")) + part(integer64(row["timestamp"]));
  payload += part(utf8.encode("percentage")) + part(utf8.encode("i64")) + part(integer64(row["percentage"]));
  payload += part(utf8.encode("depth")) + part(utf8.encode("i64")) + part(integer64(row["depth"]));
  payload += part(utf8.encode("notional")) + part(utf8.encode("i64")) + part(integer64(row["notional"]));
  payload += part(utf8.encode("level_schema")) + part(utf8.encode("u32")) + part(integer32(row["level_schema"]));
  payload += part(utf8.encode("event_ts")) + part(utf8.encode("i64")) + part(integer64(row["event_ts"]));
  payload += part(utf8.encode("available_ts")) + part(utf8.encode("i64")) + part(integer64(row["available_ts"]));
  payload += part(utf8.encode("schema_version")) + part(utf8.encode("u32")) + part(integer32(row["schema_version"]));
  provenance += part(utf8.encode("received_ts")) + part(utf8.encode("i64")) + part(integer64(row["received_ts"]));
  return Object.freeze({ row, payload, provenance });
}
/** Parse and copy the complete row before it can enter a live owner. Accessors are refused. */
export function createBookFactRow(input: unknown): BookFactRow {
  if (typeof input !== 'object' || input === null || Array.isArray(input)) throw new TypeError('ROW303: expected a complete data object');
  const keys = Reflect.ownKeys(input);
  if (keys.length !== 10 || keys.some(key => typeof key !== 'string' || !["symbol","timestamp","percentage","depth","notional","level_schema","event_ts","available_ts","received_ts","schema_version"].includes(key))) throw new TypeError('ROW303: row fields do not match the checked schema');
  const field0: unknown = descriptor(input, "symbol");
  if (typeof field0 !== 'string' || !unicode(field0)) throw new TypeError("ROW303: BookFact.symbol requires string");
  const field1: unknown = descriptor(input, "timestamp");
  if (typeof field1 !== 'bigint' || field1 < -9223372036854775808n || field1 > 9223372036854775807n) throw new TypeError("ROW303: BookFact.timestamp requires i64");
  const field2: unknown = descriptor(input, "percentage");
  if (typeof field2 !== 'bigint' || field2 < -9223372036854775808n || field2 > 9223372036854775807n) throw new TypeError("ROW303: BookFact.percentage requires i64");
  const field3: unknown = descriptor(input, "depth");
  if (typeof field3 !== 'bigint' || field3 < -9223372036854775808n || field3 > 9223372036854775807n) throw new TypeError("ROW303: BookFact.depth requires i64");
  const field4: unknown = descriptor(input, "notional");
  if (typeof field4 !== 'bigint' || field4 < -9223372036854775808n || field4 > 9223372036854775807n) throw new TypeError("ROW303: BookFact.notional requires i64");
  const field5: unknown = descriptor(input, "level_schema");
  if (typeof field5 !== 'number' || !Number.isInteger(field5) || field5 < 0 || field5 > 4294967295) throw new TypeError("ROW303: BookFact.level_schema requires u32");
  const field6: unknown = descriptor(input, "event_ts");
  if (typeof field6 !== 'bigint' || field6 < -9223372036854775808n || field6 > 9223372036854775807n) throw new TypeError("ROW303: BookFact.event_ts requires i64");
  const field7: unknown = descriptor(input, "available_ts");
  if (typeof field7 !== 'bigint' || field7 < -9223372036854775808n || field7 > 9223372036854775807n) throw new TypeError("ROW303: BookFact.available_ts requires i64");
  const field8: unknown = descriptor(input, "received_ts");
  if (typeof field8 !== 'bigint' || field8 < -9223372036854775808n || field8 > 9223372036854775807n) throw new TypeError("ROW303: BookFact.received_ts requires i64");
  const field9: unknown = descriptor(input, "schema_version");
  if (typeof field9 !== 'number' || !Number.isInteger(field9) || field9 < 0 || field9 > 4294967295) throw new TypeError("ROW303: BookFact.schema_version requires u32");
  const row: BookFactRow = Object.freeze<BookFactRow>({ [rowBrand]: "BookFact", "symbol": field0, "timestamp": field1, "percentage": field2, "depth": field3, "notional": field4, "level_schema": field5, "event_ts": field6, "available_ts": field7, "received_ts": field8, "schema_version": field9 });
  rows3.set(row, identifyBookFact(row));
  return row;
}
interface BookFactObservation {
  readonly stored: BookFactRow | null;
  readonly rejected: BookFactRow | null;
  readonly state: PrimitiveSnapshot;
}
type BookFactWriteResult =
  | { readonly ok: true; readonly effect: WriteEffect; readonly receipt: PrimitiveReceipt }
  | { readonly ok: false; readonly reason: 'invalid-row' | 'broken-binding' | PrimitiveRejection['reason']; readonly state: PrimitiveSnapshot };
interface BookFactOwner {
  readonly read: () => BookFactObservation;
  readonly write: (row: BookFactRow) => BookFactWriteResult;
}
/** The authenticated token binds payload and provenance from the same complete immutable DTO. */
function createBookFactOwner(): BookFactOwner {
  const authority = createPrimitiveAuthority();
  let stored: BookFactIdentity | null = null;
  let rejected: BookFactIdentity | null = null;
  const read = Object.freeze((): BookFactObservation => Object.freeze({ stored: stored?.row ?? null, rejected: rejected?.row ?? null, state: authority.read() }));
  const reject = (reason: 'invalid-row' | 'broken-binding'): BookFactWriteResult => Object.freeze({ ok: false, reason, state: authority.read() });
  const write = Object.freeze((row: BookFactRow): BookFactWriteResult => {
    if (typeof row !== 'object' || row === null) return reject('invalid-row');
    const incoming = rows3.get(row);
    if (incoming === undefined) return reject('invalid-row');
    const before = authority.read();
    if (!matches(stored, before, "stored", "storedProvenance", {"kind":"string","value":"none"}, {"kind":"string","value":"none"}) || !matches(rejected, before, "rejected", "rejectedProvenance", {"kind":"string","value":"none"}, {"kind":"string","value":"none"})) return reject('broken-binding');
    const pending = authority.prepare(incoming.payload, incoming.provenance);
    if (!pending.ok) return pending;
    const nextStored = lift(pending.receipt, "stored", "storedProvenance", incoming, stored, rejected, {"kind":"string","value":"none"}, {"kind":"string","value":"none"});
    const nextRejected = lift(pending.receipt, "rejected", "rejectedProvenance", incoming, stored, rejected, {"kind":"string","value":"none"}, {"kind":"string","value":"none"});
    if (!nextStored.ok || !nextRejected.ok) return reject('broken-binding');
    if (!matches(nextStored.row, pending.receipt.after, "stored", "storedProvenance", {"kind":"string","value":"none"}, {"kind":"string","value":"none"}) || !matches(nextRejected.row, pending.receipt.after, "rejected", "rejectedProvenance", {"kind":"string","value":"none"}, {"kind":"string","value":"none"})) return reject('broken-binding');
    const effect = pending.receipt.action;
    if (!isSourceAction(effect)) return reject('broken-binding');
    // Authenticated frozen rows and primitive identities run no application code here.
    const committed = pending.commit();
    if (!committed.ok) return committed;
    stored = nextStored.row;
    rejected = nextRejected.row;
    return Object.freeze({ ok: true, effect, receipt: pending.receipt });
  });
  return Object.freeze({ read, write });
}

export type BookFactPartitionReport =
  | { readonly kind: 'Write' | 'Noop'; readonly rows: readonly BookFactRow[]; readonly counts: PartitionCounts; readonly group: PartitionGroup | null }
  | { readonly kind: 'Refused'; readonly original: readonly BookFactRow[]; readonly conflicts: readonly BookFactConflict[]; readonly counts: PartitionCounts; readonly group: PartitionGroup | null };
export interface BookFactConflict { readonly stored: BookFactRow; readonly rejected: BookFactRow }
export interface BookFactClosedPartition {
  readonly [closedBrand]: "BookFact";
  readonly counts: PartitionCounts;
  readonly group: PartitionGroup | null;
  readonly intoReport: (this: BookFactClosedPartition) => PartitionResult<BookFactPartitionReport>;
}
interface BookFactClosedState {
  readonly controller: PrivateControl;
  readonly report: BookFactPartitionReport;
  consumed: boolean;
}
const closed3 = new WeakMap<object, BookFactClosedState>();
function keyBookFact(row: BookFactRow): readonly bigint[] { return [row["timestamp"], row["percentage"]]; }
function authenticateBookFact(value: unknown): BookFactRow | undefined {
  return typeof value === 'object' && value !== null ? rows3.get(value)?.row : undefined;
}
/** Complete authenticated rows enter one private trial; this function grants no filesystem authority. */
export function foldBookFact(input: { readonly baseline: readonly BookFactRow[]; readonly incoming: readonly BookFactRow[] }): PartitionResult<BookFactClosedPartition> {
  let baseline: readonly BookFactRow[];
  let incoming: readonly BookFactRow[];
  try {
    const batch = snapshotBatch(input);
    baseline = snapshotArray(batch.baseline, authenticateBookFact);
    incoming = snapshotArray(batch.incoming, authenticateBookFact);
  } catch (error) { return failed(error instanceof PartitionBoundaryError ? error.reason : 'invalid-batch'); }
  let group: PartitionGroup | null = null;
  for (const row of [...baseline, ...incoming]) {
    const time = row["timestamp"];
    if (time < 0n) return failed('invalid-calendar');
    const observed = Object.freeze({ symbol: row["symbol"], dayIndex: time / 86_400_000_000n });
    if (group === null) group = observed;
    else if (group.symbol !== observed.symbol || group.dayIndex !== observed.dayIndex) return failed('wrong-group');
  }
  const controller = createPrivateControl();
  const owners = new Map<string, { readonly key: readonly bigint[]; readonly owner: BookFactOwner }>();
  // Baseline replay is private and uncounted. A later duplicate baseline key replaces its owner.
  for (const row of baseline) {
    const owner = createBookFactOwner();
    const result = owner.write(row);
    if (!result.ok || result.effect !== 'Insert') return failed('row-rejected');
    const key = keyBookFact(row);
    owners.set(keyIdentity(key), { key, owner });
  }
  let inserted = 0n;
  let unchanged = 0n;
  const conflicts: BookFactConflict[] = [];
  for (const row of incoming) {
    const key = keyBookFact(row);
    const identity = keyIdentity(key);
    let entry = owners.get(identity);
    if (entry === undefined) { entry = { key, owner: createBookFactOwner() }; owners.set(identity, entry); }
    const before = entry.owner.read().stored;
    const result = entry.owner.write(row);
    if (!result.ok) return failed('row-rejected');
    const observed = controller.send(result.effect);
    if (!observed.ok) return failed('controller-rejected');
    switch (result.effect) {
      case 'Insert': if (inserted === maxCount) return failed('count-overflow'); inserted += 1n; break;
      case 'Same': if (unchanged === maxCount) return failed('count-overflow'); unchanged += 1n; break;
      case 'Clash':
        if (before === null) return failed('broken-binding');
        conflicts.push(Object.freeze({ stored: before, rejected: row }));
        break;
    }
  }
  const step = closeSteps.get(controller.read().nodeId);
  if (step === undefined) return failed('broken-binding');
  const closure = controller.send(step.event);
  if (!closure.ok || closure.after.nodeId !== step.next) return failed('controller-rejected');
  const counts = Object.freeze({ inserted: step.kind === 'Refused' ? 0n : inserted, unchanged });
  let report: BookFactPartitionReport;
  switch (step.kind) {
    case 'Write': {
      const rows: BookFactRow[] = [];
      for (const entry of [...owners.values()].sort((left, right) => compareKeys(left.key, right.key))) {
        const row = entry.owner.read().stored;
        if (row === null) return failed('broken-binding');
        rows.push(row);
      }
      report = Object.freeze({ kind: 'Write', rows: Object.freeze(rows), counts, group });
      break;
    }
    case 'Noop': report = Object.freeze({ kind: 'Noop', rows: baseline, counts, group }); break;
    case 'Refused': report = Object.freeze({ kind: 'Refused', original: baseline, conflicts: Object.freeze(conflicts), counts, group }); break;
  }
  const intoReport = Object.freeze(function(this: unknown): PartitionResult<BookFactPartitionReport> {
    if (typeof this !== 'object' || this === null) return failed('invalid-closed');
    const state = closed3.get(this);
    if (state === undefined) return failed('invalid-closed');
    if (state.consumed) return failed('already-consumed');
    const consume = consumeSteps.get(state.controller.read().nodeId);
    if (consume === undefined) return failed('broken-binding');
    const result = state.controller.send(consume.event);
    if (!result.ok || result.after.nodeId !== consume.next) return failed('controller-rejected');
    state.consumed = true;
    return succeeded(state.report);
  });
  const closed: BookFactClosedPartition = Object.freeze<BookFactClosedPartition>({ [closedBrand]: "BookFact", counts, group, intoReport });
  closed3.set(closed, { controller, report, consumed: false });
  return succeeded(closed);
}
