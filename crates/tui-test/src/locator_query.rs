//! Typed, non-recursive expression transport shared by the native bindings.

use serde::{Deserialize, Serialize};

use crate::api::{
    LinkSelector, LocatorDirection, LocatorQuery, LocatorSelector, MatchOccurrence, StyleSelector,
    TextSelector, TextStyle, TuiTestError, WhitespaceMode,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocatorNodeKind {
    Text,
    Style,
    Link,
    And,
    Or,
    Filter,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocatorNode {
    pub kind: LocatorNodeKind,
    pub text: Option<String>,
    pub regex: Option<bool>,
    pub whitespace: Option<WhitespaceMode>,
    pub style: Option<TextStyle>,
    pub link: Option<String>,
    pub full: Option<bool>,
    pub direction: Option<LocatorDirection>,
    pub within: Option<usize>,
    pub left: Option<usize>,
    pub right: Option<usize>,
    pub input: Option<usize>,
    pub has: Option<usize>,
    pub has_not: Option<usize>,
    pub occurrence: MatchOccurrence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocatorExpression {
    pub nodes: Vec<LocatorNode>,
    pub root: usize,
}

impl LocatorExpression {
    pub fn into_query(self) -> Result<LocatorQuery, TuiTestError> {
        if self.nodes.is_empty() || self.nodes.len() > 256 || self.root >= self.nodes.len() {
            return Err(TuiTestError::usage(
                "locator expression requires 1..256 nodes and a valid root",
            ));
        }

        let mut depths: Vec<usize> = Vec::new();
        let mut sizes: Vec<usize> = Vec::new();
        for (index, node) in self.nodes.iter().enumerate() {
            let error = |message: &str| TuiTestError::usage(format!("nodes[{index}]: {message}"));
            let references = [
                node.within,
                node.left,
                node.right,
                node.input,
                node.has,
                node.has_not,
            ];
            let mut depth = 1;
            let mut size = 1;
            for reference in references.into_iter().flatten() {
                if reference >= index {
                    return Err(error("operand references must point to earlier nodes"));
                }
                depth = depth.max(depths[reference] + 1);
                size += sizes[reference];
            }
            if depth > 64 || size > 4096 {
                return Err(error("locator expression exceeds the size or depth limit"));
            }
            let binary = matches!(node.kind, LocatorNodeKind::And | LocatorNodeKind::Or);
            let filter = node.kind == LocatorNodeKind::Filter;
            let leaf = !binary && !filter;
            if (node.kind != LocatorNodeKind::Text
                && (node.text.is_some() || node.regex.is_some() || node.whitespace.is_some()))
                || (node.kind != LocatorNodeKind::Style && node.style.is_some())
                || (node.kind != LocatorNodeKind::Link && node.link.is_some())
                || (!leaf
                    && (node.full.is_some() || node.within.is_some() || node.direction.is_some()))
                || (!binary && (node.left.is_some() || node.right.is_some()))
                || (!filter
                    && (node.input.is_some() || node.has.is_some() || node.has_not.is_some()))
            {
                return Err(error("fields do not match locator node kind"));
            }
            match node.kind {
                LocatorNodeKind::Text if node.text.is_none() => {
                    return Err(error("text locator requires text"));
                }
                LocatorNodeKind::Style => {
                    let style = node
                        .style
                        .as_ref()
                        .ok_or_else(|| error("style locator requires style"))?;
                    if style.is_empty() {
                        return Err(error("getByStyle requires at least one style property"));
                    }
                }
                LocatorNodeKind::Link if node.link.is_none() => {
                    return Err(error("link locator requires link"));
                }
                LocatorNodeKind::And | LocatorNodeKind::Or
                    if node.left.is_none() || node.right.is_none() =>
                {
                    return Err(error("binary locator requires left and right"));
                }
                LocatorNodeKind::Filter => {
                    if node.input.is_none() {
                        return Err(error("filter requires input"));
                    }
                    if node.has.is_none() && node.has_not.is_none() {
                        return Err(error("filter requires has or hasNot"));
                    }
                }
                _ => {}
            };
            let direction = node.direction.unwrap_or_default();
            if node.within.is_none() && direction != LocatorDirection::Within {
                return Err(error("locator direction requires a parent locator"));
            }
            depths.push(depth);
            sizes.push(size);
        }
        self.materialize_query(self.root, &mut 4096)
    }

    // Validate the entire table first, but expand only the reachable root tree.
    fn materialize_query(
        &self,
        index: usize,
        remaining: &mut usize,
    ) -> Result<LocatorQuery, TuiTestError> {
        if *remaining == 0 {
            return Err(TuiTestError::usage(
                "locator expression exceeds the expansion limit",
            ));
        }
        *remaining -= 1;
        let node = &self.nodes[index];
        let mut child = |index| self.materialize_query(index, remaining).map(Box::new);
        let selector = match node.kind {
            LocatorNodeKind::Text => LocatorSelector::Text(TextSelector {
                text: node.text.clone().expect("validated text"),
                regex: node.regex.unwrap_or(false),
                full: node.full.unwrap_or(false),
                whitespace: node.whitespace.unwrap_or_default(),
                scope: Default::default(),
            }),
            LocatorNodeKind::Style => LocatorSelector::Style(StyleSelector {
                style: node.style.clone().expect("validated style"),
                full: node.full.unwrap_or(false),
            }),
            LocatorNodeKind::Link => LocatorSelector::Link(LinkSelector {
                uri: node.link.clone().expect("validated link"),
                full: node.full.unwrap_or(false),
            }),
            LocatorNodeKind::And => LocatorSelector::And {
                left: child(node.left.expect("validated left"))?,
                right: child(node.right.expect("validated right"))?,
            },
            LocatorNodeKind::Or => LocatorSelector::Or {
                left: child(node.left.expect("validated left"))?,
                right: child(node.right.expect("validated right"))?,
            },
            LocatorNodeKind::Filter => LocatorSelector::Filter {
                input: child(node.input.expect("validated input"))?,
                has: node.has.map(&mut child).transpose()?,
                has_not: node.has_not.map(&mut child).transpose()?,
            },
        };
        Ok(LocatorQuery {
            occurrence: node.occurrence.clone(),
            within: node.within.map(&mut child).transpose()?,
            direction: node.direction.unwrap_or_default(),
            ..LocatorQuery::new(selector)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn expression(nodes: serde_json::Value, root: usize) -> LocatorExpression {
        serde_json::from_value(json!({ "nodes": nodes, "root": root })).unwrap()
    }

    #[test]
    fn expressions_round_trip_with_branch_selection_and_full_grid() {
        let query = expression(
            json!([
                {"kind": "text", "text": "Docs", "occurrence": "first"},
                {"kind": "link", "link": "", "full": true, "occurrence": "any"},
                {"kind": "and", "left": 0, "right": 1, "occurrence": {"nth": 1}},
                {"kind": "filter", "input": 2, "has_not": 1, "occurrence": "any"}
            ]),
            3,
        )
        .into_query()
        .unwrap();
        assert!(query.uses_full_grid());
        let value = serde_json::to_value(&query).unwrap();
        assert_eq!(
            serde_json::from_value::<LocatorQuery>(value).unwrap(),
            query
        );
        let LocatorSelector::Filter { input, .. } = query.selector else {
            panic!("expected filter");
        };
        assert_eq!(input.occurrence, MatchOccurrence::Nth(1));
    }

    #[test]
    fn malformed_graphs_are_usage_errors() {
        for nodes in [
            json!([]),
            json!([{"kind": "and", "left": 0, "right": 0, "occurrence": "any"}]),
            json!([{"kind": "link", "occurrence": "any"}]),
            json!([{"kind": "style", "style": {}, "occurrence": "any"}]),
            json!([{"kind": "text", "text": "x", "style": {"bold": true}, "occurrence": "any"}]),
            json!([
                {"kind": "text", "text": "x", "occurrence": "any"},
                {"kind": "filter", "input": 0, "occurrence": "any"}
            ]),
        ] {
            assert_eq!(
                expression(nodes, 0).into_query().unwrap_err().kind,
                crate::ErrorKind::Usage
            );
        }
        assert!(expression(
            json!([{"kind": "link", "link": "", "occurrence": "any"}]),
            1
        )
        .into_query()
        .is_err());
    }

    #[test]
    fn expanded_graphs_and_deep_queries_are_bounded() {
        let mut nodes = vec![json!({"kind": "text", "text": "x", "occurrence": "any"})];
        for index in 1..14 {
            nodes.push(
                json!({"kind": "or", "left": index - 1, "right": index - 1, "occurrence": "any"}),
            );
        }
        assert!(expression(json!(nodes), 13)
            .into_query()
            .unwrap_err()
            .message
            .contains("limit"));
        let nodes: Vec<_> = (0usize..65)
            .map(|index| {
                json!({
                    "kind": "text", "text": "x", "within": index.checked_sub(1), "occurrence": "any"
                })
            })
            .collect();
        assert!(expression(json!(nodes), 64).into_query().is_err());
    }

    #[test]
    fn removed_style_link_and_filter_properties_are_rejected() {
        assert!(
            serde_json::from_value::<TextStyle>(json!({"bold": true, "link": "test:x"})).is_err()
        );
        assert!(serde_json::from_value::<LocatorNode>(json!({
            "kind": "filter", "input": 0, "has": 1, "has_text": "x", "occurrence": "any"
        }))
        .is_err());
    }

    #[test]
    fn unreachable_nodes_are_validated_without_expanding_their_trees() {
        let mut nodes = vec![json!({"kind": "text", "text": "x", "occurrence": "any"})];
        for index in 1..12 {
            nodes.push(
                json!({"kind": "or", "left": index - 1, "right": index - 1, "occurrence": "any"}),
            );
        }
        while nodes.len() < 256 {
            nodes.push(json!({"kind": "or", "left": 10, "right": 10, "occurrence": "any"}));
        }
        let expression = expression(json!(nodes), 0);
        assert_eq!(
            expression.clone().into_query().unwrap(),
            LocatorQuery::text("x")
        );
        let mut remaining = 4096;
        expression.materialize_query(0, &mut remaining).unwrap();
        assert_eq!(remaining, 4095, "only the reachable node is materialized");
        let mut budget = 4;
        assert!(expression.materialize_query(11, &mut budget).is_err());
        let mut invalid = expression;
        invalid.nodes[255].right = Some(255);
        assert!(
            invalid.into_query().is_err(),
            "unreachable structure is still validated"
        );
    }
}
