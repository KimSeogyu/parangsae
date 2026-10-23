// Generated from the validated TLA+ equality/copy relation. Regenerate; do not edit policy.
export type FactMachineValue =
  | { readonly kind: 'string'; readonly value: string }
  | { readonly kind: 'boolean'; readonly value: boolean }
  | { readonly kind: 'integer'; readonly value: string }
  | { readonly kind: 'data'; readonly domain: "Values"; readonly value: string };
export type FactMachineExpression =
  | { readonly kind: 'literal'; readonly value: FactMachineValue }
  | { readonly kind: 'state' | 'next-state'; readonly variable: string }
  | { readonly kind: 'input' };
export interface FactMachineValues {
  readonly ["phase"]: FactMachineValue;
  readonly ["rejected"]: FactMachineValue;
  readonly ["stored"]: FactMachineValue;
}
export interface FactMachineSnapshot {
  readonly model: "ManifestWrite";
  readonly modelRevision: "5861db29aa488a59a0967058d3fcf00b95b3ce2e5dce772307a0810ef94f21a0";
  readonly revision: string;
  readonly values: FactMachineValues;
}
export interface FactMachineReceipt {
  readonly action: string;
  readonly before: FactMachineSnapshot;
  readonly after: FactMachineSnapshot;
  readonly updates: { readonly [variable: string]: FactMachineExpression };
}
export interface FactMachineRejection {
  readonly ok: false;
  readonly reason: 'invalid-input' | 'disabled' | 'ambiguous' | 'revision-exhausted' | 'stale-preparation' | 'consumed-preparation';
  readonly state: FactMachineSnapshot;
}
export type FactMachineResult = (FactMachineReceipt & { readonly ok: true }) | FactMachineRejection;
export type FactMachinePreparation =
  | { readonly ok: true; readonly receipt: FactMachineReceipt; readonly commit: () => FactMachineResult }
  | FactMachineRejection;
export interface FactMachine {
  readonly read: () => FactMachineSnapshot;
  readonly prepare: (input: string) => FactMachinePreparation;
  readonly send: (input: string) => FactMachineResult;
}
const members = new globalThis.WeakSet<object>();
const maxRevision = 18446744073709551615n;
function same(a: FactMachineValue, b: FactMachineValue): boolean {
  return a.kind === b.kind && a.value === b.value && (a.kind !== 'data' || (b.kind === 'data' && a.domain === b.domain));
}
function belongs(value: FactMachineValue, domain: string): boolean { return value.kind === 'data' && value.domain === domain; }
function snapshot(state: FactMachineValues, revision: bigint): FactMachineSnapshot {
  return Object.freeze({ model: "ManifestWrite", modelRevision: "5861db29aa488a59a0967058d3fcf00b95b3ce2e5dce772307a0810ef94f21a0", revision: revision.toString(), values: Object.freeze({
      ["phase"]: Object.freeze({ ...state["phase"] }),
      ["rejected"]: Object.freeze({ ...state["rejected"] }),
      ["stored"]: Object.freeze({ ...state["stored"] }),
  }) });
}
export function isFactMachine(value: unknown): value is FactMachine { return typeof value === 'object' && value !== null && members.has(value); }
/** Each new owner starts at Init. Payloads are immutable string identities, not external objects. */
export function createFactMachine(): FactMachine {
  let state: FactMachineValues = Object.freeze({
      ["phase"]: Object.freeze({"kind":"string","value":"absent"}),
      ["rejected"]: Object.freeze({"kind":"string","value":"none"}),
      ["stored"]: Object.freeze({"kind":"string","value":"none"}),
  });
  let revision = 0n;
  const read = Object.freeze((): FactMachineSnapshot => snapshot(state, revision));
  const reject = (reason: FactMachineRejection['reason']): FactMachineRejection => Object.freeze({ ok: false, reason, state: read() });
  const prepare = Object.freeze((input: string): FactMachinePreparation => {
    if (typeof input !== 'string') return reject('invalid-input');
    for (const character of input) {
      const code = character.codePointAt(0);
      if (code !== undefined && code >= 0xd800 && code <= 0xdfff) return reject('invalid-input');
    }
    const inputValue: FactMachineValue = Object.freeze({ kind: 'data', domain: "Values", value: input });
    let selected = -1;
    if ((belongs(inputValue, "Values") && same(state["phase"], Object.freeze({"kind":"string","value":"absent"})) && belongs(inputValue, "Values"))) {
      if (selected !== -1) return reject('ambiguous');
      selected = 0;
    }
    if ((belongs(inputValue, "Values") && (same(state["phase"], Object.freeze({"kind":"string","value":"present"})) || same(state["phase"], Object.freeze({"kind":"string","value":"conflict"}))) && same(inputValue, state["stored"]))) {
      if (selected !== -1) return reject('ambiguous');
      selected = 1;
    }
    if ((belongs(inputValue, "Values") && (same(state["phase"], Object.freeze({"kind":"string","value":"present"})) || same(state["phase"], Object.freeze({"kind":"string","value":"conflict"}))) && belongs(inputValue, "Values") && !same(inputValue, state["stored"]))) {
      if (selected !== -1) return reject('ambiguous');
      selected = 2;
    }
    if (selected === -1) return reject('disabled');
    if (revision === maxRevision) return reject('revision-exhausted');
    switch (selected) {
      case 0: {
        const next: FactMachineValues = Object.freeze({
      ["phase"]: Object.freeze({"kind":"string","value":"present"}),
      ["rejected"]: Object.freeze({"kind":"string","value":"none"}),
      ["stored"]: inputValue,
        });
        const updates: { readonly [variable: string]: FactMachineExpression } = Object.freeze({
      ["phase"]: Object.freeze({ kind: 'literal', value: Object.freeze({"kind":"string","value":"present"}) }),
      ["rejected"]: Object.freeze({ kind: 'literal', value: Object.freeze({"kind":"string","value":"none"}) }),
      ["stored"]: Object.freeze({"kind":"input"}),
        });
        const before = snapshot(state, revision);
        const preparedRevision = revision;
        const nextRevision = revision + 1n;
        const after = snapshot(next, nextRevision);
        const receipt: FactMachineReceipt = Object.freeze({ action: "Insert", before, after, updates });
        const result: FactMachineResult = Object.freeze({ ok: true, ...receipt });
        let consumed = false;
        const commit = Object.freeze((): FactMachineResult => {
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
        const next: FactMachineValues = Object.freeze({
      ["phase"]: state["phase"],
      ["rejected"]: state["rejected"],
      ["stored"]: state["stored"],
        });
        const updates: { readonly [variable: string]: FactMachineExpression } = Object.freeze({
      ["phase"]: Object.freeze({"kind":"state","variable":"phase"}),
      ["rejected"]: Object.freeze({"kind":"state","variable":"rejected"}),
      ["stored"]: Object.freeze({"kind":"state","variable":"stored"}),
        });
        const before = snapshot(state, revision);
        const preparedRevision = revision;
        const nextRevision = revision + 1n;
        const after = snapshot(next, nextRevision);
        const receipt: FactMachineReceipt = Object.freeze({ action: "Same", before, after, updates });
        const result: FactMachineResult = Object.freeze({ ok: true, ...receipt });
        let consumed = false;
        const commit = Object.freeze((): FactMachineResult => {
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
        const next: FactMachineValues = Object.freeze({
      ["phase"]: Object.freeze({"kind":"string","value":"conflict"}),
      ["rejected"]: inputValue,
      ["stored"]: state["stored"],
        });
        const updates: { readonly [variable: string]: FactMachineExpression } = Object.freeze({
      ["phase"]: Object.freeze({ kind: 'literal', value: Object.freeze({"kind":"string","value":"conflict"}) }),
      ["rejected"]: Object.freeze({"kind":"input"}),
      ["stored"]: Object.freeze({"kind":"state","variable":"stored"}),
        });
        const before = snapshot(state, revision);
        const preparedRevision = revision;
        const nextRevision = revision + 1n;
        const after = snapshot(next, nextRevision);
        const receipt: FactMachineReceipt = Object.freeze({ action: "Clash", before, after, updates });
        const result: FactMachineResult = Object.freeze({ ok: true, ...receipt });
        let consumed = false;
        const commit = Object.freeze((): FactMachineResult => {
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
  const send = Object.freeze((input: string): FactMachineResult => {
    const prepared = prepare(input);
    return prepared.ok ? prepared.commit() : prepared;
  });
  const facade: FactMachine = Object.freeze({ read, prepare, send });
  members.add(facade);
  return facade;
}
