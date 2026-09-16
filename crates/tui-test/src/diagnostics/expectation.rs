use serde::{Deserialize, Serialize};

use crate::api::LocatorQuery;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocatorExpectation {
    Matches,
    Visible,
    Hidden,
    Unique,
    Actionable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OperationExpectation {
    Locator {
        query: Box<LocatorQuery>,
        outcome: LocatorExpectation,
    },
    Value {
        subject: String,
        expected: String,
    },
    Unavailable {
        reason: String,
    },
}
