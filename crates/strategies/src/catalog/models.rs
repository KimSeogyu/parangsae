//! Bounded causal model examples, not learned-market or execution authority.
//! Integer scores and coefficients avoid platform-dependent float fitting.

use parangsae_core::{Causal, DecisionTime, Symbol};
use parangsae_signals::CausalWindow;

const SCORE_LIMIT: i64 = 1_000_000;
const MAX_ASSETS: usize = 128;
const MAX_TRAINING: usize = 128;
const COEFFICIENT_SCALE: i128 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelError {
    InvalidInput,
    AfterDecision,
    TrainingCutoff,
    Duplicate,
    Capacity,
    Degenerate,
    Stale,
    Overflow,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelDirection {
    Long,
    Short,
    Neutral,
}
fn direction(score: i64, threshold: i64) -> ModelDirection {
    if score > threshold {
        ModelDirection::Long
    } else if score < -threshold {
        ModelDirection::Short
    } else {
        ModelDirection::Neutral
    }
}
fn valid_score(score: i64) -> bool {
    (-SCORE_LIMIT..=SCORE_LIMIT).contains(&score)
}
fn visible<T>(view: &Causal<'_, T>, decision: DecisionTime) -> Result<(), ModelError> {
    if view.available_ts().micros() > decision.micros() {
        Err(ModelError::AfterDecision)
    } else {
        Ok(())
    }
}

/// A supplied past-fit two-factor loading, in coefficient basis points.
#[derive(Clone, Debug)]
pub struct FactorAsset {
    symbol: Symbol,
    intercept: i64,
    loadings: [i64; 2],
}
impl FactorAsset {
    pub fn new(symbol: Symbol, intercept: i64, loadings: [i64; 2]) -> Result<Self, ModelError> {
        if !valid_score(intercept) || loadings.iter().any(|x| !valid_score(*x)) {
            return Err(ModelError::InvalidInput);
        }
        Ok(Self {
            symbol,
            intercept,
            loadings,
        })
    }
}
/// Causally published loadings must precede the assessed snapshot. The caller
/// supplies their training provenance; constructor timestamps do not prove it.
#[derive(Clone, Debug)]
pub struct FactorModel {
    trained_through: DecisionTime,
    assets: Vec<FactorAsset>,
}
impl FactorModel {
    pub fn new(
        trained_through: DecisionTime,
        assets: Vec<FactorAsset>,
    ) -> Result<Self, ModelError> {
        if !(3..=MAX_ASSETS).contains(&assets.len()) {
            return Err(ModelError::Capacity);
        }
        for (i, a) in assets.iter().enumerate() {
            if assets[..i].iter().any(|b| a.symbol == b.symbol) {
                return Err(ModelError::Duplicate);
            }
        }
        Ok(Self {
            trained_through,
            assets,
        })
    }
}
#[derive(Clone, Debug)]
pub struct FactorSnapshot {
    observed_at: DecisionTime,
    factor_scores: [i64; 2],
    returns: Vec<(Symbol, i64)>,
}
impl FactorSnapshot {
    pub fn new(
        observed_at: DecisionTime,
        factor_scores: [i64; 2],
        returns: Vec<(Symbol, i64)>,
    ) -> Result<Self, ModelError> {
        if !(3..=MAX_ASSETS).contains(&returns.len()) {
            return Err(ModelError::Capacity);
        }
        if factor_scores.iter().any(|x| !valid_score(*x))
            || returns.iter().any(|(_, x)| !valid_score(*x))
        {
            return Err(ModelError::InvalidInput);
        }
        for (i, (symbol, _)) in returns.iter().enumerate() {
            if returns[..i].iter().any(|(s, _)| s == symbol) {
                return Err(ModelError::Duplicate);
            }
        }
        Ok(Self {
            observed_at,
            factor_scores,
            returns,
        })
    }
}
/// Equal-dollar long/short intent in residual extremes. Not factor-neutral shares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatArbAssessment {
    Neutral,
    Pair {
        long: Symbol,
        short: Symbol,
        residual_gap: i64,
    },
}
pub fn statistical_arbitrage(
    model: &Causal<'_, FactorModel>,
    snapshot: &Causal<'_, FactorSnapshot>,
    decision: DecisionTime,
    min_residual_gap: i64,
) -> Result<StatArbAssessment, ModelError> {
    visible(model, decision)?;
    visible(snapshot, decision)?;
    if min_residual_gap < 0 {
        return Err(ModelError::InvalidInput);
    }
    let mv = model.value();
    let sv = snapshot.value();
    if mv.trained_through.micros() > model.available_ts().micros()
        || sv.observed_at.micros() > snapshot.available_ts().micros()
        || model.available_ts().micros() >= sv.observed_at.micros()
    {
        return Err(ModelError::TrainingCutoff);
    }
    if mv.assets.len() != sv.returns.len() {
        return Err(ModelError::InvalidInput);
    }
    let mut residuals = Vec::with_capacity(mv.assets.len());
    for asset in &mv.assets {
        let observed = sv
            .returns
            .iter()
            .find(|(s, _)| s == &asset.symbol)
            .ok_or(ModelError::InvalidInput)?
            .1;
        let expected = i128::from(asset.intercept) * COEFFICIENT_SCALE
            + i128::from(asset.loadings[0]) * i128::from(sv.factor_scores[0])
            + i128::from(asset.loadings[1]) * i128::from(sv.factor_scores[1]);
        residuals.push((
            &asset.symbol,
            i128::from(observed) * COEFFICIENT_SCALE - expected,
        ));
    }
    residuals.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.as_str().cmp(b.0.as_str())));
    let low = residuals.first().ok_or(ModelError::Capacity)?;
    let high = residuals.last().ok_or(ModelError::Capacity)?;
    let gap = high.1 - low.1;
    if gap <= i128::from(min_residual_gap) * COEFFICIENT_SCALE {
        return Ok(StatArbAssessment::Neutral);
    }
    Ok(StatArbAssessment::Pair {
        long: low.0.clone(),
        short: high.0.clone(),
        residual_gap: i64::try_from(gap / COEFFICIENT_SCALE).map_err(|_| ModelError::Overflow)?,
    })
}

/// Announcement timestamp and forecast frozen before that announcement.
#[derive(Clone, Copy, Debug)]
pub struct EventRelease {
    announced_at: DecisionTime,
    forecast_at: DecisionTime,
    actual: i64,
    expected: i64,
}
impl EventRelease {
    pub fn new(
        announced_at: DecisionTime,
        forecast_at: DecisionTime,
        actual: i64,
        expected: i64,
    ) -> Result<Self, ModelError> {
        if forecast_at >= announced_at || !valid_score(actual) || !valid_score(expected) {
            return Err(ModelError::InvalidInput);
        }
        Ok(Self {
            announced_at,
            forecast_at,
            actual,
            expected,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventAssessment {
    pub surprise: i64,
    pub direction: ModelDirection,
}
/// Positive surprise implies Long only for an explicitly positive-response event.
/// A caller must encode the economic response sign in its supplied actual/expected.
pub fn event_driven(
    release: &Causal<'_, EventRelease>,
    decision: DecisionTime,
    threshold: i64,
    max_age_micros: i64,
) -> Result<EventAssessment, ModelError> {
    visible(release, decision)?;
    if threshold < 0 || max_age_micros < 0 {
        return Err(ModelError::InvalidInput);
    }
    let value = release.value();
    if value.forecast_at >= value.announced_at
        || value.announced_at.micros() > release.available_ts().micros()
    {
        return Err(ModelError::TrainingCutoff);
    }
    let age = decision
        .micros()
        .micros()
        .checked_sub(value.announced_at.micros().micros())
        .ok_or(ModelError::Overflow)?;
    if age > max_age_micros {
        return Err(ModelError::Stale);
    }
    let surprise = value.actual - value.expected;
    Ok(EventAssessment {
        surprise,
        direction: direction(surprise, threshold),
    })
}

/// One feature and its later realized target; label availability is separately
/// checked through the enclosing CausalWindow, not inferred from feature time.
#[derive(Clone, Copy, Debug)]
pub struct LabeledSample {
    feature_at: DecisionTime,
    label_end: DecisionTime,
    x: i64,
    y: i64,
}
impl LabeledSample {
    pub fn new(
        feature_at: DecisionTime,
        label_end: DecisionTime,
        x: i64,
        y: i64,
    ) -> Result<Self, ModelError> {
        if feature_at >= label_end || !valid_score(x) || !valid_score(y) {
            return Err(ModelError::InvalidInput);
        }
        Ok(Self {
            feature_at,
            label_end,
            x,
            y,
        })
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Predictor {
    observed_at: DecisionTime,
    x: i64,
}
impl Predictor {
    pub fn new(observed_at: DecisionTime, x: i64) -> Result<Self, ModelError> {
        if !valid_score(x) {
            return Err(ModelError::InvalidInput);
        }
        Ok(Self { observed_at, x })
    }
}
#[derive(Debug)]
pub struct RidgeModel {
    trained_at: DecisionTime,
    n: i128,
    sx: i128,
    sy: i128,
    covariance: i128,
    denominator: i128,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prediction {
    pub score: i64,
    pub direction: ModelDirection,
}
impl RidgeModel {
    /// Univariate ridge with an unpenalized intercept. Ridge is a nonnegative
    /// penalty in squared-feature units. No search, randomization or data I/O.
    pub fn fit(window: &CausalWindow<'_, LabeledSample>, ridge: i64) -> Result<Self, ModelError> {
        if ridge < 0 {
            return Err(ModelError::InvalidInput);
        }
        if !(2..=MAX_TRAINING).contains(&window.len()) || window.cap().get() > MAX_TRAINING {
            return Err(ModelError::Capacity);
        }
        // n <= 128 and |x|, |y| <= 1e6 bound every sum/product below i128.
        let (mut sx, mut sy, mut sxx, mut sxy) = (0_i128, 0_i128, 0_i128, 0_i128);
        let mut previous = None;
        for view in window.iter() {
            let row = view.value();
            if row.label_end.micros() > view.available_ts().micros() {
                return Err(ModelError::TrainingCutoff);
            }
            if previous.is_some_and(|t| t >= row.feature_at) {
                return Err(ModelError::Duplicate);
            }
            previous = Some(row.feature_at);
            let x = i128::from(row.x);
            let y = i128::from(row.y);
            sx += x;
            sy += y;
            sxx += x * x;
            sxy += x * y;
        }
        let n = i128::try_from(window.len()).map_err(|_| ModelError::Capacity)?;
        let covariance = n * sxy - sx * sy;
        let denominator = n * sxx - sx * sx + n * i128::from(ridge);
        if denominator <= 0 {
            return Err(ModelError::Degenerate);
        }
        Ok(Self {
            trained_at: window.decision(),
            n,
            sx,
            sy,
            covariance,
            denominator,
        })
    }
    pub fn predict(
        &self,
        predictor: &Causal<'_, Predictor>,
        decision: DecisionTime,
        threshold: i64,
    ) -> Result<Prediction, ModelError> {
        visible(predictor, decision)?;
        if threshold < 0 {
            return Err(ModelError::InvalidInput);
        }
        let row = predictor.value();
        if row.observed_at <= self.trained_at
            || row.observed_at.micros() > predictor.available_ts().micros()
        {
            return Err(ModelError::TrainingCutoff);
        }
        let numerator = self
            .sy
            .checked_mul(self.denominator)
            .and_then(|v| {
                self.covariance
                    .checked_mul(self.n * i128::from(row.x) - self.sx)
                    .and_then(|w| v.checked_add(w))
            })
            .ok_or(ModelError::Overflow)?;
        let denominator = self
            .n
            .checked_mul(self.denominator)
            .ok_or(ModelError::Overflow)?;
        let score = i64::try_from(numerator / denominator).map_err(|_| ModelError::Overflow)?;
        Ok(Prediction {
            score,
            direction: direction(score, threshold),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parangsae_core::{AvailableTs, EventTs, Observation, ReceivedTs, Timestamps, UtcMicros};
    use std::num::NonZeroUsize;
    fn at(t: i64) -> DecisionTime {
        DecisionTime::from_micros(UtcMicros::from_micros(t))
    }
    fn obs<T>(t: i64, value: T) -> Observation<T> {
        Observation::new(
            Timestamps {
                event_ts: EventTs::from_micros(UtcMicros::from_micros(t)),
                available_ts: AvailableTs::from_micros(UtcMicros::from_micros(t)),
                received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(t)),
            },
            value,
        )
    }
    fn symbol(s: &str) -> Symbol {
        Symbol::new(s).expect("symbol")
    }
    #[test]
    fn factor_model_removes_two_common_components_before_ranking() {
        let assets = ["A", "B", "C"]
            .into_iter()
            .map(|s| FactorAsset::new(symbol(s), 0, [10_000, 20_000]).expect("asset"))
            .collect();
        let model = obs(10, FactorModel::new(at(9), assets).expect("model"));
        let snapshot = obs(
            20,
            FactorSnapshot::new(
                at(20),
                [100, 50],
                vec![(symbol("A"), 150), (symbol("B"), 200), (symbol("C"), 260)],
            )
            .expect("snapshot"),
        );
        assert_eq!(
            statistical_arbitrage(
                &model.causal_at(at(20)).expect("visible"),
                &snapshot.causal_at(at(20)).expect("visible"),
                at(20),
                100
            ),
            Ok(StatArbAssessment::Pair {
                long: symbol("A"),
                short: symbol("C"),
                residual_gap: 110
            })
        );
    }
    #[test]
    fn different_factor_exposures_change_the_selected_extremes() {
        let assets = vec![
            FactorAsset::new(symbol("A"), 0, [10_000, 0]).expect("asset"),
            FactorAsset::new(symbol("B"), 0, [0, 10_000]).expect("asset"),
            FactorAsset::new(symbol("C"), 0, [10_000, 20_000]).expect("asset"),
        ];
        let model = obs(10, FactorModel::new(at(9), assets).expect("model"));
        let snapshot = obs(
            20,
            FactorSnapshot::new(
                at(20),
                [100, 50],
                vec![(symbol("A"), 150), (symbol("B"), 80), (symbol("C"), 190)],
            )
            .expect("snapshot"),
        );
        assert_eq!(
            statistical_arbitrage(
                &model.causal_at(at(20)).expect("visible"),
                &snapshot.causal_at(at(20)).expect("visible"),
                at(20),
                0
            ),
            Ok(StatArbAssessment::Pair {
                long: symbol("C"),
                short: symbol("A"),
                residual_gap: 60
            })
        );
        assert_eq!(
            statistical_arbitrage(
                &model.causal_at(at(20)).expect("visible"),
                &snapshot.causal_at(at(20)).expect("visible"),
                at(20),
                60
            ),
            Ok(StatArbAssessment::Neutral)
        );
    }
    #[test]
    fn announcement_uses_the_frozen_preannouncement_forecast() {
        let release = obs(
            20,
            EventRelease::new(at(20), at(10), 150, 100).expect("release"),
        );
        assert_eq!(
            event_driven(&release.causal_at(at(20)).expect("visible"), at(20), 10, 5),
            Ok(EventAssessment {
                surprise: 50,
                direction: ModelDirection::Long
            })
        );
    }
    #[test]
    fn fits_a_line_then_predicts_a_strictly_later_feature() {
        let rows = [
            obs(2, LabeledSample::new(at(1), at(2), 1, 3).expect("sample")),
            obs(4, LabeledSample::new(at(3), at(4), 2, 5).expect("sample")),
            obs(6, LabeledSample::new(at(5), at(6), 3, 7).expect("sample")),
        ];
        let window = CausalWindow::from_views(
            at(6),
            NonZeroUsize::new(3).expect("cap"),
            rows.iter().filter_map(|r| r.causal_at(at(6))),
        )
        .expect("window");
        let model = RidgeModel::fit(&window, 0).expect("fit");
        let predictor = obs(7, Predictor::new(at(7), 4).expect("predictor"));
        assert_eq!(
            model.predict(&predictor.causal_at(at(7)).expect("visible"), at(7), 0),
            Ok(Prediction {
                score: 9,
                direction: ModelDirection::Long
            })
        );
    }
    fn training() -> Vec<Observation<LabeledSample>> {
        vec![
            obs(2, LabeledSample::new(at(1), at(2), 1, 3).expect("sample")),
            obs(4, LabeledSample::new(at(3), at(4), 2, 5).expect("sample")),
            obs(6, LabeledSample::new(at(5), at(6), 3, 7).expect("sample")),
        ]
    }
    fn window(rows: &[Observation<LabeledSample>]) -> CausalWindow<'_, LabeledSample> {
        CausalWindow::from_views(
            at(6),
            NonZeroUsize::new(3).expect("cap"),
            rows.iter().filter_map(|r| r.causal_at(at(6))),
        )
        .expect("window")
    }
    #[test]
    fn training_requires_visible_labels_and_prediction_is_strictly_out_of_sample() {
        let mut rows = training();
        rows[2] = obs(6, LabeledSample::new(at(5), at(7), 3, 7).expect("sample"));
        assert!(matches!(
            RidgeModel::fit(&window(&rows), 0),
            Err(ModelError::TrainingCutoff)
        ));
        let model = RidgeModel::fit(&window(&training()), 0).expect("model");
        let predictor = obs(7, Predictor::new(at(6), 4).expect("predictor"));
        assert_eq!(
            model.predict(&predictor.causal_at(at(7)).expect("view"), at(7), 0),
            Err(ModelError::TrainingCutoff)
        );
        assert_eq!(
            model.predict(&predictor.causal_at(at(7)).expect("view"), at(6), 0),
            Err(ModelError::AfterDecision)
        );
    }
    #[test]
    fn degeneracy_duplicate_and_invalid_parameters_are_not_predictions() {
        let rows = [
            obs(2, LabeledSample::new(at(1), at(2), 1, 3).expect("sample")),
            obs(4, LabeledSample::new(at(3), at(4), 1, 5).expect("sample")),
        ];
        assert!(matches!(
            RidgeModel::fit(&window(&rows), 0),
            Err(ModelError::Degenerate)
        ));
        let model = RidgeModel::fit(&window(&rows), 1).expect("ridge admits constant x");
        let predictor = obs(7, Predictor::new(at(7), 50).expect("predictor"));
        assert_eq!(
            model
                .predict(&predictor.causal_at(at(7)).expect("view"), at(7), 0)
                .expect("prediction")
                .score,
            4
        );
        assert!(matches!(
            RidgeModel::fit(&window(&rows), -1),
            Err(ModelError::InvalidInput)
        ));
        let duplicate = [rows[0].clone(), rows[0].clone()];
        assert!(matches!(
            RidgeModel::fit(&window(&duplicate), 1),
            Err(ModelError::Duplicate)
        ));
        assert!(LabeledSample::new(at(1), at(1), 0, 0).is_err());
        assert!(Predictor::new(at(7), i64::MIN).is_err());
    }
    #[test]
    fn stale_events_and_backdated_announcements_are_refused() {
        let release = obs(
            20,
            EventRelease::new(at(20), at(10), 150, 100).expect("release"),
        );
        let view = release.causal_at(at(30)).expect("visible");
        assert_eq!(event_driven(&view, at(30), 10, 5), Err(ModelError::Stale));
        assert_eq!(
            event_driven(&view, at(20), 50, 5)
                .expect("at equality")
                .direction,
            ModelDirection::Neutral
        );
        let bad = obs(
            19,
            EventRelease::new(at(20), at(10), 150, 100).expect("release"),
        );
        assert_eq!(
            event_driven(&bad.causal_at(at(20)).expect("visible"), at(20), 10, 5),
            Err(ModelError::TrainingCutoff)
        );
    }
    #[test]
    fn statarb_needs_exact_universe_and_prior_model() {
        let assets = ["A", "B", "C"]
            .into_iter()
            .map(|s| FactorAsset::new(symbol(s), 0, [10_000, 20_000]).expect("asset"))
            .collect();
        let model = obs(20, FactorModel::new(at(9), assets).expect("model"));
        let snapshot = obs(
            20,
            FactorSnapshot::new(
                at(20),
                [0, 0],
                vec![(symbol("A"), 150), (symbol("B"), 200), (symbol("C"), 260)],
            )
            .expect("snapshot"),
        );
        assert_eq!(
            statistical_arbitrage(
                &model.causal_at(at(20)).expect("visible"),
                &snapshot.causal_at(at(20)).expect("visible"),
                at(20),
                0
            ),
            Err(ModelError::TrainingCutoff)
        );
        assert!(matches!(
            FactorSnapshot::new(
                at(20),
                [0, 0],
                vec![(symbol("A"), 1), (symbol("A"), 2), (symbol("C"), 3)]
            ),
            Err(ModelError::Duplicate)
        ));
    }
    proptest::proptest! {
        #[test]
        fn future_labels_do_not_change_fitted_predictions(x in -100_i64..100, y in -1000_i64..1000) {
            let mut rows = training();
            let predictor = obs(7, Predictor::new(at(7), 4).expect("predictor"));
            let before = RidgeModel::fit(&window(&rows), 0).expect("fit").predict(&predictor.causal_at(at(7)).expect("view"), at(7), 0).expect("predict");
            rows.push(obs(100, LabeledSample::new(at(90), at(100), x, y).expect("sample")));
            let after = RidgeModel::fit(&window(&rows), 0).expect("fit").predict(&predictor.causal_at(at(7)).expect("view"), at(7), 0).expect("predict");
            proptest::prop_assert_eq!(before, after);
        }
        #[test]
        fn exact_affine_training_is_recovered(slope in -20_i64..20, intercept in -100_i64..100, x in -100_i64..100) {
            let rows = [obs(2, LabeledSample::new(at(1), at(2), 1, slope + intercept).expect("sample")), obs(4, LabeledSample::new(at(3), at(4), 2, slope * 2 + intercept).expect("sample")), obs(6, LabeledSample::new(at(5), at(6), 3, slope * 3 + intercept).expect("sample"))];
            let model = RidgeModel::fit(&window(&rows), 0).expect("fit");
            let predictor = obs(7, Predictor::new(at(7), x).expect("predictor"));
            let prediction = model.predict(&predictor.causal_at(at(7)).expect("view"), at(7), 0).expect("predict");
            proptest::prop_assert_eq!(prediction.score, slope * x + intercept);
        }
    }
}
