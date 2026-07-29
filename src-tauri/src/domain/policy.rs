use serde::{Deserialize, Deserializer, Serialize};

use super::{DomainError, DomainResult, QuotaBalance};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct BasisPoints(u16);

impl BasisPoints {
    pub fn new(value: u16) -> DomainResult<Self> {
        if !(1..=10_000).contains(&value) {
            return Err(DomainError::InvalidThreshold {
                basis_points: value,
            });
        }

        Ok(Self(value))
    }

    pub fn value(self) -> u16 {
        self.0
    }
}

impl<'de> Deserialize<'de> for BasisPoints {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u16::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnforcementPolicy {
    warn_at: Option<BasisPoints>,
    confirm_at: Option<BasisPoints>,
    stop_at: Option<BasisPoints>,
}

impl EnforcementPolicy {
    pub fn new(
        warn_at: Option<BasisPoints>,
        confirm_at: Option<BasisPoints>,
        stop_at: Option<BasisPoints>,
    ) -> DomainResult<Self> {
        let ordered_thresholds: Vec<_> = [warn_at, confirm_at, stop_at]
            .into_iter()
            .flatten()
            .collect();

        if ordered_thresholds.windows(2).any(|pair| pair[0] > pair[1]) {
            return Err(DomainError::InvalidThresholdOrder);
        }

        Ok(Self {
            warn_at,
            confirm_at,
            stop_at,
        })
    }

    pub fn standard() -> Self {
        Self {
            warn_at: Some(BasisPoints(8_000)),
            confirm_at: Some(BasisPoints(9_000)),
            stop_at: Some(BasisPoints(10_000)),
        }
    }

    pub fn warn_at(&self) -> Option<BasisPoints> {
        self.warn_at
    }

    pub fn confirm_at(&self) -> Option<BasisPoints> {
        self.confirm_at
    }

    pub fn stop_at(&self) -> Option<BasisPoints> {
        self.stop_at
    }

    pub fn evaluate(&self, balance: &QuotaBalance) -> EnforcementDecision {
        if balance.allocation().value() == 0 {
            return EnforcementDecision::Stop;
        }

        if self
            .stop_at
            .is_some_and(|threshold| reached(balance, threshold))
        {
            return EnforcementDecision::Stop;
        }

        if self
            .confirm_at
            .is_some_and(|threshold| reached(balance, threshold))
        {
            return EnforcementDecision::RequireConfirmation;
        }

        if self
            .warn_at
            .is_some_and(|threshold| reached(balance, threshold))
        {
            return EnforcementDecision::Warn;
        }

        EnforcementDecision::Allow
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnforcementDecision {
    Allow,
    Warn,
    RequireConfirmation,
    Stop,
}

fn reached(balance: &QuotaBalance, threshold: BasisPoints) -> bool {
    let committed = u128::from(balance.committed());
    let allocation = u128::from(balance.allocation().value());
    let threshold = u128::from(threshold.value());

    committed * 10_000 >= allocation * threshold
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{QuotaAmount, QuotaUnit};

    fn balance(allocation: u64, usage: u64, reserved: u64) -> QuotaBalance {
        let unit = QuotaUnit::new("quota_points").unwrap();
        QuotaBalance::new(
            QuotaAmount::new(allocation, unit.clone()),
            QuotaAmount::new(usage, unit.clone()),
            QuotaAmount::new(reserved, unit),
        )
        .unwrap()
    }

    #[test]
    fn standard_policy_escalates_at_each_threshold() {
        let policy = EnforcementPolicy::standard();

        assert_eq!(
            policy.evaluate(&balance(100, 79, 0)),
            EnforcementDecision::Allow
        );
        assert_eq!(
            policy.evaluate(&balance(100, 80, 0)),
            EnforcementDecision::Warn
        );
        assert_eq!(
            policy.evaluate(&balance(100, 85, 5)),
            EnforcementDecision::RequireConfirmation
        );
        assert_eq!(
            policy.evaluate(&balance(100, 100, 0)),
            EnforcementDecision::Stop
        );
    }

    #[test]
    fn zero_allocation_always_stops() {
        let policy = EnforcementPolicy::new(None, None, None).unwrap();

        assert_eq!(
            policy.evaluate(&balance(0, 0, 0)),
            EnforcementDecision::Stop
        );
    }

    #[test]
    fn thresholds_must_be_ordered() {
        let result = EnforcementPolicy::new(
            Some(BasisPoints::new(9_000).unwrap()),
            Some(BasisPoints::new(8_000).unwrap()),
            Some(BasisPoints::new(10_000).unwrap()),
        );

        assert_eq!(result, Err(DomainError::InvalidThresholdOrder));
    }

    #[test]
    fn deserialization_cannot_create_an_invalid_threshold() {
        let result = serde_json::from_str::<BasisPoints>("10001");

        assert!(result.is_err());
    }
}
