use std::fmt;

pub type DomainResult<T> = Result<T, DomainError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    EmptyValue {
        field: &'static str,
    },
    ZeroAmount {
        context: &'static str,
    },
    InvalidWindow {
        starts_at: i64,
        ends_at: i64,
    },
    InvalidReservationExpiry {
        created_at: i64,
        expires_at: i64,
    },
    SelfParent {
        scope_id: String,
    },
    UnitMismatch {
        expected: String,
        actual: String,
    },
    ScopeMismatch {
        expected: String,
        actual: String,
    },
    WindowMismatch {
        expected: String,
        actual: String,
    },
    InvalidReservationTransition {
        from: &'static str,
        to: &'static str,
    },
    InvalidThreshold {
        basis_points: u16,
    },
    InvalidThresholdOrder,
    AllocationExceeded {
        limit: u64,
        allocated: u64,
        unit: String,
    },
    ArithmeticOverflow,
}

impl fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyValue { field } => write!(formatter, "{field} cannot be empty"),
            Self::ZeroAmount { context } => {
                write!(formatter, "{context} amount must be greater than zero")
            }
            Self::InvalidWindow {
                starts_at,
                ends_at,
            } => write!(
                formatter,
                "quota window must end after it starts ({starts_at}..{ends_at})"
            ),
            Self::InvalidReservationExpiry {
                created_at,
                expires_at,
            } => write!(
                formatter,
                "reservation must expire after it is created ({created_at}..{expires_at})"
            ),
            Self::SelfParent { scope_id } => {
                write!(formatter, "scope {scope_id} cannot be its own parent")
            }
            Self::UnitMismatch { expected, actual } => {
                write!(formatter, "expected quota unit {expected}, received {actual}")
            }
            Self::ScopeMismatch { expected, actual } => {
                write!(formatter, "expected scope {expected}, received {actual}")
            }
            Self::WindowMismatch { expected, actual } => {
                write!(formatter, "expected window {expected}, received {actual}")
            }
            Self::InvalidReservationTransition { from, to } => {
                write!(formatter, "cannot transition reservation from {from} to {to}")
            }
            Self::InvalidThreshold { basis_points } => write!(
                formatter,
                "policy threshold must be between 1 and 10,000 basis points, received {basis_points}"
            ),
            Self::InvalidThresholdOrder => write!(
                formatter,
                "policy thresholds must be ordered warn <= confirm <= stop"
            ),
            Self::AllocationExceeded {
                limit,
                allocated,
                unit,
            } => write!(
                formatter,
                "allocated {allocated} {unit}, exceeding the limit of {limit} {unit}"
            ),
            Self::ArithmeticOverflow => write!(formatter, "quota arithmetic overflowed"),
        }
    }
}

impl std::error::Error for DomainError {}
