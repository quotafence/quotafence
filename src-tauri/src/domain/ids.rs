use serde::{Deserialize, Deserializer, Serialize};

use super::{DomainError, DomainResult};

macro_rules! string_id {
    ($name:ident, $field:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> DomainResult<Self> {
                let value = value.into();
                let value = value.trim();

                if value.is_empty() {
                    return Err(DomainError::EmptyValue { field: $field });
                }

                Ok(Self(value.to_owned()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

string_id!(ProviderId, "provider ID");
string_id!(AccountId, "account ID");
string_id!(QuotaPoolId, "quota pool ID");
string_id!(WindowId, "window ID");
string_id!(ScopeId, "scope ID");
string_id!(ReservationId, "reservation ID");
string_id!(UsageEventId, "usage event ID");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_trimmed() {
        let id = ScopeId::new("  project-a  ").expect("valid scope ID");

        assert_eq!(id.as_str(), "project-a");
    }

    #[test]
    fn identifiers_reject_blank_values() {
        assert_eq!(
            ScopeId::new(" \n "),
            Err(DomainError::EmptyValue { field: "scope ID" })
        );
    }

    #[test]
    fn deserialization_cannot_bypass_identifier_validation() {
        let result = serde_json::from_str::<ScopeId>("\"  \"");

        assert!(result.is_err());
    }
}
