//! Fixed execution/statistics settings are admitted before any DATA hash.

use serde_json::{json, Value};

use super::admission::C1AdmissionError;
use crate::{fold_schedule, Fold, ScheduleSpec, UtcDate};

pub(super) fn schedule() -> Result<Vec<Fold>, C1AdmissionError> {
    let date = |text| UtcDate::parse(text).ok_or_else(|| refusal("native calendar date"));
    fold_schedule(&ScheduleSpec {
        sample_start: date("2020-12-18")?,
        holdout_start: date("2025-10-01")?,
        warmup_days: 3,
        train_days: 365,
        purge_days: 1,
        test_days: 90,
        step_days: 90,
        remainder_own_fold_days: 45,
    })
    .map_err(|error| refusal(&format!("native calendar: {error}")))
}

fn refusal(path: &str) -> C1AdmissionError {
    C1AdmissionError {
        code: "RUNTIME_SETTINGS",
        detail: format!("selected native contract differs at {path}"),
    }
}

pub(super) fn validate_runtime_settings(actual: &Value) -> Result<(), C1AdmissionError> {
    // This binds the selected native profile; parsing this data grants no run permission.
    // Source profile names/calendars have one separate Data-owned validator.
    let expected = json!({
      "scope": {
        "family": "C1",
        "development_start_us": 1608249600000000,
        "development_end_exclusive_us": 1759276800000000,
        "holdout_start_us": 1759276800000000,
        "source_route": "CONDITIONAL_FUNDING_OPEN_PROXY_IRREGULAR_SPOT_V1",
        "C1_mark_gap_carry": false
      },
      "horizons_h": [
        168,
        336
      ],
      "clock": {
        "units": "UTC_MICROSECONDS",
        "command_grid_us": 1000,
        "entry_decision_period_us": 3600000000,
        "entry_decision_offset_us": 1000,
        "risk_observe_period_us": 60000000,
        "risk_observe_offset_us": 1000,
        "actor_delivery": "CEIL_TO_1MS_EFFECTIVE_AVAILABLE_NO_EXTRA_US",
        "dependent_decision": "NEXT_1MS_STRICTLY_AFTER_COMPLETE_CUT",
        "risk_required_when": [
          "ACTUAL_POSITION",
          "NONTERMINAL_ORDER",
          "CAPTURED_UNSETTLED_FUNDING_OR_OTHER_CONTINGENT_LIABILITY",
          "NONIDLE_NATIVE_PHASE"
        ],
        "normal_source_event_max_age_us": 120000000,
        "required_latest_expected_key": true,
        "entry_aligned_trade_open_key": "UTC_HOUR_MINUS_60_SECONDS",
        "daily_final_open_key": "UTC_SESSION_END_MINUS_60_SECONDS",
        "daily_report_knowledge_offset_us": 1000,
        "daily_economic_prefix": "EVENT_TIME_STRICTLY_BEFORE_SESSION_END",
        "source_received": "ORIGINAL_ACQUISITION_METADATA_PRESERVED",
        "modeled_received": "TAGGED_SYNTHETIC_CEIL_MS_DELIVERY",
        "retained_pair_some_implies_active": false,
        "genuinely_settled_stopped_cash": "KEEP_STOPPED_NO_UNUSED_QUOTES_OR_FAKE_Q0_CAPTURE"
      },
      "opportunity": {
        "generated_control_owner": "ONE_PRIVATE_BACKTEST_SGDV2_OPPORTUNITY_CONTROL",
        "signal_payload_owner": "CAUSAL_RATE_WINDOW_AND_KEY_CLASSIFICATION_NO_ARMED_FLAG",
        "feature_rows": 9,
        "feature_key_span_us": 230400000000,
        "projection_normalization_hours": 72,
        "latest_rate_positive": true,
        "basis_nonnegative": true,
        "projection": "R9_TIMES_H_DIVIDED_BY_72_EXACT_RATIONAL",
        "entry_comparison": "NET_Q_TIMES_CAUSAL_PERP_REFERENCE_TIMES_PROJECTION_STRICTLY_GREATER_THAN_EXACT_HEADLINE_C4",
        "opportunity_cost_estimator": "HEADLINE",
        "batch_admit_before_latest_key": true,
        "entry_event_identity": "LATEST_SETTLEMENT_KEY_ONCE_PER_CONFIG_WITH_PRESERVED_CONSUMPTION",
        "initial_mode": "AWAIT_FALSE",
        "epoch_activation_requires": "HEALTHY_FULLY_SETTLED_IDLE",
        "epoch_preserves": [
          "CASH",
          "ORIGINAL_LOSS_ANCHOR",
          "ALL_ORDERS_FILLS_FUNDING",
          "SOURCE_WATERMARKS",
          "CONSUMED_KEYS",
          "STOPPED_LATCH"
        ],
        "known_false_arms_only_while_healthy_idle": true,
        "temporary_unavailable": "PENDING_UNTIL_FIRST_COMPLETE_OR_NEW_KEY",
        "definitive_capital_grid_refusal": "CONSUME_KEY_NOT_FALSE",
        "source_discontinuity": "AWAIT_FALSE_OR_TECHNICAL_IF_CONTRACT_VIOLATION",
        "emit_consumes_and_disarms": true,
        "occupied_keys": "CONSUMED_NO_REARM",
        "after_spot_opportunity_worsens": "FINISH_SAFE_HEDGE_WITH_NEW_CAUSAL_AUTHORITY_BEFORE_ORIGINAL_DEADLINE",
        "max_exit_poll_without_new_rate": true,
        "early_exit_latest_rate_lte_zero": true,
        "post_captured_payment_before_economic_exit": true,
        "hold_anchor": "OBSERVED_HEDGED_AT",
        "report_physical_completion_and_lag": true,
        "C4_state_basis": "READ_ONLY_CURRENT_SCENARIO_HEALTHY_IDLE_ACCOUNT_REALIZED_PERP_SCALED_RESIDUE",
        "C4_perp_cash_delta": "DIV_FLOOR_REALIZED_SCALED_PLUS_NET_Q_TIMES_HEADLINE_SELL_MINUS_BUY_BY_SCALE_MINUS_CURRENT_REALIZED_PERP",
        "C4_hypothetical_costs": "EXACT_SPOT_GROSS_TO_FEE_NET_ROUND_TRIP_AND_BOTH_PERP_HEADLINE5BPS_FEES_WITH_HEADLINE2BPS_PRICES_NO_FUTURE_VALUES_NO_STATE_MUTATION",
        "C4_isolated_reset_or_ceil_estimate": false,
        "native_entry_epoch_start": "ACTUAL_HEALTHY_FULLY_SETTLED_IDLE_POLICY_OR_SOURCE_NEWEPOCH_ACTIVATION_UTC_CLOCK",
        "entry_rearm_effective_key_gate": "S_GREATER_THAN_OR_EQUAL_NATIVE_ENTRY_EPOCH_START_AND_STRICTLY_GREATER_THAN_LAST_CONSUMED",
        "pre_epoch_unconsumed_seen": "KEEP_SUPPRESSED_WITH_NO_GENERATED_EVENT_NOT_REPLAY_NOT_NO_SIZE_NOT_FAKE_CONSUME",
        "pre_epoch_rate_history": "RETAIN_AS_CAUSAL_R9_HISTORY_EPOCH_GATE_ONLY_ENTRY_AND_REARM_NOT_CASH_OR_REQUIRED_EXIT",
        "NewEpoch_advances_consumed": false,
        "midnight_S_equal_epoch": "ELIGIBLE_ONLY_AFTER_ACTUAL_RATE_AND_REQUIRED_INPUT_AVAILABILITY"
      },
      "capital": {
        "scale": 100000000,
        "initial_total_cash_ticks": 100000000000000,
        "spot_wallet_ticks": 47500000000000,
        "isolated_perp_wallet_ticks": 47500000000000,
        "idle_wallet_ticks": 5000000000000,
        "spot_close_designation_ticks": 2500000000000,
        "perp_close_designation_ticks": 500000000000,
        "perp_funding_designation_ticks": 2000000000000,
        "initial_each_leg_envelope_ticks": 45000000000000,
        "unhedged_notional_ceiling_ticks": 50000000000000,
        "idle_available_for_transfer": false,
        "perp_short_principal_credit": false,
        "borrow": false,
        "interest": false,
        "automatic_transfers": false,
        "profit_raises_initial_envelopes": false,
        "size_policy": "LARGEST_EXACT_COMPATIBLE_QUANTITY_WITHIN_CURRENT_ADMITTED_WALLETS_AND_INITIAL_ENVELOPES",
        "freeze_quantity_authority_at_dispatch": true,
        "matched_quantity_rebalance": false
      },
      "assumed_filters": {
        "spot_quantity_step_ticks": 1000,
        "perp_quantity_step_ticks": 100000,
        "gross_spot_max_qty_ticks": 10000000000,
        "net_perp_max_qty_ticks": 9990000000,
        "minimum_each_order_notional_cash_ticks": 10000000000,
        "gross_spot_grid_required": true,
        "net_spot_exit_grid_required": true,
        "net_perp_grid_required": true,
        "no_dust_erasure": true,
        "final_price_grid": "NATIVE_1E8_PRICE_ONLY_NO_001_USDT_FILTER",
        "classification": "CONDITIONAL_RESEARCH_ASSUMPTIONS_NOT_HISTORICAL_EXCHANGE_FILTERS",
        "minimum_notional_check_clock": "PLAN_AND_DISPATCH_ADMITTED_PER_LEG_CONTEXT_PRICE_PROXY_SIZER_DECISION_TRADE_AND_RISK_REFERENCES",
        "actual_executed_fill_minimum_notional_filter": false
      },
      "fees": {
        "spot_buy_bps": 10,
        "spot_buy_asset": "BTC",
        "spot_sell_bps": 10,
        "spot_sell_asset": "USDT",
        "perp_fee_asset": "USDT",
        "discount": false,
        "asset_rounding": "TRADER_ADVERSE_CHECKED_CEIL_TO_1E8_ASSET_TICK",
        "net_spot_formula": "GROSS_Q_TICKS_MINUS_CEIL_GROSS_Q_TICKS_TIMES_10_DIVIDED_BY_10000"
      },
      "execution": {
        "historical_proxy": "FULL_FILL_OR_PENDING_NO_CAPACITY_OR_PARTIAL_FREQUENCY_CLAIM",
        "fill_reference": "RAW_OPEN_OF_FIRST_FORWARD_ELIGIBLE_POSITIVE_COUNT_MINUTE_STRICTLY_AFTER_DISPATCH",
        "slippage": "EXISTING_NATIVE_FILL_PRICE_SINGLE_TRANSFORM_FROM_RAW_OPEN",
        "driver_band_lower": "CEIL_REFERENCE_TICKS_TIMES_9900_DIVIDED_BY_10000",
        "driver_band_upper": "FLOOR_REFERENCE_TICKS_TIMES_10100_DIVIDED_BY_10000",
        "native_max_price": "BAND_UPPER_ALL_SIDES",
        "band_classification": "MODELED_ELIGIBILITY_NOT_EXCHANGE_ORDER_TYPE",
        "entry_order": "SPOT_THEN_COMPLETE_CUT_THEN_NEW_HEDGE_DECISION",
        "exit_order": "PERP_THEN_COMPLETE_FLAT_CUT_THEN_NEW_SPOT_EXIT_DECISION",
        "opening_deadline_us": 600000000,
        "closing_deadline_us": 600000000,
        "normal_completion_by_deadline": "LESS_THAN_OR_EQUAL",
        "unsupported_or_unfinished_deadline": "WHOLE_TRIAL_TECHNICAL_NO_POSTDEADLINE_COMPENSATING_FILL",
        "funding_capture": "ACTUAL_PRE_SETTLEMENT_SHORT_RATE_FREE_BOUNDARY",
        "settlement_drain": "ADMITTED_UM_RECEIPTS_AND_COMPLETE_CUTS_BEFORE_BOUNDARY_S",
        "no_perp_fill_interval_around_nominal_settlement_us": 15000000,
        "ambiguous_boundary": "TECHNICAL_UNRESOLVED",
        "single_money_lifecycle_owner": "EXISTING_CARRY_ENGINE_AND_LEDGER",
        "order_replacement": "NONE_IN_FIRST_FULL_FILL_OR_PENDING_MODEL_NO_REPRICE_OR_AMEND",
        "session_ancillaries": {
          "mono_approval_age_max_us": 120000000,
          "execution_config_same_approval_age": true,
          "static_dispatch_timing_budget_us": 1000,
          "max_attempts": 1,
          "dormant_not_found_receive_window_ms": 5000,
          "dormant_not_found_margin_ms": 1000,
          "dormant_not_found_total_us": 6000000,
          "dispatch_and_plan_same_decision_clock": true,
          "scope": "STATIC_NO_NETWORK_MODEL_BUDGET_NOT_MEASURED_RTT_OR_BAR_FILL_RECEIPT_DEADLINE",
          "not_found_is_order_absence_or_zero_cancel_cut_evidence": false,
          "max_attempts_scope": "NATIVE_EXECUTION_MAXATTEMPTS_NOT_GLOBAL_TRIAL_OR_INVOCATION_COUNT"
        }
      },
      "risk": {
        "initial_margin_bps": 10000,
        "assumed_maintenance_bps": 100,
        "session_loss_bps_of_original_C0": 500,
        "session_loss_latch_reset_at_cycle_or_OOS_fold": false,
        "peak_drawdown_hard_gate": null,
        "sampled_mark_only": true,
        "intraminute_liquidation_claim": false,
        "margin_loss_trip": "STICKY_NO_NEW_ENTRY_SUPPORTED_KNOWN_REDUCING_CLOSE_ONLY",
        "classification": "MODEL_CONTROLS_NOT_USER_TOLERANCE_OR_EXCHANGE_TIERS_OR_LOSS_GUARANTEE"
      },
      "folds": {
        "feature_warmup_days": 3,
        "train_days": 365,
        "train_test_calendar_separation_days": 1,
        "test_days": 90,
        "step_days": 90,
        "remainder": "APPEND_REMAINDER_TO_PREVIOUS_FOLD_WHEN_LESS_THAN45_DAYS",
        "fold_count": 15,
        "OOS_days": 1379,
        "first_OOS_start_us": 1640131200000000,
        "last_OOS_end_exclusive_us": 1759276800000000,
        "train_accounts": "INDEPENDENT_C0_RESET_PER_WINDOW_CONFIG",
        "OOS_accounts": "ONE_CONTINUOUS_ACCOUNT_PER_COST_SCENARIO",
        "terminal_intent_before_end_us": 600000000,
        "terminal_requires_closed": "STRICTLY_BEFORE_FOLD_END_WITH_ALL_LIABILITIES_SETTLED",
        "per_fold_H_entry_ban": false,
        "global_entry_remaining_support_us": "OPENING_600_SECONDS_PLUS_H_PLUS_3600_SECONDS_PLUS_CLOSING_600_SECONDS_BEFORE_GLOBAL_TERMINAL",
        "account_reset_at_OOS_fold": false,
        "remainder_merge_threshold_days": 45
      },
      "selection": {
        "contract_id": "TRAIN_HEADLINE_POSITIVE_NET_SR_THEN_SHORTER_ELSE_FLAT_V1",
        "ordered_horizons_h": [
          168,
          336
        ],
        "cost": "HEADLINE",
        "eligible_train_sharpe": "STRICTLY_POSITIVE_DEFINED",
        "tie": "LOWER_GRID_INDEX",
        "otherwise": "FLAT_KEEP_ORIGINAL_ACCOUNT_CASH_AND_CALENDAR",
        "future_fold_choice_visible": "ONLY_AT_ITS_TEST_ACTIVATION",
        "per_config_OOS_curves": false
      },
      "cost_scenarios": {
        "HEADLINE": {
          "perp_fee_bps": 5,
          "slippage_bps_each_fill": 2,
          "necessary": true
        },
        "HIGHER_PERP_FEE": {
          "perp_fee_bps": 6,
          "slippage_bps_each_fill": 2,
          "necessary": true
        },
        "SLIPPAGE_STRESS": {
          "perp_fee_bps": 5,
          "slippage_bps_each_fill": 5,
          "necessary": false
        },
        "shared_spot_fee_bps": 10,
        "shared_entry_estimator": "HEADLINE_5_FEE_2_SLIP_IN_ALL_SCENARIOS",
        "selection_schedule": "HEADLINE_TRAIN_SCHEDULE_IN_ALL_SCENARIOS",
        "actual_state_paths": "INDEPENDENT_CONTINUOUS_SCENARIO_WALLETS_ACTUAL_COSTS_MAY_CHANGE_FEASIBILITY",
        "no_fixed_trade_path_claim": true
      },
      "statistics": {
        "cash_benchmark": "SAME_C0_ZERO_INTEREST_USDT_NO_RISK_FREE_CLAIM",
        "extra_traded_or_BTC_S01_benchmark": false,
        "daily_return": "E_SESSION_END_DIVIDED_BY_PRIOR_E_MINUS_ONE_SAME_CONTINUOUS_ACCOUNT",
        "initial_previous_E": "C0_AT_FIRST_OOS_START",
        "total_return": "E_FINAL_DIVIDED_BY_C0_MINUS_ONE",
        "cash_designations_not_added_twice": true,
        "Sharpe_ddof": 1,
        "Sharpe_annualization_days": 365,
        "calendar_T": 1379,
        "headline_net_return_must_be_positive": true,
        "headline_Sharpe_must_be_positive": true,
        "higher_fee_Sharpe_must_be_positive": true,
        "DSR_min_num": 95,
        "DSR_min_den": 100,
        "PBO_max_num": 1,
        "PBO_max_den": 5,
        "normal_matched_round_trips_min": 50,
        "active_test_folds_min": 10,
        "forced_risk_failed_partial_pairs_count_for_activity": false,
        "IS_matrix_rows": 1625,
        "IS_matrix_deoverlap": "FIRST_365_TRAIN_DAYS_THEN_LAST_90_NEW_DAYS_EACH_LATER_TRAIN",
        "CSCV_blocks": 16,
        "CSCV_splits": 12870,
        "CSCV_train_boundary_guard_days": 16,
        "CSCV_guard_scope": "CEIL_LOCAL_EXPOSURE_14D80MIN_TO15D_PLUS1DAY_EXPLICIT_GUARD_NOT_LOOKBACK_SUM_OR_INDEPENDENCE_PROOF",
        "DSR_ann_variance_floor_num": 1,
        "DSR_ann_variance_floor_den": 4,
        "DSR_variance_source": "TWO_HEADLINE_DEOVERLAPPED_TRAIN_COLUMN_SHARPES",
        "undefined_train_SR": "INELIGIBLE",
        "undefined_PBO_SR": "RANK_LAST",
        "PBO_rank_ties": "MIDRANKS",
        "PBO_rank_denominator": "K_PLUS_ONE",
        "PBO_event": "LAMBDA_LESS_THAN_OR_EQUAL_ZERO",
        "history_complete": false,
        "history_base_observed": 85,
        "history_documentary_charge": 13,
        "history_unknown_U": null,
        "history_N_floor": 202,
        "N_floor_is_exact_independent_or_global_N": false,
        "positive_ceiling": "INCONCLUSIVE_HISTORY",
        "technical_precedes_numeric_verdict": true,
        "valid_numeric_failure": "REJECT_ON_DECLARED_MODEL_SNAPSHOT",
        "coverage_only_failure": "INCONCLUSIVE_SPARSE_NO_PROMOTION",
        "bootstrap": "SELECTED_SERIES_DESCRIPTIVE_CIRCULAR_MOVING_BLOCK_MEAN_ONLY",
        "bootstrap_block_days": [
          28,
          56
        ],
        "bootstrap_resamples": 10000,
        "bootstrap_seed": 20261024,
        "bootstrap_percentile_quantile": "TYPE7_EQUAL_TAILED_95_PERCENT",
        "bootstrap_hard_gate": false,
        "cross_config_variance_ddof": 1,
        "undefined_column_SR_for_variance_ticks": 0,
        "moments": "EXISTING_INTEGER_POPULATION_CENTRAL_MOMENTS_NONEXCESS_KURTOSIS",
        "DSR_implementation": "EXISTING_BACKTEST_STATS_INTEGER_Q1E15_NORMAL_APPROX_TO1E8_PROBABILITY_TICKS",
        "DSR_native_gate_probability_ticks_min_inclusive": 95000000,
        "independent_true_CDF": "DIAGNOSTIC_NOT_POSTRESULT_GATE_SUBSTITUTION",
        "IS_matrix_overlap_policy": "EARLIEST_COVERING_DECLARED_TRAIN_WINDOW_THEN_EACH_LATER_NEW_CHRONOLOGICAL_SUFFIX_PREVIOUS_TRAIN_END",
        "IS_matrix_overlap_returns_equal_claim": false,
        "IS_matrix_row_provenance": "SOURCE_TRAIN_FOLD_DATE_AND_CONFIG_COLUMN_RETAINED",
        "CSCV_gap_call_literal": 16,
        "CSCV_gap_helper": "EXPLICIT_PBO_TICKS_GAP16_NOT_LEGACY_FAMILY_DEFAULT",
        "CSCV_IS_trimming": "NATIVE_IS_DAYS_TRIM_FIRST16_ROWS_IF_LEFT_NEIGHBOR_IS_OOS_TRIM_LAST16_IF_RIGHT_NEIGHBOR_IS_OOS_SATURATE_BLOCK_KEEP_HALF_OPEN_RANGE",
        "CSCV_OOS_trimming": "NONE",
        "CSCV_internal_adjacent_IS_boundary_trimming": "NONE"
      },
      "lifetime_bounds": {
        "source": "EXACT_CANONICAL_8H_SETTLEMENT_CALENDAR_NOT_OBSERVED_OPPORTUNITIES",
        "training_calendar_settlements_N": 1095,
        "continuous_OOS_calendar_settlements_N": 4137,
        "max_candidate_attempts": "N",
        "max_leg_orders_full_fill_baseline": "4_TIMES_N",
        "max_full_fill_receipts": "4_TIMES_N",
        "max_funding_identities": "N",
        "max_feature_rows": 9,
        "max_risk_observe_ticks_train": 525600,
        "max_risk_observe_ticks_OOS": 1985760,
        "max_hourly_signal_ticks_train": 8760,
        "max_hourly_signal_ticks_OOS": 33096,
        "synthetic_partial_fixture_max_receipts_per_order": 4,
        "cap_violation": "RETAINED_WHOLE_TRIAL_TECHNICAL_NO_HISTORY_PRUNING_OR_FRESH_ENGINE",
        "max_partial_script_receipts": "16_TIMES_N_WHEN_FOUR_RECEIPTS_PER_ORDER"
      },
      "execution_budget": {
        "candidate_roles": [
          "TRAIN_MAX168",
          "TRAIN_MAX336"
        ],
        "continuous_policy_roles": [
          "OOS_HEADLINE",
          "OOS_HIGHER_PERP_FEE",
          "OOS_SLIPPAGE_STRESS"
        ],
        "train_segments": 30,
        "continuous_segments": 3,
        "total_segments_if_complete": 33,
        "start_terminal_records_if_complete": 66,
        "conservative_initial_charged_identities_max": 5,
        "native_invocations_max": 1,
        "automatic_retry": false,
        "H4_counting_unchanged": true
      }
    });
    for (section, expected) in expected
        .as_object()
        .ok_or_else(|| refusal("native profile"))?
    {
        let Some(value) = actual.get(section) else {
            return Err(refusal(section));
        };
        if section == "folds" {
            let mut scalars = value.as_object().cloned().ok_or_else(|| refusal("folds"))?;
            scalars.remove("schedule");
            if Value::Object(scalars) != *expected {
                return Err(refusal(section));
            }
        } else if value != expected {
            return Err(refusal(section));
        }
    }
    let declared = actual
        .pointer("/folds/schedule")
        .and_then(Value::as_array)
        .ok_or_else(|| refusal("folds.schedule"))?;
    let native = schedule()?;
    if declared.len() != native.len() {
        return Err(refusal("folds.schedule count"));
    }
    for (row, fold) in declared.iter().zip(native) {
        let expected = json!({
            "fold": fold.index,
            "test_days": (fold.test.end.micros() - fold.test.start.micros()) / 86_400_000_000_i64,
            "train_start_us": fold.train.start.micros(),
            "train_end_exclusive_us": fold.train.end.micros(),
            "test_start_us": fold.test.start.micros(),
            "test_end_exclusive_us": fold.test.end.micros(),
            "train_terminal_intent_us": fold.train.end.micros() - 600_000_000_i64,
            "test_terminal_intent_us": fold.test.end.micros() - 600_000_000_i64,
        });
        for (key, value) in expected.as_object().ok_or_else(|| refusal("native fold"))? {
            if row.get(key) != Some(value) {
                return Err(refusal(&format!("folds.schedule.{key}")));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_selected_settings_and_calendar_match_the_native_binding() {
        let value: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/c1/runtime-settings.json"
        ))
        .unwrap();
        validate_runtime_settings(&value).unwrap();
        let folds = schedule().unwrap();
        assert_eq!(folds.len(), 15);
        assert_eq!(
            folds[0].train.start,
            UtcDate::parse("2020-12-21").unwrap().start_us()
        );
        assert_eq!(
            folds[0].test.start,
            UtcDate::parse("2021-12-22").unwrap().start_us()
        );
        assert_eq!(
            (folds.last().unwrap().test.end.micros() - folds[0].test.start.micros())
                / 86_400_000_000_i64,
            1379
        );
    }

    #[test]
    fn self_consistent_hashes_cannot_change_runtime_capital_costs_or_calendar() {
        let original: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/c1/runtime-settings.json"
        ))
        .unwrap();
        for path in [
            "/capital/initial_total_cash_ticks",
            "/risk/session_loss_bps_of_original_C0",
            "/cost_scenarios/HEADLINE/perp_fee_bps",
            "/clock/entry_decision_offset_us",
            "/statistics/CSCV_gap_call_literal",
            "/folds/schedule/0/train_end_exclusive_us",
        ] {
            let mut value = original.clone();
            *value.pointer_mut(path).unwrap() = json!(1);
            assert_eq!(
                validate_runtime_settings(&value).unwrap_err().code,
                "RUNTIME_SETTINGS"
            );
        }
    }
}
