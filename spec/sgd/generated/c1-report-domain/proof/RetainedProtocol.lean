import DomainProtocol

/- A closed, synchronous reference owner. The payload token exists when the
   owner is constructed, and remains in every owner along an execution. Lean's
   extraction is a mathematical projection; Lean does not enforce linear
   consumption. The callback is pure in this model. Native Rust correspondence, payload
   contents under interior mutation, panic, external effects, persistence,
   cancellation, and caller disposal are separate obligations. -/
namespace SGD.RetainedProtocol
open SGD.DomainProtocol

inductive Observation (E O : Type)
  | refused
  | completed (event : Nat) (outcome : Except E O)
  | completionRejected (event : Nat) (outcome : Except E O)

structure Owner (Payload : Type) where
  state : Nat
  payload : Payload

structure Result (Payload E O : Type) where
  owner : Owner Payload
  observation : Observation E O

def create {Payload : Type} (model : Model) (payload : Payload) : Owner Payload :=
  ⟨model.initial, payload⟩

def extract {Payload : Type} (owner : Owner Payload) : Payload × Nat :=
  (owner.payload, owner.state)

def completionEvent {E O : Type} (operation : Operation) : Except E O → Nat
  | .ok _ => operation.success
  | .error _ => operation.failure

def invoke {Payload E O : Type} (model : Model) (operation : Operation)
    (owner : Owner Payload) (capacity : Bool) (port : Payload → Except E O) :
    Result Payload E O :=
  if !capacity then ⟨owner, .refused⟩
  else match lookupEdge model.edges owner.state operation.start with
    | none => ⟨owner, .refused⟩
    | some pending =>
      let outcome := port owner.payload
      let event := completionEvent operation outcome
      match lookupEdge model.edges pending event with
      | none => ⟨⟨pending, owner.payload⟩, .completionRejected event outcome⟩
      | some target => ⟨⟨target, owner.payload⟩, .completed event outcome⟩

theorem ordinary_return_retains_owner_payload {Payload E O : Type}
    (model : Model) (operation : Operation) (owner : Owner Payload) (capacity : Bool)
    (port : Payload → Except E O) :
    (invoke model operation owner capacity port).owner.payload = owner.payload := by
  simp only [invoke]
  split
  · rfl
  · split
    · rfl
    · split <;> rfl

theorem refusal_is_before_port {Payload E O : Type}
    (model : Model) (operation : Operation) (owner : Owner Payload) (capacity : Bool)
    (port : Payload → Except E O)
    (refused : (invoke model operation owner capacity port).observation = .refused) :
    capacity = false ∨ lookupEdge model.edges owner.state operation.start = none := by
  cases capacity with
  | false => exact Or.inl rfl
  | true =>
    right
    cases found : lookupEdge model.edges owner.state operation.start with
    | none => rfl
    | some pending =>
      simp only [invoke, Bool.not_true, Bool.false_eq_true, ↓reduceIte, found] at refused
      split at refused <;> contradiction

theorem refused_result_does_not_depend_on_port {Payload E O : Type}
    (model : Model) (operation : Operation) (owner : Owner Payload) (capacity : Bool)
    (first second : Payload → Except E O)
    (refused : (invoke model operation owner capacity first).observation = .refused) :
    invoke model operation owner capacity first = invoke model operation owner capacity second := by
  rcases refusal_is_before_port model operation owner capacity first refused with noCapacity | noStart
  · simp [invoke, noCapacity]
  · cases capacity <;> simp [invoke, noStart]

theorem observed_completion_uses_actual_port_result {Payload E O : Type}
    (model : Model) (operation : Operation) (owner : Owner Payload) (capacity : Bool)
    (port : Payload → Except E O) (event : Nat) (outcome : Except E O)
    (observed : (invoke model operation owner capacity port).observation = .completed event outcome ∨
      (invoke model operation owner capacity port).observation = .completionRejected event outcome) :
    outcome = port owner.payload ∧ event = completionEvent operation (port owner.payload) := by
  simp only [invoke] at observed
  split at observed
  · simp at observed
  · split at observed
    · simp at observed
    · split at observed <;> simp_all
      all_goals rw [← observed.2]; first | exact observed.1 | exact observed.1.symm

theorem invoke_preserves_graph_reachability {Payload E O : Type}
    (model : Model) (operation : Operation) (owner : Owner Payload) (capacity : Bool)
    (port : Payload → Except E O) (reachable : Reachable model owner.state) :
    Reachable model (invoke model operation owner capacity port).owner.state := by
  simp only [invoke]
  split
  · exact reachable
  · split
    · exact reachable
    · rename_i pending found
      have pendingReach : Reachable model pending :=
        .step reachable (lookup_transition_sound model owner.state operation.start pending found)
      split
      · exact pendingReach
      · rename_i target completed
        exact .step pendingReach (lookup_transition_sound model pending
          (completionEvent operation (port owner.payload)) target completed)

inductive OwnerStep {Payload E O : Type} (model : Model) : Owner Payload → Owner Payload → Prop
  | operation (operation : Operation) (owner : Owner Payload) (capacity : Bool)
      (port : Payload → Except E O) :
      OwnerStep model owner (invoke model operation owner capacity port).owner
  | ordinary (owner : Owner Payload) (event target : Nat)
      (edge : Transition model owner.state event target) :
      OwnerStep model owner ⟨target, owner.payload⟩

inductive OwnerReach {Payload E O : Type} (model : Model) : Owner Payload → Owner Payload → Prop
  | start (initial : Owner Payload) : OwnerReach model initial initial
  | advance {initial before after : Owner Payload} : OwnerReach model initial before →
      OwnerStep (E := E) (O := O) model before after → OwnerReach model initial after

theorem owner_step_preserves_payload {Payload E O : Type} (model : Model)
    {before after : Owner Payload} (step : OwnerStep (E := E) (O := O) model before after) :
    after.payload = before.payload := by
  cases step <;> simp_all [ordinary_return_retains_owner_payload]

theorem owner_step_preserves_graph_reachability {Payload E O : Type} (model : Model)
    {before after : Owner Payload} (step : OwnerStep (E := E) (O := O) model before after)
    (reachable : Reachable model before.state) : Reachable model after.state := by
  cases step with
  | operation operation owner capacity port =>
      exact invoke_preserves_graph_reachability model operation _ capacity port reachable
  | ordinary owner event target edge => exact .step reachable edge

theorem owner_history_preserves_payload_until_extraction {Payload E O : Type}
    (model : Model) (payload : Payload) {owner : Owner Payload}
    (history : OwnerReach (E := E) (O := O) model (create model payload) owner) :
    (extract owner).1 = payload := by
  induction history with
  | start => rfl
  | advance history step ih =>
      exact (owner_step_preserves_payload model step).trans ih

theorem owner_history_preserves_model_and_payload {Payload E O : Type}
    (model : Model) (payload : Payload) (invariant : Nat → Prop)
    (initial : invariant model.initial)
    (closed : ∀ source event target, invariant source →
      Transition model source event target → invariant target)
    {owner : Owner Payload}
    (history : OwnerReach (E := E) (O := O) model (create model payload) owner) :
    invariant owner.state ∧ (extract owner).1 = payload := by
  have graphReach : Reachable model owner.state := by
    induction history with
    | start => exact .initial
    | advance history step ih => exact owner_step_preserves_graph_reachability model step ih
  exact ⟨reachable_preserves_invariant model invariant initial closed graphReach,
    owner_history_preserves_payload_until_extraction model payload history⟩

def exampleModel : Model :=
  ⟨3, 3, 0, [⟨0, 0, 1⟩, ⟨1, 1, 2⟩, ⟨1, 2, 0⟩], [⟨0, 1, 2⟩]⟩
def exampleOperation : Operation := ⟨0, 1, 2⟩

theorem examples_exercise_success_failure_refusal_and_completion_rejection :
    (invoke exampleModel exampleOperation (create exampleModel 42) true
      (fun _ => Except.ok (7 : Nat) : Nat → Except Unit Nat)).observation = .completed 1 (.ok 7) ∧
    (invoke exampleModel exampleOperation (create exampleModel 42) true
      (fun _ => Except.error () : Nat → Except Unit Nat)).observation = .completed 2 (.error ()) ∧
    (invoke exampleModel exampleOperation (create exampleModel 42) false
      (fun _ => Except.ok (7 : Nat) : Nat → Except Unit Nat)).observation = .refused ∧
    (invoke { exampleModel with edges := [⟨0, 0, 1⟩] } exampleOperation (create exampleModel 42) true
      (fun _ => Except.ok (7 : Nat) : Nat → Except Unit Nat)).observation = .completionRejected 1 (.ok 7) := by
  exact ⟨rfl, rfl, rfl, rfl⟩

end SGD.RetainedProtocol

#print axioms SGD.RetainedProtocol.ordinary_return_retains_owner_payload
#print axioms SGD.RetainedProtocol.refusal_is_before_port
#print axioms SGD.RetainedProtocol.refused_result_does_not_depend_on_port
#print axioms SGD.RetainedProtocol.observed_completion_uses_actual_port_result
#print axioms SGD.RetainedProtocol.invoke_preserves_graph_reachability
#print axioms SGD.RetainedProtocol.owner_step_preserves_payload
#print axioms SGD.RetainedProtocol.owner_step_preserves_graph_reachability
#print axioms SGD.RetainedProtocol.owner_history_preserves_payload_until_extraction
#print axioms SGD.RetainedProtocol.owner_history_preserves_model_and_payload
#print axioms SGD.RetainedProtocol.examples_exercise_success_failure_refusal_and_completion_rejection
