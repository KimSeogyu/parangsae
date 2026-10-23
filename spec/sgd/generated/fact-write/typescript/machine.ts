// Generated portable closed DTO authority from an audited Rust whole-row schema.
// It does not prove TypeScript application source coverage or persistence.
import { createFactEnvelopeMachine as createPrimitiveAuthority, type FactEnvelopeMachineValue as PrimitiveValue, type FactEnvelopeMachineExpression as PrimitiveExpression, type FactEnvelopeMachineSnapshot as PrimitiveSnapshot, type FactEnvelopeMachineReceipt as PrimitiveReceipt, type FactEnvelopeMachineRejection as PrimitiveRejection } from './primitive.js';
const rowBrand: unique symbol = Symbol('sgd-row');
const utf8 = new TextEncoder();
export type WriteEffect = "Clash" | "Insert" | "Same";
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
export interface KlineFactObservation {
  readonly stored: KlineFactRow | null;
  readonly rejected: KlineFactRow | null;
  readonly state: PrimitiveSnapshot;
}
export type KlineFactWriteResult =
  | { readonly ok: true; readonly effect: WriteEffect; readonly receipt: PrimitiveReceipt }
  | { readonly ok: false; readonly reason: 'invalid-row' | 'broken-binding' | PrimitiveRejection['reason']; readonly state: PrimitiveSnapshot };
export interface KlineFactOwner {
  readonly read: () => KlineFactObservation;
  readonly write: (row: KlineFactRow) => KlineFactWriteResult;
}
/** The authenticated token binds payload and provenance from the same complete immutable DTO. */
export function createKlineFactOwner(): KlineFactOwner {
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
export interface FundingFactObservation {
  readonly stored: FundingFactRow | null;
  readonly rejected: FundingFactRow | null;
  readonly state: PrimitiveSnapshot;
}
export type FundingFactWriteResult =
  | { readonly ok: true; readonly effect: WriteEffect; readonly receipt: PrimitiveReceipt }
  | { readonly ok: false; readonly reason: 'invalid-row' | 'broken-binding' | PrimitiveRejection['reason']; readonly state: PrimitiveSnapshot };
export interface FundingFactOwner {
  readonly read: () => FundingFactObservation;
  readonly write: (row: FundingFactRow) => FundingFactWriteResult;
}
/** The authenticated token binds payload and provenance from the same complete immutable DTO. */
export function createFundingFactOwner(): FundingFactOwner {
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
export interface MetricsFactObservation {
  readonly stored: MetricsFactRow | null;
  readonly rejected: MetricsFactRow | null;
  readonly state: PrimitiveSnapshot;
}
export type MetricsFactWriteResult =
  | { readonly ok: true; readonly effect: WriteEffect; readonly receipt: PrimitiveReceipt }
  | { readonly ok: false; readonly reason: 'invalid-row' | 'broken-binding' | PrimitiveRejection['reason']; readonly state: PrimitiveSnapshot };
export interface MetricsFactOwner {
  readonly read: () => MetricsFactObservation;
  readonly write: (row: MetricsFactRow) => MetricsFactWriteResult;
}
/** The authenticated token binds payload and provenance from the same complete immutable DTO. */
export function createMetricsFactOwner(): MetricsFactOwner {
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
export interface BookFactObservation {
  readonly stored: BookFactRow | null;
  readonly rejected: BookFactRow | null;
  readonly state: PrimitiveSnapshot;
}
export type BookFactWriteResult =
  | { readonly ok: true; readonly effect: WriteEffect; readonly receipt: PrimitiveReceipt }
  | { readonly ok: false; readonly reason: 'invalid-row' | 'broken-binding' | PrimitiveRejection['reason']; readonly state: PrimitiveSnapshot };
export interface BookFactOwner {
  readonly read: () => BookFactObservation;
  readonly write: (row: BookFactRow) => BookFactWriteResult;
}
/** The authenticated token binds payload and provenance from the same complete immutable DTO. */
export function createBookFactOwner(): BookFactOwner {
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
