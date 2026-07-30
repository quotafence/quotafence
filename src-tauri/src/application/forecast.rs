use super::{DepletionForecast, DepletionForecastStatus, ForecastConfidence};

const HOUR_MILLIS: i64 = 60 * 60 * 1_000;
const DAY_MILLIS: u128 = 24 * 60 * 60 * 1_000;
const MIN_SAMPLES: usize = 2;
const MIN_COVERAGE_BASIS_POINTS: u16 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ManagedUsageSample {
    pub created_at: i64,
    pub reconciled_at: i64,
    pub amount: u64,
    pub attributed: bool,
    pub trustworthy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForecastInput {
    pub window_starts_at: i64,
    pub window_ends_at: i64,
    pub at: i64,
    pub provider_remaining: u64,
    pub provider_observed_usage: u64,
    pub samples: Vec<ManagedUsageSample>,
}

pub(crate) fn calculate_depletion_forecast(input: ForecastInput) -> DepletionForecast {
    if input.at >= input.window_ends_at {
        return empty_forecast(
            DepletionForecastStatus::WindowEnded,
            input.at,
            input.samples.len(),
        );
    }
    if input.at < input.window_starts_at {
        return empty_forecast(DepletionForecastStatus::InsufficientData, input.at, 0);
    }

    let samples: Vec<_> = input
        .samples
        .into_iter()
        .filter(|sample| {
            sample.reconciled_at >= input.window_starts_at
                && sample.reconciled_at <= input.at
                && sample.reconciled_at < input.window_ends_at
        })
        .collect();
    let observation_start = samples
        .iter()
        .map(|sample| sample.created_at.max(input.window_starts_at))
        .min();
    let attributed_usage = samples
        .iter()
        .filter(|sample| sample.attributed)
        .fold(0_u64, |total, sample| total.saturating_add(sample.amount));
    let managed_usage = samples
        .iter()
        .fold(0_u64, |total, sample| total.saturating_add(sample.amount));
    let trustworthy_samples = samples.iter().filter(|sample| sample.trustworthy).count();
    let coverage_basis_points =
        coverage_basis_points(attributed_usage, input.provider_observed_usage);

    let Some(observation_start) = observation_start else {
        return empty_forecast(DepletionForecastStatus::InsufficientData, input.at, 0);
    };
    let observation_duration = input.at.saturating_sub(observation_start);
    let enough_evidence = samples.len() >= MIN_SAMPLES
        && trustworthy_samples >= MIN_SAMPLES
        && observation_duration >= HOUR_MILLIS
        && (input.provider_observed_usage == 0
            || coverage_basis_points >= MIN_COVERAGE_BASIS_POINTS);

    if !enough_evidence {
        return DepletionForecast {
            status: DepletionForecastStatus::InsufficientData,
            confidence: ForecastConfidence::Low,
            sample_count: samples.len() as u32,
            observation_start: Some(observation_start),
            observation_end: input.at,
            managed_usage,
            attributed_managed_usage: attributed_usage,
            coverage_basis_points,
            burn_rate_per_day_milliunits: None,
            projected_depletion_at: None,
            projected_remaining_at_reset: None,
        };
    }

    if attributed_usage == 0 {
        return DepletionForecast {
            status: DepletionForecastStatus::NoManagedBurn,
            confidence: ForecastConfidence::Medium,
            sample_count: samples.len() as u32,
            observation_start: Some(observation_start),
            observation_end: input.at,
            managed_usage,
            attributed_managed_usage: 0,
            coverage_basis_points,
            burn_rate_per_day_milliunits: Some(0),
            projected_depletion_at: None,
            projected_remaining_at_reset: Some(
                i64::try_from(input.provider_remaining).unwrap_or(i64::MAX),
            ),
        };
    }

    let duration = u128::try_from(observation_duration).unwrap_or(1);
    let attributed = u128::from(attributed_usage);
    let burn_rate_per_day_milliunits =
        attributed.saturating_mul(DAY_MILLIS).saturating_mul(1_000) / duration;
    let millis_to_depletion =
        u128::from(input.provider_remaining).saturating_mul(duration) / attributed;
    let projected_depletion_at = i64::try_from(millis_to_depletion)
        .ok()
        .and_then(|duration| input.at.checked_add(duration));
    let remaining_window_duration =
        u128::try_from(input.window_ends_at.saturating_sub(input.at)).unwrap_or(0);
    let projected_burn_to_reset = attributed
        .saturating_mul(remaining_window_duration)
        .div_ceil(duration);
    let projected_remaining_at_reset =
        u128::from(input.provider_remaining).saturating_sub(projected_burn_to_reset);
    let projected_remaining_at_reset =
        i64::try_from(projected_remaining_at_reset).unwrap_or(i64::MAX);
    let status =
        if projected_depletion_at.is_some_and(|depletion_at| depletion_at < input.window_ends_at) {
            DepletionForecastStatus::DepletesBeforeReset
        } else {
            DepletionForecastStatus::SurvivesToReset
        };

    DepletionForecast {
        status,
        confidence: ForecastConfidence::Medium,
        sample_count: samples.len() as u32,
        observation_start: Some(observation_start),
        observation_end: input.at,
        managed_usage,
        attributed_managed_usage: attributed_usage,
        coverage_basis_points,
        burn_rate_per_day_milliunits: Some(
            u64::try_from(burn_rate_per_day_milliunits).unwrap_or(u64::MAX),
        ),
        projected_depletion_at,
        projected_remaining_at_reset: Some(projected_remaining_at_reset),
    }
}

fn empty_forecast(
    status: DepletionForecastStatus,
    at: i64,
    sample_count: usize,
) -> DepletionForecast {
    DepletionForecast {
        status,
        confidence: ForecastConfidence::Low,
        sample_count: sample_count as u32,
        observation_start: None,
        observation_end: at,
        managed_usage: 0,
        attributed_managed_usage: 0,
        coverage_basis_points: 0,
        burn_rate_per_day_milliunits: None,
        projected_depletion_at: None,
        projected_remaining_at_reset: None,
    }
}

fn coverage_basis_points(attributed_usage: u64, provider_observed_usage: u64) -> u16 {
    if provider_observed_usage == 0 {
        return 10_000;
    }
    let coverage =
        u128::from(attributed_usage).saturating_mul(10_000) / u128::from(provider_observed_usage);
    u16::try_from(coverage.min(10_000)).unwrap_or(10_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = HOUR_MILLIS;
    const DAY: i64 = 24 * HOUR;

    fn sample(created_at: i64, reconciled_at: i64, amount: u64) -> ManagedUsageSample {
        ManagedUsageSample {
            created_at,
            reconciled_at,
            amount,
            attributed: true,
            trustworthy: true,
        }
    }

    fn input(at: i64, spendable: u64, samples: Vec<ManagedUsageSample>) -> ForecastInput {
        ForecastInput {
            window_starts_at: 0,
            window_ends_at: 7 * DAY,
            at,
            provider_remaining: spendable,
            provider_observed_usage: samples.iter().map(|sample| sample.amount).sum(),
            samples,
        }
    }

    #[test]
    fn sparse_history_does_not_produce_a_precise_rate() {
        let forecast =
            calculate_depletion_forecast(input(2 * HOUR, 90, vec![sample(HOUR, HOUR, 10)]));

        assert_eq!(forecast.status, DepletionForecastStatus::InsufficientData);
        assert_eq!(forecast.confidence, ForecastConfidence::Low);
        assert_eq!(forecast.sample_count, 1);
        assert_eq!(forecast.burn_rate_per_day_milliunits, None);
        assert_eq!(forecast.projected_depletion_at, None);
    }

    #[test]
    fn reconciled_zero_usage_reports_no_managed_burn() {
        let forecast = calculate_depletion_forecast(input(
            3 * HOUR,
            100,
            vec![sample(HOUR, HOUR, 0), sample(2 * HOUR, 2 * HOUR, 0)],
        ));

        assert_eq!(forecast.status, DepletionForecastStatus::NoManagedBurn);
        assert_eq!(forecast.confidence, ForecastConfidence::Medium);
        assert_eq!(forecast.burn_rate_per_day_milliunits, Some(0));
        assert_eq!(forecast.projected_remaining_at_reset, Some(100));
    }

    #[test]
    fn steady_usage_forecasts_depletion_before_reset() {
        let forecast = calculate_depletion_forecast(input(
            3 * DAY,
            20,
            vec![sample(DAY, DAY, 20), sample(2 * DAY, 2 * DAY, 20)],
        ));

        assert_eq!(
            forecast.status,
            DepletionForecastStatus::DepletesBeforeReset
        );
        assert_eq!(forecast.burn_rate_per_day_milliunits, Some(20_000));
        assert_eq!(forecast.projected_depletion_at, Some(4 * DAY));
        assert_eq!(forecast.projected_remaining_at_reset, Some(0));
    }

    #[test]
    fn bursty_usage_uses_the_observed_average_and_can_survive_reset() {
        let forecast = calculate_depletion_forecast(input(
            5 * DAY,
            80,
            vec![
                sample(DAY, DAY, 1),
                sample(DAY + HOUR, DAY + HOUR, 39),
                sample(4 * DAY, 4 * DAY, 1),
            ],
        ));

        assert_eq!(forecast.status, DepletionForecastStatus::SurvivesToReset);
        let depletion_at = forecast.projected_depletion_at.unwrap();
        assert!(depletion_at > 12 * DAY);
        assert!(depletion_at < 13 * DAY);
        assert!(forecast.projected_remaining_at_reset.unwrap() > 0);
    }

    #[test]
    fn mostly_unattributed_usage_suppresses_a_precise_forecast() {
        let mut forecast_input = input(
            3 * DAY,
            70,
            vec![sample(DAY, DAY, 5), sample(2 * DAY, 2 * DAY, 5)],
        );
        forecast_input.provider_observed_usage = 30;

        let forecast = calculate_depletion_forecast(forecast_input);

        assert_eq!(forecast.status, DepletionForecastStatus::InsufficientData);
        assert_eq!(forecast.coverage_basis_points, 3_333);
        assert_eq!(forecast.projected_depletion_at, None);
    }

    #[test]
    fn reset_boundary_never_projects_into_an_expired_window() {
        let forecast = calculate_depletion_forecast(input(
            7 * DAY,
            10,
            vec![sample(DAY, DAY, 20), sample(2 * DAY, 2 * DAY, 20)],
        ));

        assert_eq!(forecast.status, DepletionForecastStatus::WindowEnded);
        assert_eq!(forecast.projected_depletion_at, None);
    }
}
