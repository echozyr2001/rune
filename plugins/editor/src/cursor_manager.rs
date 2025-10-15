//! Cursor position manager for mapping between raw and rendered positions

use crate::editor_state::CursorPosition;
use crate::inline_renderer::RenderedElement;
use crate::syntax_parser::{PositionRange, SyntaxElement};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Mapping between raw markdown positions and rendered HTML positions
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositionMapping {
    pub raw_position: usize,
    pub rendered_position: usize,
    pub element_id: Option<String>,
}

/// Element mapping information for cursor position tracking
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElementMapping {
    pub raw_range: PositionRange,
    pub rendered_range: PositionRange,
    pub element_type: String,
    pub is_active: bool,
}

/// Statistics about cursor manager mappings
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingStats {
    pub element_count: usize,
    pub position_mapping_count: usize,
    pub active_element_count: usize,
    pub raw_content_length: usize,
    pub rendered_content_length: usize,
}

/// Cursor manager for handling position mapping between raw and rendered content
#[derive(Debug, Clone)]
pub struct CursorManager {
    raw_position: usize,
    rendered_position: Option<usize>,
    element_mappings: HashMap<String, ElementMapping>,
    position_mappings: Vec<PositionMapping>,
    raw_content_length: usize,
    rendered_content_length: usize,
}

impl CursorManager {
    pub fn new() -> Self {
        Self {
            raw_position: 0,
            rendered_position: None,
            element_mappings: HashMap::new(),
            position_mappings: Vec::new(),
            raw_content_length: 0,
            rendered_content_length: 0,
        }
    }

    pub fn with_position(raw_position: usize) -> Self {
        Self {
            raw_position,
            rendered_position: None,
            element_mappings: HashMap::new(),
            position_mappings: Vec::new(),
            raw_content_length: 0,
            rendered_content_length: 0,
        }
    }

    pub fn raw_position(&self) -> usize {
        self.raw_position
    }

    pub fn rendered_position(&self) -> Option<usize> {
        self.rendered_position
    }

    pub fn map_raw_to_rendered(&self, raw_pos: usize) -> Option<usize> {
        if let Some(mapping) = self
            .position_mappings
            .iter()
            .find(|m| m.raw_position == raw_pos)
        {
            return Some(mapping.rendered_position);
        }

        for element_mapping in self.element_mappings.values() {
            if element_mapping.raw_range.contains(raw_pos) {
                let relative_pos = raw_pos - element_mapping.raw_range.start;
                let raw_element_length = element_mapping.raw_range.len();
                let rendered_element_length = element_mapping.rendered_range.len();

                if raw_element_length == 0 {
                    return Some(element_mapping.rendered_range.start);
                }

                let ratio = relative_pos as f64 / raw_element_length as f64;
                let rendered_offset = (ratio * rendered_element_length as f64) as usize;

                return Some(element_mapping.rendered_range.start + rendered_offset);
            }
        }

        if self.raw_content_length > 0 && self.rendered_content_length > 0 {
            let ratio = raw_pos as f64 / self.raw_content_length as f64;
            let estimated_pos = (ratio * self.rendered_content_length as f64) as usize;
            Some(estimated_pos.min(self.rendered_content_length))
        } else {
            None
        }
    }

    pub fn map_rendered_to_raw(&self, rendered_pos: usize) -> usize {
        if let Some(mapping) = self
            .position_mappings
            .iter()
            .find(|m| m.rendered_position == rendered_pos)
        {
            return mapping.raw_position;
        }

        for element_mapping in self.element_mappings.values() {
            if element_mapping.rendered_range.contains(rendered_pos) {
                let relative_pos = rendered_pos - element_mapping.rendered_range.start;
                let rendered_element_length = element_mapping.rendered_range.len();
                let raw_element_length = element_mapping.raw_range.len();

                if rendered_element_length == 0 {
                    return element_mapping.raw_range.start;
                }

                let ratio = relative_pos as f64 / rendered_element_length as f64;
                let raw_offset = (ratio * raw_element_length as f64) as usize;

                return element_mapping.raw_range.start + raw_offset;
            }
        }

        if self.rendered_content_length > 0 && self.raw_content_length > 0 {
            let ratio = rendered_pos as f64 / self.rendered_content_length as f64;
            let estimated_pos = (ratio * self.raw_content_length as f64) as usize;
            estimated_pos.min(self.raw_content_length)
        } else {
            rendered_pos.min(self.raw_content_length)
        }
    }

    pub fn update_element_mappings(
        &mut self,
        syntax_elements: &[SyntaxElement],
        rendered_elements: &[RenderedElement],
        raw_content: &str,
        rendered_content: &str,
    ) {
        self.raw_content_length = raw_content.len();
        self.rendered_content_length = rendered_content.len();
        self.element_mappings.clear();
        self.position_mappings.clear();

        for (i, syntax_element) in syntax_elements.iter().enumerate() {
            let element_id = format!("element_{}", i);

            if let Some(rendered_element) = rendered_elements.get(i) {
                let element_mapping = ElementMapping {
                    raw_range: syntax_element.range.clone(),
                    rendered_range: PositionRange::new(
                        rendered_element.position_range.0,
                        rendered_element.position_range.1,
                    ),
                    element_type: format!("{:?}", syntax_element.element_type),
                    is_active: syntax_element.is_active,
                };

                self.element_mappings
                    .insert(element_id.clone(), element_mapping);

                self.position_mappings.push(PositionMapping {
                    raw_position: syntax_element.range.start,
                    rendered_position: rendered_element.position_range.0,
                    element_id: Some(element_id.clone()),
                });

                self.position_mappings.push(PositionMapping {
                    raw_position: syntax_element.range.end,
                    rendered_position: rendered_element.position_range.1,
                    element_id: Some(element_id),
                });
            }
        }

        self.position_mappings.sort_by_key(|m| m.raw_position);
        self.rendered_position = self.map_raw_to_rendered(self.raw_position);
    }

    pub fn handle_content_change(
        &mut self,
        change_range: &PositionRange,
        _old_content: &str,
        new_content: &str,
    ) {
        let change_delta =
            new_content.len() as i32 - (change_range.end - change_range.start) as i32;
        self.raw_content_length = (self.raw_content_length as i32 + change_delta) as usize;

        if self.raw_position >= change_range.end {
            self.raw_position = (self.raw_position as i32 + change_delta) as usize;
        } else if self.raw_position >= change_range.start {
            self.raw_position = change_range.start + new_content.len();
        }

        let mut updated_mappings = HashMap::new();
        for (id, mut mapping) in self.element_mappings.drain() {
            if mapping.raw_range.start >= change_range.end {
                mapping.raw_range.start = (mapping.raw_range.start as i32 + change_delta) as usize;
                mapping.raw_range.end = (mapping.raw_range.end as i32 + change_delta) as usize;
                updated_mappings.insert(id, mapping);
            } else if mapping.raw_range.end <= change_range.start {
                updated_mappings.insert(id, mapping);
            }
        }
        self.element_mappings = updated_mappings;

        self.position_mappings.retain_mut(|mapping| {
            if mapping.raw_position >= change_range.end {
                mapping.raw_position = (mapping.raw_position as i32 + change_delta) as usize;
                true
            } else {
                mapping.raw_position < change_range.start
            }
        });

        self.rendered_position = None;
    }

    pub fn preserve_position_for_mode_switch(
        &mut self,
        from_raw: bool,
        to_raw: bool,
    ) -> Result<CursorPosition, String> {
        match (from_raw, to_raw) {
            (true, false) => {
                if let Some(rendered_pos) = self.map_raw_to_rendered(self.raw_position) {
                    self.rendered_position = Some(rendered_pos);
                    Ok(CursorPosition::new(0, rendered_pos, rendered_pos))
                } else {
                    Err("Could not map raw position to rendered position".to_string())
                }
            }
            (false, true) => {
                if let Some(rendered_pos) = self.rendered_position {
                    let raw_pos = self.map_rendered_to_raw(rendered_pos);
                    self.raw_position = raw_pos;
                    Ok(CursorPosition::new(0, raw_pos, raw_pos))
                } else {
                    Ok(CursorPosition::new(0, self.raw_position, self.raw_position))
                }
            }
            _ => {
                let position = if to_raw {
                    self.raw_position
                } else {
                    self.rendered_position.unwrap_or(0)
                };
                Ok(CursorPosition::new(0, position, position))
            }
        }
    }

    pub fn get_element_at_cursor(&self) -> Option<&ElementMapping> {
        self.element_mappings
            .values()
            .find(|mapping| mapping.raw_range.contains(self.raw_position))
    }

    pub fn is_cursor_in_active_element(&self) -> bool {
        self.get_element_at_cursor()
            .is_some_and(|mapping| mapping.is_active)
    }

    pub fn set_element_active(&mut self, element_id: &str, active: bool) {
        if let Some(mapping) = self.element_mappings.get_mut(element_id) {
            mapping.is_active = active;
        }
    }

    pub fn get_active_elements(&self) -> Vec<(&String, &ElementMapping)> {
        self.element_mappings
            .iter()
            .filter(|(_, mapping)| mapping.is_active)
            .collect()
    }

    pub fn clear_mappings(&mut self) {
        self.element_mappings.clear();
        self.position_mappings.clear();
        self.rendered_position = None;
    }

    pub fn get_mapping_stats(&self) -> MappingStats {
        MappingStats {
            element_count: self.element_mappings.len(),
            position_mapping_count: self.position_mappings.len(),
            active_element_count: self
                .element_mappings
                .values()
                .filter(|m| m.is_active)
                .count(),
            raw_content_length: self.raw_content_length,
            rendered_content_length: self.rendered_content_length,
        }
    }
}

impl Default for CursorManager {
    fn default() -> Self {
        Self::new()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax_parser::{SyntaxElement, SyntaxElementType};

    #[test]
    fn test_cursor_manager_creation() {
        let manager = CursorManager::new();
        assert_eq!(manager.raw_position(), 0);
        assert_eq!(manager.rendered_position(), None);
    }

    #[test]
    fn test_cursor_manager_with_position() {
        let manager = CursorManager::with_position(10);
        assert_eq!(manager.raw_position(), 10);
        assert_eq!(manager.rendered_position(), None);
    }

    #[test]
    fn test_basic_position_mapping() {
        let mut manager = CursorManager::new();
        manager.raw_content_length = 100;
        manager.rendered_content_length = 120;

        // Test proportional mapping when no specific mappings exist
        let rendered_pos = manager.map_raw_to_rendered(50);
        assert_eq!(rendered_pos, Some(60)); // 50/100 * 120 = 60

        let raw_pos = manager.map_rendered_to_raw(60);
        assert_eq!(raw_pos, 50); // 60/120 * 100 = 50
    }

    #[test]
    fn test_element_mapping_updates() {
        let mut manager = CursorManager::new();

        let syntax_elements = vec![SyntaxElement::new(
            SyntaxElementType::Bold,
            PositionRange::new(0, 8),
            "**bold**".to_string(),
            "bold".to_string(),
        )];

        let rendered_elements = vec![RenderedElement::new(
            "<strong>bold</strong>".to_string(),
            vec!["bold".to_string()],
            "**bold**".to_string(),
            (0, 21),
        )];

        manager.update_element_mappings(
            &syntax_elements,
            &rendered_elements,
            "**bold**",
            "<strong>bold</strong>",
        );

        assert_eq!(manager.element_mappings.len(), 1);
        assert_eq!(manager.position_mappings.len(), 2); // Start and end positions
        assert_eq!(manager.raw_content_length, 8);
        assert_eq!(manager.rendered_content_length, 21);
    }

    #[test]
    fn test_content_change_handling() {
        let mut manager = CursorManager::new();
        manager.raw_content_length = 20;
        manager.raw_position = 15;

        let change_range = PositionRange::new(5, 10);
        manager.handle_content_change(&change_range, "old", "new content");

        // Position after change should be adjusted
        // 15 + (11 - 5) = 15 + 6 = 21
        assert_eq!(manager.raw_position, 21);
        assert_eq!(manager.raw_content_length, 26); // 20 + (11 - 5) = 26
    }

    #[test]
    fn test_element_at_cursor() {
        let mut manager = CursorManager::new();

        let element_mapping = ElementMapping {
            raw_range: PositionRange::new(5, 15),
            rendered_range: PositionRange::new(10, 25),
            element_type: "Bold".to_string(),
            is_active: true,
        };

        manager
            .element_mappings
            .insert("test_element".to_string(), element_mapping);
        manager.raw_position = 10;

        let element = manager.get_element_at_cursor();
        assert!(element.is_some());
        assert!(element.unwrap().is_active);
        assert!(manager.is_cursor_in_active_element());
    }

    #[test]
    fn test_mapping_stats() {
        let mut manager = CursorManager::new();
        manager.raw_content_length = 100;
        manager.rendered_content_length = 150;

        let stats = manager.get_mapping_stats();
        assert_eq!(stats.element_count, 0);
        assert_eq!(stats.position_mapping_count, 0);
        assert_eq!(stats.active_element_count, 0);
        assert_eq!(stats.raw_content_length, 100);
        assert_eq!(stats.rendered_content_length, 150);
    }
}
