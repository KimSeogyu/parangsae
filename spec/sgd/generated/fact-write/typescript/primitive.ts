// Generated from the validated TLA+ equality/copy relation. Regenerate; do not edit policy.
export type FactEnvelopeMachineValue =
  | { readonly kind: 'string'; readonly value: string }
  | { readonly kind: 'boolean'; readonly value: boolean }
  | { readonly kind: 'integer'; readonly value: string }
  | { readonly kind: 'data'; readonly domain: "Values" | "Receipts"; readonly value: string };
export type FactEnvelopeMachineExpression =
  | { readonly kind: 'literal'; readonly value: FactEnvelopeMachineValue }
  | { readonly kind: 'state' | 'next-state'; readonly variable: string }
  | { readonly kind: 'input'; readonly parameter: "v" | "provenance" };
export interface FactEnvelopeMachineValues {
  readonly ["phase"]: FactEnvelopeMachineValue;
  readonly ["rejected"]: FactEnvelopeMachineValue;
  readonly ["rejectedProvenance"]: FactEnvelopeMachineValue;
  readonly ["stored"]: FactEnvelopeMachineValue;
  readonly ["storedProvenance"]: FactEnvelopeMachineValue;
}
export interface FactEnvelopeMachineSnapshot {
  readonly model: "FactWriteEnvelope";
  readonly modelRevision: "dcc32c6c46d16abd4b90ee573974b59aec70e95315c296a18484a3ac089ac25d";
  readonly revision: string;
  readonly values: FactEnvelopeMachineValues;
}
export interface FactEnvelopeMachineReceipt {
  readonly action: string;
  readonly before: FactEnvelopeMachineSnapshot;
  readonly after: FactEnvelopeMachineSnapshot;
  readonly updates: { readonly [variable: string]: FactEnvelopeMachineExpression };
}
export interface FactEnvelopeMachineRejection {
  readonly ok: false;
  readonly reason: 'invalid-input' | 'disabled' | 'ambiguous' | 'revision-exhausted' | 'stale-preparation' | 'consumed-preparation';
  readonly state: FactEnvelopeMachineSnapshot;
}
export type FactEnvelopeMachineResult = (FactEnvelopeMachineReceipt & { readonly ok: true }) | FactEnvelopeMachineRejection;
export type FactEnvelopeMachinePreparation =
  | { readonly ok: true; readonly receipt: FactEnvelopeMachineReceipt; readonly commit: () => FactEnvelopeMachineResult }
  | FactEnvelopeMachineRejection;
export interface FactEnvelopeMachine {
  readonly read: () => FactEnvelopeMachineSnapshot;
  readonly prepare: (input_0: string, input_1: string) => FactEnvelopeMachinePreparation;
  readonly send: (input_0: string, input_1: string) => FactEnvelopeMachineResult;
}
const members = new globalThis.WeakSet<object>();
const maxRevision = 18446744073709551615n;
function same(a: FactEnvelopeMachineValue, b: FactEnvelopeMachineValue): boolean {
  return a.kind === b.kind && a.value === b.value && (a.kind !== 'data' || (b.kind === 'data' && a.domain === b.domain));
}
function belongs(value: FactEnvelopeMachineValue, domain: string): boolean { return value.kind === 'data' && value.domain === domain; }
function snapshot(state: FactEnvelopeMachineValues, revision: bigint): FactEnvelopeMachineSnapshot {
  return Object.freeze({ model: "FactWriteEnvelope", modelRevision: "dcc32c6c46d16abd4b90ee573974b59aec70e95315c296a18484a3ac089ac25d", revision: revision.toString(), values: Object.freeze({
      ["phase"]: Object.freeze({ ...state["phase"] }),
      ["rejected"]: Object.freeze({ ...state["rejected"] }),
      ["rejectedProvenance"]: Object.freeze({ ...state["rejectedProvenance"] }),
      ["stored"]: Object.freeze({ ...state["stored"] }),
      ["storedProvenance"]: Object.freeze({ ...state["storedProvenance"] }),
  }) });
}
export function isFactEnvelopeMachine(value: unknown): value is FactEnvelopeMachine { return typeof value === 'object' && value !== null && members.has(value); }
/** Each new owner starts at Init. Payloads are immutable string identities, not external objects. */
export function createFactEnvelopeMachine(): FactEnvelopeMachine {
  let state: FactEnvelopeMachineValues = Object.freeze({
      ["phase"]: Object.freeze({"kind":"string","value":"absent"}),
      ["rejected"]: Object.freeze({"kind":"string","value":"none"}),
      ["rejectedProvenance"]: Object.freeze({"kind":"string","value":"none"}),
      ["stored"]: Object.freeze({"kind":"string","value":"none"}),
      ["storedProvenance"]: Object.freeze({"kind":"string","value":"none"}),
  });
  let revision = 0n;
  const read = Object.freeze((): FactEnvelopeMachineSnapshot => snapshot(state, revision));
  const reject = (reason: FactEnvelopeMachineRejection['reason']): FactEnvelopeMachineRejection => Object.freeze({ ok: false, reason, state: read() });
  const prepare = Object.freeze(function (input_0: string, input_1: string): FactEnvelopeMachinePreparation {
    if (arguments.length !== 2) return reject('invalid-input');
    if (typeof input_0 !== 'string') return reject('invalid-input');
    for (const character of input_0) {
      const code = character.codePointAt(0);
      if (code !== undefined && code >= 0xd800 && code <= 0xdfff) return reject('invalid-input');
    }
    if (typeof input_1 !== 'string') return reject('invalid-input');
    for (const character of input_1) {
      const code = character.codePointAt(0);
      if (code !== undefined && code >= 0xd800 && code <= 0xdfff) return reject('invalid-input');
    }
    const inputValue0: FactEnvelopeMachineValue = Object.freeze({ kind: 'data', domain: "Values", value: input_0 });
    const inputValue1: FactEnvelopeMachineValue = Object.freeze({ kind: 'data', domain: "Receipts", value: input_1 });
    let selected = -1;
    if ((belongs(inputValue0, "Values") && belongs(inputValue1, "Receipts") && same(state["phase"], Object.freeze({"kind":"string","value":"absent"})) && belongs(inputValue0, "Values") && belongs(inputValue1, "Receipts"))) {
      if (selected !== -1) return reject('ambiguous');
      selected = 0;
    }
    if ((belongs(inputValue0, "Values") && belongs(inputValue1, "Receipts") && (same(state["phase"], Object.freeze({"kind":"string","value":"present"})) || same(state["phase"], Object.freeze({"kind":"string","value":"conflict"}))) && same(inputValue0, state["stored"]) && belongs(inputValue1, "Receipts"))) {
      if (selected !== -1) return reject('ambiguous');
      selected = 1;
    }
    if ((belongs(inputValue0, "Values") && belongs(inputValue1, "Receipts") && (same(state["phase"], Object.freeze({"kind":"string","value":"present"})) || same(state["phase"], Object.freeze({"kind":"string","value":"conflict"}))) && belongs(inputValue0, "Values") && !same(inputValue0, state["stored"]) && belongs(inputValue1, "Receipts"))) {
      if (selected !== -1) return reject('ambiguous');
      selected = 2;
    }
    if (selected === -1) return reject('disabled');
    if (revision === maxRevision) return reject('revision-exhausted');
    switch (selected) {
      case 0: {
        const next: FactEnvelopeMachineValues = Object.freeze({
      ["phase"]: Object.freeze({"kind":"string","value":"present"}),
      ["rejected"]: Object.freeze({"kind":"string","value":"none"}),
      ["rejectedProvenance"]: Object.freeze({"kind":"string","value":"none"}),
      ["stored"]: inputValue0,
      ["storedProvenance"]: inputValue1,
        });
        const updates: { readonly [variable: string]: FactEnvelopeMachineExpression } = Object.freeze({
      ["phase"]: Object.freeze({ kind: 'literal', value: Object.freeze({"kind":"string","value":"present"}) }),
      ["rejected"]: Object.freeze({ kind: 'literal', value: Object.freeze({"kind":"string","value":"none"}) }),
      ["rejectedProvenance"]: Object.freeze({ kind: 'literal', value: Object.freeze({"kind":"string","value":"none"}) }),
      ["stored"]: Object.freeze({"kind":"input","parameter":"v"}),
      ["storedProvenance"]: Object.freeze({"kind":"input","parameter":"provenance"}),
        });
        const before = snapshot(state, revision);
        const preparedRevision = revision;
        const nextRevision = revision + 1n;
        const after = snapshot(next, nextRevision);
        const receipt: FactEnvelopeMachineReceipt = Object.freeze({ action: "Insert", before, after, updates });
        const result: FactEnvelopeMachineResult = Object.freeze({ ok: true, ...receipt });
        let consumed = false;
        const commit = Object.freeze((): FactEnvelopeMachineResult => {
          if (consumed) return reject('consumed-preparation');
          consumed = true;
          if (revision !== preparedRevision) return reject('stale-preparation');
          state = next;
          revision = nextRevision;
          return result;
        });
        return Object.freeze({ ok: true, receipt, commit });
      }
      case 1: {
        const next: FactEnvelopeMachineValues = Object.freeze({
      ["phase"]: state["phase"],
      ["rejected"]: state["rejected"],
      ["rejectedProvenance"]: state["rejectedProvenance"],
      ["stored"]: state["stored"],
      ["storedProvenance"]: state["storedProvenance"],
        });
        const updates: { readonly [variable: string]: FactEnvelopeMachineExpression } = Object.freeze({
      ["phase"]: Object.freeze({"kind":"state","variable":"phase"}),
      ["rejected"]: Object.freeze({"kind":"state","variable":"rejected"}),
      ["rejectedProvenance"]: Object.freeze({"kind":"state","variable":"rejectedProvenance"}),
      ["stored"]: Object.freeze({"kind":"state","variable":"stored"}),
      ["storedProvenance"]: Object.freeze({"kind":"state","variable":"storedProvenance"}),
        });
        const before = snapshot(state, revision);
        const preparedRevision = revision;
        const nextRevision = revision + 1n;
        const after = snapshot(next, nextRevision);
        const receipt: FactEnvelopeMachineReceipt = Object.freeze({ action: "Same", before, after, updates });
        const result: FactEnvelopeMachineResult = Object.freeze({ ok: true, ...receipt });
        let consumed = false;
        const commit = Object.freeze((): FactEnvelopeMachineResult => {
          if (consumed) return reject('consumed-preparation');
          consumed = true;
          if (revision !== preparedRevision) return reject('stale-preparation');
          state = next;
          revision = nextRevision;
          return result;
        });
        return Object.freeze({ ok: true, receipt, commit });
      }
      case 2: {
        const next: FactEnvelopeMachineValues = Object.freeze({
      ["phase"]: Object.freeze({"kind":"string","value":"conflict"}),
      ["rejected"]: inputValue0,
      ["rejectedProvenance"]: inputValue1,
      ["stored"]: state["stored"],
      ["storedProvenance"]: state["storedProvenance"],
        });
        const updates: { readonly [variable: string]: FactEnvelopeMachineExpression } = Object.freeze({
      ["phase"]: Object.freeze({ kind: 'literal', value: Object.freeze({"kind":"string","value":"conflict"}) }),
      ["rejected"]: Object.freeze({"kind":"input","parameter":"v"}),
      ["rejectedProvenance"]: Object.freeze({"kind":"input","parameter":"provenance"}),
      ["stored"]: Object.freeze({"kind":"state","variable":"stored"}),
      ["storedProvenance"]: Object.freeze({"kind":"state","variable":"storedProvenance"}),
        });
        const before = snapshot(state, revision);
        const preparedRevision = revision;
        const nextRevision = revision + 1n;
        const after = snapshot(next, nextRevision);
        const receipt: FactEnvelopeMachineReceipt = Object.freeze({ action: "Clash", before, after, updates });
        const result: FactEnvelopeMachineResult = Object.freeze({ ok: true, ...receipt });
        let consumed = false;
        const commit = Object.freeze((): FactEnvelopeMachineResult => {
          if (consumed) return reject('consumed-preparation');
          consumed = true;
          if (revision !== preparedRevision) return reject('stale-preparation');
          state = next;
          revision = nextRevision;
          return result;
        });
        return Object.freeze({ ok: true, receipt, commit });
      }
      default: throw new Error('Corrupt generated symbolic branch');
    }
  });
  const send = Object.freeze(function (input_0: string, input_1: string): FactEnvelopeMachineResult {
    if (arguments.length !== 2) return reject('invalid-input');
    const prepared = prepare(input_0, input_1);
    return prepared.ok ? prepared.commit() : prepared;
  });
  const facade: FactEnvelopeMachine = Object.freeze({ read, prepare, send });
  members.add(facade);
  return facade;
}
