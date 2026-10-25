# Generated retained-operation Rust boundary

Call `finalize(payload, |payload| write_report(payload))`. The writer receives `&T` and returns `Result<R, E>`. The generated boundary owns the original payload before the callback and returns it on every ordinary success, failure, or admission refusal. The model's start action gates callback admission. The actual callback result chooses the success or failure action. Callers cannot send those events through this API.

`Finalized<T, R, E>` has private fields. Inspect `payload()`, `status()`, and `snapshot()`. `FinalizationStatus::Completed(Err(error))` preserves the report failure separately from execution and persistence facts in the payload. A refusal invokes no callback. An unexpected completion-transition rejection retains the observed callback outcome. `into_parts()` explicitly transfers all three parts to the caller.

Keep this generated crate as a separate dependency. Safe Rust prevents the writer from moving ordinary owned data through its shared borrow. Interior mutability, panic, abort, process termination, caller drop or forget, and changed generated code remain outside this contract. The callback's success does not prove durable storage. Payload remains retained even on success. This finite lower contract does not prove refinement of an enclosing model, source coverage, or application-wide preservation.
