use serde::{Deserialize, Serialize};

use crate::api::{ClipboardPattern, LocatorQuery, Operation};

const MAX_EXPECTATION_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocatorExpectation {
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

impl OperationExpectation {
    pub(crate) fn capture(operation: &Operation) -> Option<Self> {
        let value = |subject: &str, expected: String| Self::Value {
            subject: subject.into(),
            expected,
        };
        let pattern = |text: &str, regex: bool| {
            format!(
                "{} {text:?}",
                if regex { "matches regex" } else { "contains" }
            )
        };
        let expectation = match operation {
            Operation::WaitLocator { query, not, .. } => Self::Locator {
                query: Box::new(query.clone()),
                outcome: if *not {
                    LocatorExpectation::Hidden
                } else {
                    LocatorExpectation::Visible
                },
            },
            Operation::ResolveLocator { query } => Self::Locator {
                query: Box::new(query.clone()),
                outcome: LocatorExpectation::Unique,
            },
            Operation::ClickLocator { query, .. } => Self::Locator {
                query: Box::new(query.clone()),
                outcome: LocatorExpectation::Actionable,
            },
            Operation::HighlightLocator { query, .. } => Self::Locator {
                query: Box::new(query.clone()),
                outcome: LocatorExpectation::Visible,
            },
            Operation::WaitTitle {
                text, regex, not, ..
            }
            | Operation::ExpectTitle {
                text, regex, not, ..
            } => value(
                "Terminal title",
                format!(
                    "{}{}",
                    if *not { "not " } else { "" },
                    pattern(text, *regex)
                ),
            ),
            Operation::ExpectOutput { text, regex } => {
                value("Command output", pattern(text, *regex))
            }
            Operation::ExpectExitCode { code, .. } => value("Command exit code", code.to_string()),
            Operation::ExpectBellCount { count, .. } => {
                value("Bell count", format!("at least {count}"))
            }
            Operation::ExpectMode { mode, enabled, .. } => {
                value(&format!("Terminal mode {mode:?}"), enabled.to_string())
            }
            Operation::ExpectColors {
                foreground,
                background,
                cursor,
                palette,
                ..
            } => {
                let colors = [
                    ("foreground", foreground),
                    ("background", background),
                    ("cursor", cursor),
                ]
                .into_iter()
                .filter_map(|(name, color)| color.as_ref().map(|color| format!("{name}={color:?}")))
                .chain(
                    palette
                        .iter()
                        .map(|(index, color)| format!("palette[{index}]={color:?}")),
                )
                .collect::<Vec<_>>()
                .join(", ");
                value("Terminal colors", colors)
            }
            Operation::ExpectCursor {
                visible,
                shape,
                x,
                y,
                ..
            } => {
                let fields = [
                    visible.map(|value| format!("visible={value}")),
                    shape.as_ref().map(|value| format!("shape={value:?}")),
                    x.map(|value| format!("column={value}")),
                    y.map(|value| format!("row={value}")),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(", ");
                value("Cursor", fields)
            }
            Operation::WaitClipboardMatch {
                pattern: expected, ..
            } => value(
                "Clipboard",
                pattern(
                    expected.as_str(),
                    matches!(expected, ClipboardPattern::Regex(_)),
                ),
            ),
            Operation::WaitClipboard { .. } => value("Clipboard", "changes".into()),
            Operation::WaitIdle { .. } => value("Screen", "becomes idle".into()),
            Operation::WaitCommand { .. } => value("Command", "completes".into()),
            Operation::WaitExit { .. } => value("Process", "exits".into()),
            Operation::WaitReady { .. } => value("Shell", "becomes ready".into()),
            Operation::WaitBell { .. } => value("Bell", "a new event arrives".into()),
            Operation::Snapshot {
                name,
                update,
                include_style,
                include_title,
                ..
            } => value(
                &format!("Snapshot {name:?}"),
                format!(
                    "{} (style={include_style}, title={include_title})",
                    if *update {
                        "write or update"
                    } else {
                        "matches stored snapshot"
                    }
                ),
            ),
            _ => return None,
        };
        Some(match serde_json::to_vec(&expectation) {
            Ok(bytes) if bytes.len() <= MAX_EXPECTATION_BYTES => expectation,
            Ok(_) => Self::Unavailable {
                reason: format!(
                    "Expectation exceeded the {MAX_EXPECTATION_BYTES}-byte retention limit"
                ),
            },
            Err(error) => Self::Unavailable {
                reason: format!("Could not serialize expectation: {error}"),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{MatchOccurrence, TextStyle};

    #[test]
    fn preserves_the_complete_locator_and_negation() {
        let mut query = LocatorQuery::text("Save");
        query.within = Some(Box::new(LocatorQuery::text("Settings")));
        query.occurrence = MatchOccurrence::Nth(2);
        query.style = TextStyle {
            foreground: Some("2".into()),
            bold: Some(true),
            ..TextStyle::default()
        };
        let expected = OperationExpectation::capture(&Operation::WaitLocator {
            query: query.clone(),
            not: true,
            timeout_ms: Some(50),
        })
        .unwrap();
        assert_eq!(
            expected,
            OperationExpectation::Locator {
                query: Box::new(query),
                outcome: LocatorExpectation::Hidden,
            }
        );
    }

    #[test]
    fn bounds_operands_and_does_not_capture_input_or_environment() {
        let oversized = OperationExpectation::capture(&Operation::WaitLocator {
            query: LocatorQuery::text("\u{4f60}".repeat(MAX_EXPECTATION_BYTES)),
            not: false,
            timeout_ms: None,
        })
        .unwrap();
        assert!(matches!(
            oversized,
            OperationExpectation::Unavailable { .. }
        ));
        assert!(serde_json::to_vec(&oversized).unwrap().len() < MAX_EXPECTATION_BYTES);
        assert!(OperationExpectation::capture(&Operation::Write {
            data: "secret".into()
        })
        .is_none());
        assert!(OperationExpectation::capture(&Operation::Submit {
            data: Some("secret".into())
        })
        .is_none());
        assert!(OperationExpectation::capture(&Operation::Open(Default::default())).is_none());
    }
}
