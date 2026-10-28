import Std

/- Trusted domain-protocol checker. The project file supplies numeric DATA only.
   Edge is the independent transition relation of the supplied exact finite
   graph. This module proves the serial port protocol for that graph, not TLA
   extraction, native TypeScript execution, persistence, or external effects.
-/
namespace SGD.DomainProtocol

structure Edge where
  source : Nat
  event : Nat
  target : Nat
  deriving DecidableEq, Repr

structure Operation where
  start : Nat
  success : Nat
  failure : Nat
  deriving DecidableEq, Repr

structure Model where
  stateCount : Nat
  eventCount : Nat
  initial : Nat
  edges : List Edge
  operations : List Operation
  deriving Repr

def Transition (model : Model) (source event target : Nat) : Prop :=
  Edge.mk source event target ∈ model.edges

def lookupEdge (edges : List Edge) (source event : Nat) : Option Nat :=
  match edges with
  | [] => none
  | edge :: rest =>
      if edge.source = source ∧ edge.event = event then some edge.target
      else lookupEdge rest source event

theorem lookup_edge_sound (edges : List Edge) (source event target : Nat)
    (found : lookupEdge edges source event = some target) :
    Edge.mk source event target ∈ edges := by
  induction edges with
  | nil => simp [lookupEdge] at found
  | cons edge rest ih =>
      simp only [lookupEdge] at found
      split at found
      · rename_i same
        cases edge with
        | mk s e t =>
          simp only at same found
          obtain ⟨rfl, rfl⟩ := same
          cases found
          exact List.Mem.head _
      · exact List.Mem.tail _ (ih found)

theorem lookup_transition_sound (model : Model) (source event target : Nat)
    (found : lookupEdge model.edges source event = some target) :
    Transition model source event target := lookup_edge_sound model.edges source event target found

theorem lookup_none_iff_no_transition (model : Model) (source event : Nat) :
    lookupEdge model.edges source event = none ↔ ¬ ∃ target, Transition model source event target := by
  have complete (edges : List Edge) : lookupEdge edges source event = none ↔
      ¬ ∃ target, Edge.mk source event target ∈ edges := by
    induction edges with
    | nil => simp [lookupEdge]
    | cons edge rest ih =>
      cases edge with
      | mk s e t =>
        simp only [lookupEdge]
        split
        · rename_i same
          obtain ⟨rfl, rfl⟩ := same
          simp
        · rename_i different
          simp only [ih, List.mem_cons, not_exists, not_or]
          constructor
          · intro absent target
            exact ⟨by intro equal; cases equal; exact different ⟨rfl, rfl⟩, absent target⟩
          · intro absent target
            exact (absent target).2
  exact complete model.edges

/- This is reachability in the input graph, independently of generated code.
   Connecting a native implementation to this relation is a separate obligation. -/
inductive Reachable (model : Model) : Nat → Prop
  | initial : Reachable model model.initial
  | step {source event target : Nat} : Reachable model source →
      Transition model source event target → Reachable model target

theorem reachable_preserves_invariant (model : Model) (invariant : Nat → Prop)
    (initial : invariant model.initial)
    (closed : ∀ source event target, invariant source →
      Transition model source event target → invariant target)
    {state : Nat} (reachable : Reachable model state) : invariant state := by
  induction reachable with
  | initial => exact initial
  | step prior edge ih => exact closed _ _ _ ih edge

def Enabled (model : Model) (source event : Nat) : Prop :=
  ∃ target, Transition model source event target

def Pending (model : Model) (operation : Operation) (state : Nat) : Prop :=
  ∃ source, Transition model source operation.start state

def AnyPending (model : Model) (state : Nat) : Prop :=
  ∃ operation ∈ model.operations, Pending model operation state

def roles (model : Model) : List Nat :=
  model.operations.flatMap (fun operation => [operation.start, operation.success, operation.failure])

def pendingCheck (model : Model) (operation : Operation) (state : Nat) : Bool :=
  model.edges.any (fun edge => decide (edge.event = operation.start ∧ edge.target = state))

def enabledCheck (model : Model) (state event : Nat) : Bool :=
  model.edges.any (fun edge => decide (edge.source = state ∧ edge.event = event))

def anyPendingCheck (model : Model) (state : Nat) : Bool :=
  model.operations.any (fun operation => pendingCheck model operation state)

theorem pending_check_exact (model : Model) (operation : Operation) (state : Nat) :
    pendingCheck model operation state = true ↔ Pending model operation state := by
  simp only [pendingCheck, List.any_eq_true, decide_eq_true_eq, Pending, Transition]
  constructor
  · rintro ⟨⟨source, event, target⟩, member, sameEvent, sameTarget⟩
    simp only at sameEvent sameTarget
    subst event
    subst target
    exact ⟨source, member⟩
  · rintro ⟨source, member⟩
    exact ⟨⟨source, operation.start, state⟩, member, rfl, rfl⟩

theorem enabled_check_exact (model : Model) (state event : Nat) :
    enabledCheck model state event = true ↔ Enabled model state event := by
  simp only [enabledCheck, List.any_eq_true, decide_eq_true_eq, Enabled, Transition]
  constructor
  · rintro ⟨⟨source, action, target⟩, member, sameSource, sameEvent⟩
    simp only at sameSource sameEvent
    subst source
    subst action
    exact ⟨target, member⟩
  · rintro ⟨target, member⟩
    exact ⟨⟨state, event, target⟩, member, rfl, rfl⟩

theorem any_pending_check_exact (model : Model) (state : Nat) :
    anyPendingCheck model state = true ↔ AnyPending model state := by
  simp only [anyPendingCheck, List.any_eq_true, AnyPending, pending_check_exact]

/- Finite executable checks. Implications are reflected over finite edge/role
   lists; validity does not accept a caller's success flag or a proof receipt.
-/
def boundsCheck (model : Model) : Bool :=
  decide (model.initial < model.stateCount) &&
  (model.edges.all (fun edge => decide
    (edge.source < model.stateCount ∧ edge.event < model.eventCount ∧ edge.target < model.stateCount)) &&
  model.operations.all (fun operation => decide
    (operation.start < model.eventCount ∧ operation.success < model.eventCount ∧ operation.failure < model.eventCount)))

def deterministicCheck (model : Model) : Bool :=
  model.edges.all (fun left => model.edges.all (fun right => decide
    (left.source = right.source ∧ left.event = right.event → left.target = right.target)))

def outcomesCheck (model : Model) : Bool :=
  model.operations.all (fun operation => model.edges.all (fun incoming =>
    if incoming.event = operation.start then
      enabledCheck model incoming.target operation.success &&
      (enabledCheck model incoming.target operation.failure &&
      model.edges.all (fun outgoing => decide
        (outgoing.source = incoming.target →
          outgoing.event = operation.success ∨ outgoing.event = operation.failure)))
    else true))

def entriesCheck (model : Model) : Bool :=
  model.operations.all (fun operation => model.edges.all (fun edge =>
    !pendingCheck model operation edge.target || decide (edge.event = operation.start)))

def completionsCheck (model : Model) : Bool :=
  model.operations.all (fun operation => model.edges.all (fun edge =>
    if edge.event = operation.success ∨ edge.event = operation.failure then
      pendingCheck model operation edge.source && !anyPendingCheck model edge.target
    else true))

def pendingOwnersCheck (model : Model) : Bool :=
  model.operations.all (fun left => model.operations.all (fun right =>
    model.edges.all (fun edge =>
      if edge.event = left.start then
        !pendingCheck model right edge.target || decide (left = right)
      else true)))

def startsCheck (model : Model) : Bool :=
  model.operations.all (fun operation => model.edges.any (fun edge => decide (edge.event = operation.start)))

def valid (model : Model) : Bool :=
  boundsCheck model && (deterministicCheck model &&
  (decide (model.operations ≠ []) && (decide ((roles model).Nodup) &&
  (startsCheck model && (!anyPendingCheck model model.initial &&
  (outcomesCheck model && (entriesCheck model && (completionsCheck model && pendingOwnersCheck model))))))))

/- Independent semantic protocol. Its predicates speak about the graph's
   Transition/Enabled/Pending relations, not about the Boolean checker result.
-/
structure Contract (model : Model) : Prop where
  initialBound : model.initial < model.stateCount
  edgeBounds : ∀ edge ∈ model.edges,
    edge.source < model.stateCount ∧ edge.event < model.eventCount ∧ edge.target < model.stateCount
  roleBounds : ∀ operation ∈ model.operations,
    operation.start < model.eventCount ∧ operation.success < model.eventCount ∧ operation.failure < model.eventCount
  deterministic : ∀ source event first second,
    Transition model source event first → Transition model source event second → first = second
  operationsPresent : model.operations ≠ []
  rolesDisjoint : (roles model).Nodup
  startsObserved : ∀ operation ∈ model.operations, ∃ source target, Transition model source operation.start target
  initialNotPending : ¬ AnyPending model model.initial
  outcomes : ∀ operation ∈ model.operations, ∀ source state,
    Transition model source operation.start state →
      Enabled model state operation.success ∧ Enabled model state operation.failure ∧
      ∀ event target, Transition model state event target →
        event = operation.success ∨ event = operation.failure
  entries : ∀ operation ∈ model.operations, ∀ state,
    Pending model operation state →
      ∀ source event, Transition model source event state → event = operation.start
  completions : ∀ operation ∈ model.operations, ∀ source event target,
    Transition model source event target →
      (event = operation.success ∨ event = operation.failure) →
        Pending model operation source ∧ ¬ AnyPending model target
  uniquePendingOwner : ∀ left ∈ model.operations, ∀ right ∈ model.operations, ∀ state,
    Pending model left state → Pending model right state → left = right

theorem valid_implies_contract (model : Model) (checked : valid model = true) : Contract model := by
  simp only [valid, Bool.and_eq_true, decide_eq_true_eq] at checked
  rcases checked with ⟨bounds, deterministic, present, disjoint, starts, initial, outcomes, entries, completions, owners⟩
  simp only [boundsCheck, Bool.and_eq_true, List.all_eq_true, decide_eq_true_eq] at bounds
  simp only [deterministicCheck, List.all_eq_true, decide_eq_true_eq] at deterministic
  simp only [startsCheck, List.all_eq_true, List.any_eq_true, decide_eq_true_eq] at starts
  simp only [outcomesCheck, List.all_eq_true] at outcomes
  simp only [entriesCheck, List.all_eq_true] at entries
  simp only [completionsCheck, List.all_eq_true] at completions
  simp only [pendingOwnersCheck, List.all_eq_true] at owners
  refine ⟨bounds.1, bounds.2.1, bounds.2.2, ?_, present, disjoint, ?_, ?_, ?_, ?_, ?_, ?_⟩
  · intro source event first second left right
    exact deterministic ⟨source, event, first⟩ left ⟨source, event, second⟩ right ⟨rfl, rfl⟩
  · intro operation member
    obtain ⟨edge, inGraph, event⟩ := starts operation member
    rcases edge with ⟨source, action, target⟩
    change action = operation.start at event
    subst action
    exact ⟨source, target, inGraph⟩
  · intro pending
    have yes := (any_pending_check_exact model model.initial).mpr pending
    simp [yes] at initial
  · intro operation member source state incoming
    have checkedEdge := outcomes operation member ⟨source, operation.start, state⟩ incoming
    simp only [↓reduceIte, Bool.and_eq_true, enabled_check_exact, List.all_eq_true,
      decide_eq_true_eq] at checkedEdge
    refine ⟨checkedEdge.1, checkedEdge.2.1, ?_⟩
    intro event target outgoing
    exact checkedEdge.2.2 ⟨state, event, target⟩ outgoing rfl
  · intro operation member state pending source event edge
    have checkedEdge := entries operation member ⟨source, event, state⟩ edge
    have inPending := (pending_check_exact model operation state).mpr pending
    simpa [inPending] using checkedEdge
  · intro operation member source event target edge outcome
    have checkedEdge := completions operation member ⟨source, event, target⟩ edge
    simp only [outcome, ↓reduceIte, Bool.and_eq_true] at checkedEdge
    refine ⟨(pending_check_exact model operation source).mp checkedEdge.1, ?_⟩
    intro pending
    have yes := (any_pending_check_exact model target).mpr pending
    simp [yes] at checkedEdge
  · intro left inLeft right inRight state pendingLeft pendingRight
    obtain ⟨source, incoming⟩ := pendingLeft
    have checkedEdge := owners left inLeft right inRight ⟨source, left.start, state⟩ incoming
    have yes := (pending_check_exact model right state).mpr pendingRight
    simpa [yes] using checkedEdge

/- Mandatory completion edges have unique destinations, and a pending state's
   only enabled events are those observed outcomes. This follows from the
   independent Contract relation plus determinism, not from `by decide` alone.
-/
theorem pending_has_unique_outcomes (model : Model) (checked : valid model = true)
    (operation : Operation) (member : operation ∈ model.operations)
    (state : Nat) (pending : Pending model operation state) :
    (∃ successTarget, Transition model state operation.success successTarget ∧
      ∀ other, Transition model state operation.success other → other = successTarget) ∧
    (∃ failureTarget, Transition model state operation.failure failureTarget ∧
      ∀ other, Transition model state operation.failure other → other = failureTarget) := by
  have contract := valid_implies_contract model checked
  obtain ⟨source, incoming⟩ := pending
  have outcomes := contract.outcomes operation member source state incoming
  obtain ⟨successTarget, successEdge⟩ := outcomes.1
  obtain ⟨failureTarget, failureEdge⟩ := outcomes.2.1
  exact ⟨⟨successTarget, successEdge, fun other edge => contract.deterministic _ _ _ _ edge successEdge⟩,
    ⟨failureTarget, failureEdge, fun other edge => contract.deterministic _ _ _ _ edge failureEdge⟩⟩

theorem pending_rejects_other_events (model : Model) (checked : valid model = true)
    (operation : Operation) (member : operation ∈ model.operations)
    (state event : Nat) (pending : Pending model operation state)
    (notSuccess : event ≠ operation.success) (notFailure : event ≠ operation.failure) :
    ¬ Enabled model state event := by
  obtain ⟨source, incoming⟩ := pending
  intro enabled
  obtain ⟨target, outgoing⟩ := enabled
  rcases (valid_implies_contract model checked).outcomes operation member source state incoming |>.2.2 event target outgoing with success | failure
  · exact notSuccess success
  · exact notFailure failure

theorem pending_entry_is_owned (model : Model) (checked : valid model = true)
    (operation : Operation) (member : operation ∈ model.operations)
    (state : Nat) (pending : Pending model operation state)
    (source event : Nat) (edge : Transition model source event state) : event = operation.start :=
  (valid_implies_contract model checked).entries operation member state pending source event edge

theorem completion_cannot_leave_pending (model : Model) (checked : valid model = true)
    (operation : Operation) (member : operation ∈ model.operations)
    (source event target : Nat) (edge : Transition model source event target)
    (outcome : event = operation.success ∨ event = operation.failure) :
    Pending model operation source ∧ ¬ AnyPending model target :=
  (valid_implies_contract model checked).completions operation member source event target edge outcome

end SGD.DomainProtocol

#print axioms SGD.DomainProtocol.valid_implies_contract
#print axioms SGD.DomainProtocol.lookup_transition_sound
#print axioms SGD.DomainProtocol.lookup_none_iff_no_transition
#print axioms SGD.DomainProtocol.pending_has_unique_outcomes
#print axioms SGD.DomainProtocol.pending_rejects_other_events
#print axioms SGD.DomainProtocol.pending_entry_is_owned
#print axioms SGD.DomainProtocol.completion_cannot_leave_pending
