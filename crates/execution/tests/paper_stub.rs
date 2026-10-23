use parangsae_core::Qty;
use parangsae_execution::{
    Attempt, ClientOrderId, IntentId, OrderState, PaperOrder, PaperVenue, Reservation, VenueError,
};

#[test]
fn paper_stub_refuses_definitively() {
    let venue = PaperVenue::new();
    let id = ClientOrderId::from_parts(IntentId::from_raw(1), Attempt::FIRST);
    let order = PaperOrder::new(id.clone(), Qty::from_ticks(2).expect("qty")).expect("order");
    assert_eq!(order.reservation(), Reservation::Held);
    assert_eq!(order.qty().ticks(), 2);
    match venue.submit(order) {
        Ok(ack) => match ack {},
        Err(refusal) => {
            assert_eq!(refusal.error, VenueError::PaperNotWired);
            assert_eq!(refusal.state, OrderState::Rejected);
            assert_eq!(refusal.reservation, Reservation::Released);
        }
    }
    match venue.sim_key(&id) {
        Ok(key) => match key {},
        Err(error) => assert_eq!(error, VenueError::PaperNotWired),
    }
}

#[test]
fn paper_order_rejects_zero_qty() {
    let id = ClientOrderId::from_parts(IntentId::from_raw(1), Attempt::FIRST);
    let zero = Qty::from_ticks(0).expect("zero");
    assert!(PaperOrder::new(id, zero).is_err());
}
