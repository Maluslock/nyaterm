//! Parser for tmux window layout strings.
//!
//! Format (verified against real `tmux 3.2a` output — see `tests`):
//!
//! ```text
//! layout  := checksum "," geometry ( leaf | "{" node ("," node)* "}" | "[" node ("," node)* "]" )
//! geometry:= WIDTH "x" HEIGHT "," X "," Y
//! checksum:= 4 hex digits
//! leaf    := "," PANE_INDEX
//! ```
//!
//! `{...}` arranges children side by side; `[...]` stacks them. Leaf pane
//! numbers are **indices** within the window (`#{pane_index}`), not the stable
//! `%N` pane ids used by `%output` notifications.

use super::types::{TmuxLayoutNode, TmuxPane, TmuxSplitDirection};

#[derive(Debug, PartialEq, Eq)]
pub enum LayoutParseError {
    UnexpectedEnd,
    BadChecksum,
    BadGeometry,
    BadNumber,
    EmptySplit,
    TrailingInput,
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            bytes: input.as_bytes(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.pos += 1;
        Some(byte)
    }

    fn expect(&mut self, expected: u8) -> Result<(), LayoutParseError> {
        if self.peek() == Some(expected) {
            self.pos += 1;
            Ok(())
        } else {
            Err(LayoutParseError::BadGeometry)
        }
    }

    fn parse_u32(&mut self) -> Result<u32, LayoutParseError> {
        let start = self.pos;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.pos == start {
            return Err(LayoutParseError::BadNumber);
        }
        std::str::from_utf8(&self.bytes[start..self.pos])
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or(LayoutParseError::BadNumber)
    }

    fn parse_u16(&mut self) -> Result<u16, LayoutParseError> {
        let value = self.parse_u32()?;
        u16::try_from(value).map_err(|_| LayoutParseError::BadNumber)
    }

    fn skip_checksum(&mut self) -> Result<(), LayoutParseError> {
        for _ in 0..4 {
            match self.bump() {
                Some(b) if b.is_ascii_hexdigit() => {}
                Some(_) => return Err(LayoutParseError::BadChecksum),
                None => return Err(LayoutParseError::UnexpectedEnd),
            }
        }
        Ok(())
    }

    fn parse_geometry(&mut self) -> Result<(u16, u16, u16, u16), LayoutParseError> {
        let width = self.parse_u16()?;
        self.expect(b'x')?;
        let height = self.parse_u16()?;
        self.expect(b',')?;
        let x = self.parse_u16()?;
        self.expect(b',')?;
        let y = self.parse_u16()?;
        Ok((width, height, x, y))
    }

    fn parse_node(&mut self) -> Result<TmuxLayoutNode, LayoutParseError> {
        // Only the root of a layout string carries a checksum; children are
        // bare geometry followed by either a pane index or a child list.
        let (width, height, x, y) = self.parse_geometry()?;

        match self.peek() {
            Some(b'{') => {
                self.pos += 1;
                self.parse_split(TmuxSplitDirection::Columns, width, height, x, y, b'}')
            }
            Some(b'[') => {
                self.pos += 1;
                self.parse_split(TmuxSplitDirection::Rows, width, height, x, y, b']')
            }
            _ => {
                // Leaf: an index may follow, but trample-tolerant when absent.
                let index = if self.peek() == Some(b',') {
                    self.pos += 1;
                    self.parse_u32()?
                } else {
                    0
                };
                Ok(TmuxLayoutNode::Leaf {
                    width,
                    height,
                    x,
                    y,
                    pane: TmuxPane {
                        index,
                        id: None,
                        width,
                        height,
                        x,
                        y,
                        active: false,
                    },
                })
            }
        }
    }

    fn parse_split(
        &mut self,
        direction: TmuxSplitDirection,
        width: u16,
        height: u16,
        x: u16,
        y: u16,
        closer: u8,
    ) -> Result<TmuxLayoutNode, LayoutParseError> {
        let mut children = Vec::new();
        loop {
            if self.peek() == Some(closer) {
                self.pos += 1;
                break;
            }
            children.push(self.parse_node()?);
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(byte) if byte == closer => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(LayoutParseError::BadGeometry),
            }
        }
        if children.is_empty() {
            return Err(LayoutParseError::EmptySplit);
        }
        Ok(TmuxLayoutNode::Split {
            direction,
            width,
            height,
            x,
            y,
            children,
        })
    }
}

/// Parse a tmux layout string into a recursive layout tree.
pub fn parse_layout(input: &str) -> Result<TmuxLayoutNode, LayoutParseError> {
    let mut parser = Parser::new(input.trim());
    parser.skip_checksum()?;
    parser.expect(b',')?;
    let node = parser.parse_node()?;
    if parser.pos != parser.bytes.len() {
        return Err(LayoutParseError::TrailingInput);
    }
    Ok(node)
}

/// Mark `active` on the pane matching `pane_index`, leaving the rest untouched.
pub fn mark_active(node: &mut TmuxLayoutNode, pane_index: u32) {
    match node {
        TmuxLayoutNode::Leaf { pane, .. } => pane.active = pane.index == pane_index,
        TmuxLayoutNode::Split { children, .. } => {
            for child in children.iter_mut() {
                mark_active(child, pane_index);
            }
        }
    }
}

/// Attach stable `%N` pane ids to layout leaves by pane index.
pub fn apply_pane_ids(node: &mut TmuxLayoutNode, ids_by_index: &std::collections::HashMap<u32, String>) {
    match node {
        TmuxLayoutNode::Leaf { pane, .. } => {
            if let Some(id) = ids_by_index.get(&pane.index) {
                pane.id = Some(id.clone());
            }
        }
        TmuxLayoutNode::Split { children, .. } => {
            for child in children.iter_mut() {
                apply_pane_ids(child, ids_by_index);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn panes(node: &TmuxLayoutNode) -> Vec<TmuxPane> {
        let mut out = Vec::new();
        node.collect_panes(&mut out);
        out
    }

    #[test]
    fn parses_single_pane_window() {
        // Captured from real tmux: window @1 with one pane.
        let node = parse_layout("b260,80x24,0,0,3").expect("parses");
        let panes = panes(&node);
        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].index, 3);
        assert_eq!((panes[0].width, panes[0].height), (80, 24));
        assert_eq!((panes[0].x, panes[0].y), (0, 0));
    }

    #[test]
    fn parses_nested_real_tmux_layout() {
        // Captured from real tmux: window @0 with three panes.
        // {} = side by side, [] = stacked.
        let node = parse_layout(
            "d67e,80x24,0,0{40x24,0,0,0,39x24,41,0[39x12,41,0,1,39x11,41,13,2]}",
        )
        .expect("parses");

        match &node {
            TmuxLayoutNode::Split {
                direction,
                children,
                ..
            } => {
                assert_eq!(*direction, TmuxSplitDirection::Columns);
                assert_eq!(children.len(), 2);
            }
            other => panic!("expected columns split, got {other:?}"),
        }

        // Flattened panes must match `tmux list-panes` exactly.
        let panes = panes(&node);
        let geometry: Vec<(u32, u16, u16, u16, u16)> = panes
            .iter()
            .map(|p| (p.index, p.width, p.height, p.x, p.y))
            .collect();
        assert_eq!(
            geometry,
            vec![
                (0, 40, 24, 0, 0),
                (1, 39, 12, 41, 0),
                (2, 39, 11, 41, 13),
            ]
        );
    }

    #[test]
    fn parses_man_page_top_bottom_layout() {
        let node =
            parse_layout("b25d,80x24,0,0[80x12,0,0,1,80x11,0,13,2]").expect("parses");
        match &node {
            TmuxLayoutNode::Split { direction, .. } => {
                assert_eq!(*direction, TmuxSplitDirection::Rows)
            }
            other => panic!("expected rows split, got {other:?}"),
        }
        assert_eq!(panes(&node).len(), 2);
    }

    #[test]
    fn rejects_malformed_input() {
        assert!(parse_layout("").is_err());
        assert!(parse_layout("zzzz,80x24,0,0,1").is_err());
        assert!(parse_layout("b260,80x24,0,0,1junk").is_err());
        assert!(parse_layout("b260,80x24,0,0{}").is_err());
        assert!(parse_layout("b260,80x").is_err());
    }

    #[test]
    fn marks_active_pane() {
        let mut node =
            parse_layout("d67e,80x24,0,0{40x24,0,0,0,39x24,41,0[39x12,41,0,1,39x11,41,13,2]}")
                .expect("parses");
        mark_active(&mut node, 2);
        let panes = panes(&node);
        assert_eq!(
            panes.iter().map(|p| p.active).collect::<Vec<_>>(),
            vec![false, false, true]
        );
    }

    #[test]
    fn applies_pane_ids_by_index() {
        let mut node = parse_layout("b260,80x24,0,0,3").expect("parses");
        let mut ids = HashMap::new();
        ids.insert(3u32, "%7".to_string());
        apply_pane_ids(&mut node, &ids);
        assert_eq!(panes(&node)[0].id.as_deref(), Some("%7"));
    }

    /// The frontend consumes this exact JSON shape; lock the contract down.
    #[test]
    fn serializes_to_the_shape_the_frontend_expects() {
        let node = parse_layout(
            "d67e,80x24,0,0{40x24,0,0,0,39x24,41,0[39x12,41,0,1,39x11,41,13,2]}",
        )
        .expect("parses");
        let json = serde_json::to_value(&node).expect("serializes");

        assert_eq!(json["kind"], "split");
        assert_eq!(json["direction"], "columns");
        assert_eq!(json["width"], 80);
        assert_eq!(json["children"].as_array().expect("children").len(), 2);

        let first = &json["children"][0];
        assert_eq!(first["kind"], "leaf");
        assert_eq!(first["pane"]["index"], 0);
        assert_eq!(first["pane"]["width"], 40);

        let nested = &json["children"][1];
        assert_eq!(nested["kind"], "split");
        assert_eq!(nested["direction"], "rows");
        assert_eq!(nested["children"].as_array().expect("children").len(), 2);
        assert_eq!(nested["children"][1]["pane"]["index"], 2);
    }
}
