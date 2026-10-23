// Generated finite-owned authority. Regenerate from the verified TLA+ model; do not edit.
// Scope: one live object, exact finite graph, synchronous dispatch, trusted JavaScript realm.
export type FactEnvelopeMachineControlFiniteValue =
  | { readonly kind: 'string'; readonly value: string }
  | { readonly kind: 'boolean'; readonly value: boolean }
  | { readonly kind: 'integer'; readonly value: string };
export type FactEnvelopeMachineControlEvent = "Clash" | "CloseNoop" | "CloseRefused" | "CloseWrite" | "ConsumeNoop" | "ConsumeRefused" | "ConsumeWrite" | "Insert" | "Same";
export type FactEnvelopeMachineControlNodeId = "s0" | "s1" | "s10" | "s11" | "s2" | "s3" | "s4" | "s5" | "s6" | "s7" | "s8" | "s9";
export interface FactEnvelopeMachineControlValues {
  readonly ["hasConflict"]: FactEnvelopeMachineControlFiniteValue;
  readonly ["hasInserted"]: FactEnvelopeMachineControlFiniteValue;
  readonly ["phase"]: FactEnvelopeMachineControlFiniteValue;
}
export interface FactEnvelopeMachineControlSnapshot {
  readonly model: "PartitionMergeControl";
  readonly modelRevision: "030db4d6fef0c8de3dc20a42172a295a0aefd7693d3e83029b8a2343ef205cd0";
  readonly nodeId: FactEnvelopeMachineControlNodeId;
  readonly revision: string;
  readonly values: FactEnvelopeMachineControlValues;
}
export type FactEnvelopeMachineControlDispatchResult =
  | { readonly ok: true; readonly event: FactEnvelopeMachineControlEvent; readonly before: FactEnvelopeMachineControlSnapshot; readonly after: FactEnvelopeMachineControlSnapshot }
  | { readonly ok: false; readonly reason: 'disabled' | 'unknown-event' | 'revision-exhausted'; readonly state: FactEnvelopeMachineControlSnapshot };
export interface FactEnvelopeMachineControl {
  readonly read: () => FactEnvelopeMachineControlSnapshot;
  readonly send: (event: FactEnvelopeMachineControlEvent) => FactEnvelopeMachineControlDispatchResult;
}

const nodes = new Map<FactEnvelopeMachineControlNodeId, FactEnvelopeMachineControlValues>([
  ["s0", Object.freeze({
    ["hasConflict"]: Object.freeze({"kind":"boolean","value":false}),
    ["hasInserted"]: Object.freeze({"kind":"boolean","value":false}),
    ["phase"]: Object.freeze({"kind":"string","value":"Consumed"}),
  })],
  ["s1", Object.freeze({
    ["hasConflict"]: Object.freeze({"kind":"boolean","value":false}),
    ["hasInserted"]: Object.freeze({"kind":"boolean","value":false}),
    ["phase"]: Object.freeze({"kind":"string","value":"Open"}),
  })],
  ["s10", Object.freeze({
    ["hasConflict"]: Object.freeze({"kind":"boolean","value":true}),
    ["hasInserted"]: Object.freeze({"kind":"boolean","value":true}),
    ["phase"]: Object.freeze({"kind":"string","value":"Open"}),
  })],
  ["s11", Object.freeze({
    ["hasConflict"]: Object.freeze({"kind":"boolean","value":true}),
    ["hasInserted"]: Object.freeze({"kind":"boolean","value":true}),
    ["phase"]: Object.freeze({"kind":"string","value":"ReadyRefused"}),
  })],
  ["s2", Object.freeze({
    ["hasConflict"]: Object.freeze({"kind":"boolean","value":false}),
    ["hasInserted"]: Object.freeze({"kind":"boolean","value":false}),
    ["phase"]: Object.freeze({"kind":"string","value":"ReadyNoop"}),
  })],
  ["s3", Object.freeze({
    ["hasConflict"]: Object.freeze({"kind":"boolean","value":false}),
    ["hasInserted"]: Object.freeze({"kind":"boolean","value":true}),
    ["phase"]: Object.freeze({"kind":"string","value":"Consumed"}),
  })],
  ["s4", Object.freeze({
    ["hasConflict"]: Object.freeze({"kind":"boolean","value":false}),
    ["hasInserted"]: Object.freeze({"kind":"boolean","value":true}),
    ["phase"]: Object.freeze({"kind":"string","value":"Open"}),
  })],
  ["s5", Object.freeze({
    ["hasConflict"]: Object.freeze({"kind":"boolean","value":false}),
    ["hasInserted"]: Object.freeze({"kind":"boolean","value":true}),
    ["phase"]: Object.freeze({"kind":"string","value":"ReadyWrite"}),
  })],
  ["s6", Object.freeze({
    ["hasConflict"]: Object.freeze({"kind":"boolean","value":true}),
    ["hasInserted"]: Object.freeze({"kind":"boolean","value":false}),
    ["phase"]: Object.freeze({"kind":"string","value":"Consumed"}),
  })],
  ["s7", Object.freeze({
    ["hasConflict"]: Object.freeze({"kind":"boolean","value":true}),
    ["hasInserted"]: Object.freeze({"kind":"boolean","value":false}),
    ["phase"]: Object.freeze({"kind":"string","value":"Open"}),
  })],
  ["s8", Object.freeze({
    ["hasConflict"]: Object.freeze({"kind":"boolean","value":true}),
    ["hasInserted"]: Object.freeze({"kind":"boolean","value":false}),
    ["phase"]: Object.freeze({"kind":"string","value":"ReadyRefused"}),
  })],
  ["s9", Object.freeze({
    ["hasConflict"]: Object.freeze({"kind":"boolean","value":true}),
    ["hasInserted"]: Object.freeze({"kind":"boolean","value":true}),
    ["phase"]: Object.freeze({"kind":"string","value":"Consumed"}),
  })],
]);
const transitions = new Map<FactEnvelopeMachineControlNodeId, globalThis.Map<string, FactEnvelopeMachineControlNodeId>>([
  ["s0", new Map<string, FactEnvelopeMachineControlNodeId>([])],
  ["s1", new Map<string, FactEnvelopeMachineControlNodeId>([["Clash", "s7"], ["CloseNoop", "s2"], ["Insert", "s4"], ["Same", "s1"]])],
  ["s10", new Map<string, FactEnvelopeMachineControlNodeId>([["Clash", "s10"], ["CloseRefused", "s11"], ["Insert", "s10"], ["Same", "s10"]])],
  ["s11", new Map<string, FactEnvelopeMachineControlNodeId>([["ConsumeRefused", "s9"]])],
  ["s2", new Map<string, FactEnvelopeMachineControlNodeId>([["ConsumeNoop", "s0"]])],
  ["s3", new Map<string, FactEnvelopeMachineControlNodeId>([])],
  ["s4", new Map<string, FactEnvelopeMachineControlNodeId>([["Clash", "s10"], ["CloseWrite", "s5"], ["Insert", "s4"], ["Same", "s4"]])],
  ["s5", new Map<string, FactEnvelopeMachineControlNodeId>([["ConsumeWrite", "s3"]])],
  ["s6", new Map<string, FactEnvelopeMachineControlNodeId>([])],
  ["s7", new Map<string, FactEnvelopeMachineControlNodeId>([["Clash", "s7"], ["CloseRefused", "s8"], ["Insert", "s10"], ["Same", "s7"]])],
  ["s8", new Map<string, FactEnvelopeMachineControlNodeId>([["ConsumeRefused", "s6"]])],
  ["s9", new Map<string, FactEnvelopeMachineControlNodeId>([])],
]);
const events = new Set<string>(["Clash","CloseNoop","CloseRefused","CloseWrite","ConsumeNoop","ConsumeRefused","ConsumeWrite","Insert","Same"]);
const members = new WeakSet<object>();
const maxRevision = 18446744073709551615n;

function snapshot(nodeId: FactEnvelopeMachineControlNodeId, revision: bigint): FactEnvelopeMachineControlSnapshot {
  const values = nodes.get(nodeId);
  if (values === undefined) throw new Error('Corrupt generated finite-owned graph');
  return Object.freeze({
    model: "PartitionMergeControl",
    modelRevision: "030db4d6fef0c8de3dc20a42172a295a0aefd7693d3e83029b8a2343ef205cd0",
    nodeId,
    revision: revision.toString(),
    values: Object.freeze({
      ["hasConflict"]: Object.freeze({ ...values["hasConflict"] }),
      ["hasInserted"]: Object.freeze({ ...values["hasInserted"] }),
      ["phase"]: Object.freeze({ ...values["phase"] }),
    }),
  });
}

/** Recognize only facades created by this loaded module instance. Proxies and copies are not owners. */
export function isFactEnvelopeMachineControl(value: unknown): value is FactEnvelopeMachineControl {
  return typeof value === 'object' && value !== null && members.has(value);
}

/** Start a new live object. Persistent identity, restart, restore and effects require a separate contract. */
export function createFactEnvelopeMachineControl(): FactEnvelopeMachineControl {
  let nodeId: FactEnvelopeMachineControlNodeId = "s1";
  let revision = 0n;
  const read = Object.freeze((): FactEnvelopeMachineControlSnapshot => snapshot(nodeId, revision));
  const send = Object.freeze((event: FactEnvelopeMachineControlEvent): FactEnvelopeMachineControlDispatchResult => {
    if (typeof event !== 'string' || !events.has(event)) {
      return Object.freeze({ ok: false, reason: 'unknown-event', state: read() });
    }
    const successor = transitions.get(nodeId)?.get(event);
    if (successor === undefined) return Object.freeze({ ok: false, reason: 'disabled', state: read() });
    if (revision === maxRevision) return Object.freeze({ ok: false, reason: 'revision-exhausted', state: read() });
    const before = read();
    const nextRevision = revision + 1n;
    const after = snapshot(successor, nextRevision);
    const result: FactEnvelopeMachineControlDispatchResult = Object.freeze({ ok: true, event, before, after });
    nodeId = successor;
    revision = nextRevision;
    return result;
  });
  const facade: FactEnvelopeMachineControl = Object.freeze({ read, send });
  members.add(facade);
  return facade;
}
