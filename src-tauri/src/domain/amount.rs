use serde::{Deserialize, Deserializer, Serialize};

use super::{DomainError, DomainResult};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct QuotaUnit(String);

impl QuotaUnit {
    pub fn new(value: impl Into<String>) -> DomainResult<Self> {
        let value = value.into();
        let value = value.trim();

        if value.is_empty() {
            return Err(DomainError::EmptyValue {
                field: "quota unit",
            });
        }

        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for QuotaUnit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for QuotaUnit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaAmount {
    value: u64,
    unit: QuotaUnit,
}

impl QuotaAmount {
    pub fn new(value: u64, unit: QuotaUnit) -> Self {
        Self { value, unit }
    }

    pub fn value(&self) -> u64 {
        self.value
    }

    pub fn unit(&self) -> &QuotaUnit {
        &self.unit
    }

    pub fn checked_add(&self, other: &Self) -> DomainResult<Self> {
        self.ensure_same_unit(other)?;

        let value = self
            .value
            .checked_add(other.value)
            .ok_or(DomainError::ArithmeticOverflow)?;

        Ok(Self::new(value, self.unit.clone()))
    }

    pub(crate) fn ensure_same_unit(&self, other: &Self) -> DomainResult<()> {
        if self.unit != other.unit {
            return Err(DomainError::UnitMismatch {
                expected: self.unit.to_string(),
                actual: other.unit.to_string(),
            });
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QuotaBalance {
    allocation: QuotaAmount,
    attributed_usage: QuotaAmount,
    active_reservations: QuotaAmount,
}

impl QuotaBalance {
    pub(crate) fn new(
        allocation: QuotaAmount,
        attributed_usage: QuotaAmount,
        active_reservations: QuotaAmount,
    ) -> DomainResult<Self> {
        allocation.ensure_same_unit(&attributed_usage)?;
        allocation.ensure_same_unit(&active_reservations)?;
        attributed_usage.checked_add(&active_reservations)?;

        Ok(Self {
            allocation,
            attributed_usage,
            active_reservations,
        })
    }

    pub fn allocation(&self) -> &QuotaAmount {
        &self.allocation
    }

    pub fn attributed_usage(&self) -> &QuotaAmount {
        &self.attributed_usage
    }

    pub fn active_reservations(&self) -> &QuotaAmount {
        &self.active_reservations
    }

    pub fn committed(&self) -> u64 {
        self.attributed_usage.value + self.active_reservations.value
    }

    pub fn remaining(&self) -> i128 {
        i128::from(self.allocation.value) - i128::from(self.committed())
    }

    pub fn spendable(&self) -> QuotaAmount {
        let value = u64::try_from(self.remaining()).unwrap_or(0);
        QuotaAmount::new(value, self.allocation.unit.clone())
    }

    pub fn is_exhausted(&self) -> bool {
        self.remaining() <= 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(value: u64) -> QuotaAmount {
        QuotaAmount::new(value, QuotaUnit::new("quota_points").unwrap())
    }

    #[test]
    fn amount_addition_rejects_different_units() {
        let points = points(10);
        let credits = QuotaAmount::new(5, QuotaUnit::new("credits").unwrap());

        assert_eq!(
            points.checked_add(&credits),
            Err(DomainError::UnitMismatch {
                expected: "quota_points".to_owned(),
                actual: "credits".to_owned(),
            })
        );
    }

    #[test]
    fn negative_remaining_is_preserved_but_not_spendable() {
        let balance = QuotaBalance::new(points(100), points(120), points(5)).unwrap();

        assert_eq!(balance.remaining(), -25);
        assert_eq!(balance.spendable().value(), 0);
        assert!(balance.is_exhausted());
    }
}
