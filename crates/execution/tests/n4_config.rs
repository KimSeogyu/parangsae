use parangsae_execution::{
    ApprovalTiming, ConfigError, Elapsed, ExecutionConfig, MaxAttempts, NotFoundAuthority,
    PositiveMicros,
};

#[test]
fn n4_config_rejects_ttl_not_above_dispatch_latency() {
    let ten = PositiveMicros::new(10).expect("10");
    let ten_again = PositiveMicros::new(10).expect("10");
    let five = PositiveMicros::new(5).expect("5");
    let nine = PositiveMicros::new(9).expect("9");
    assert_eq!(
        ApprovalTiming::new(ten, ten_again),
        Err(ConfigError::ApprovalTtlNotAboveDispatchLatency)
    );
    assert_eq!(
        ApprovalTiming::new(five, ten),
        Err(ConfigError::ApprovalTtlNotAboveDispatchLatency)
    );
    let timing = ApprovalTiming::new(ten, nine).expect("age above latency");
    assert_eq!(timing.max_approval_age().get(), 10);
    assert_eq!(timing.max_dispatch_latency().get(), 9);
}

#[test]
fn n4_config_rejects_approval_age_mismatch() {
    let timing = ApprovalTiming::new(
        PositiveMicros::new(10).expect("age"),
        PositiveMicros::new(4).expect("latency"),
    )
    .expect("timing");
    let authority = NotFoundAuthority::new(1_000, 1).expect("authority");
    let attempts = MaxAttempts::new(2).expect("attempts");
    assert_eq!(
        ExecutionConfig::new(
            timing,
            authority,
            attempts,
            PositiveMicros::new(11).expect("other age")
        ),
        Err(ConfigError::ApprovalAgeMismatch)
    );
    let config = ExecutionConfig::new(
        timing,
        authority,
        attempts,
        PositiveMicros::new(10).expect("same age"),
    )
    .expect("config");
    assert_eq!(config.timing().max_approval_age().get(), 10);
    assert_eq!(config.max_attempts().get(), 2);
    assert_eq!(config.authority().bound_us(), 1_001_000);
}

#[test]
fn not_found_authority_rejects_bad_windows() {
    assert_eq!(
        NotFoundAuthority::new(0, 1),
        Err(ConfigError::RecvWindowOutOfRange)
    );
    assert_eq!(
        NotFoundAuthority::new(60_001, 1),
        Err(ConfigError::RecvWindowOutOfRange)
    );
    assert_eq!(
        NotFoundAuthority::new(1, 0),
        Err(ConfigError::MarginOutOfRange)
    );
    assert_eq!(
        NotFoundAuthority::new(60_000, u32::MAX),
        Err(ConfigError::BoundOverflow)
    );
    let authority = NotFoundAuthority::new(1, 1).expect("authority");
    assert_eq!(authority.bound_us(), 2_000);
    assert!(!authority.is_authoritative(Elapsed::from_micros(2_000)));
    assert!(authority.is_authoritative(Elapsed::from_micros(2_001)));
}

#[test]
fn max_attempts_is_bounded() {
    assert_eq!(MaxAttempts::new(0), Err(ConfigError::MaxAttemptsOutOfRange));
    assert_eq!(
        MaxAttempts::new(17),
        Err(ConfigError::MaxAttemptsOutOfRange)
    );
    assert_eq!(MaxAttempts::new(1).expect("one").last_attempt().index(), 0);
    assert_eq!(MaxAttempts::new(16).expect("sixteen").get(), 16);
    assert_eq!(PositiveMicros::new(0), Err(ConfigError::NonPositiveMicros));
}
