use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DomainError {
    #[error("invalid {kind} identifier '{value}': {reason}")]
    InvalidIdentifier {
        kind: &'static str,
        value: String,
        reason: String,
    },
}

macro_rules! domain_id {
    ($name:ident, $kind:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = DomainError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value)
                    .map(Self)
                    .map_err(|error| DomainError::InvalidIdentifier {
                        kind: $kind,
                        value: value.to_owned(),
                        reason: error.to_string(),
                    })
            }
        }
    };
}

domain_id!(CalendarId, "calendar");
domain_id!(EventId, "event");
domain_id!(TodoId, "todo");
domain_id!(ReminderId, "reminder");
domain_id!(DeliveryId, "reminder delivery");
domain_id!(AuditId, "audit record");
