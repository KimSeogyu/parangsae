import DomainProtocol
import RetainedProtocol


set_option maxRecDepth 65536
set_option maxHeartbeats 2000000
namespace SGD.GeneratedDomain
open SGD.DomainProtocol

def model : Model := {
  stateCount := 4
  eventCount := 4
  initial := 1
  edges := [
    ⟨0, 3, 0⟩,
    ⟨1, 2, 2⟩,
    ⟨2, 0, 0⟩,
    ⟨2, 1, 3⟩,
    ⟨3, 3, 3⟩]
  operations := [
    ⟨2, 1, 0⟩]
}

theorem protocol_valid : valid model = true := by decide

theorem protocol_contract : Contract model := valid_implies_contract model protocol_valid

theorem lookup_is_transition (source event target : Nat)
    (found : lookupEdge model.edges source event = some target) :
    Transition model source event target := lookup_transition_sound model source event target found

theorem lookup_refusal_exact (source event : Nat) :
    lookupEdge model.edges source event = none ↔ ¬ ∃ target, Transition model source event target :=
  lookup_none_iff_no_transition model source event

theorem pending_outcomes (operation : Operation) (member : operation ∈ model.operations)
    (state : Nat) (pending : Pending model operation state) :
    (∃ successTarget, Transition model state operation.success successTarget ∧
      ∀ other, Transition model state operation.success other → other = successTarget) ∧
    (∃ failureTarget, Transition model state operation.failure failureTarget ∧
      ∀ other, Transition model state operation.failure other → other = failureTarget) :=
  pending_has_unique_outcomes model protocol_valid operation member state pending

theorem pending_events (operation : Operation) (member : operation ∈ model.operations)
    (state event : Nat) (pending : Pending model operation state)
    (notSuccess : event ≠ operation.success) (notFailure : event ≠ operation.failure) :
    ¬ Enabled model state event :=
  pending_rejects_other_events model protocol_valid operation member state event pending notSuccess notFailure

theorem pending_entries (operation : Operation) (member : operation ∈ model.operations)
    (state : Nat) (pending : Pending model operation state)
    (source event : Nat) (edge : Transition model source event state) : event = operation.start :=
  pending_entry_is_owned model protocol_valid operation member state pending source event edge

theorem completion_targets (operation : Operation) (member : operation ∈ model.operations)
    (source event target : Nat) (edge : Transition model source event target)
    (outcome : event = operation.success ∨ event = operation.failure) :
    Pending model operation source ∧ ¬ AnyPending model target :=
  completion_cannot_leave_pending model protocol_valid operation member source event target edge outcome

def resident (state : Nat) : Prop := state ∈ [0, 1, 2, 3]
instance (state : Nat) : Decidable (resident state) := by unfold resident; infer_instance

theorem resident_initial : resident model.initial := by decide

theorem resident_edge_target (source event target : Nat) (edge : Transition model source event target) : resident target := by
  have all : model.edges.all (fun entry => decide (resident entry.target)) = true := by decide
  have targetChecked := (List.all_eq_true.mp all) ⟨source, event, target⟩ edge
  exact of_decide_eq_true targetChecked

theorem retained_owner_preserves_residency_and_payload {Payload E O : Type}
    (payload : Payload) {owner : SGD.RetainedProtocol.Owner Payload}
    (history : SGD.RetainedProtocol.OwnerReach (E := E) (O := O) model
      (SGD.RetainedProtocol.create model payload) owner) :
    resident owner.state ∧ (SGD.RetainedProtocol.extract owner).1 = payload :=
  SGD.RetainedProtocol.owner_history_preserves_model_and_payload model payload resident resident_initial
    (fun source event target _ edge => resident_edge_target source event target edge) history


end SGD.GeneratedDomain
#print axioms SGD.GeneratedDomain.protocol_valid
#print axioms SGD.GeneratedDomain.protocol_contract
#print axioms SGD.GeneratedDomain.lookup_is_transition
#print axioms SGD.GeneratedDomain.lookup_refusal_exact
#print axioms SGD.GeneratedDomain.pending_outcomes
#print axioms SGD.GeneratedDomain.pending_events
#print axioms SGD.GeneratedDomain.pending_entries
#print axioms SGD.GeneratedDomain.completion_targets
#print axioms SGD.GeneratedDomain.resident_initial
#print axioms SGD.GeneratedDomain.resident_edge_target
#print axioms SGD.GeneratedDomain.retained_owner_preserves_residency_and_payload
