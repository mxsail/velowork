use serde::{Deserialize, Serialize};
use super::types::{PanelMode, PanelCollapseState};

/// Layout state for a single tab.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabLayoutState {
    pub panel_id: String,
    pub title: String,
    pub kind_str: String,
}

/// Layout state for a DockPanel (left/right/bottom/etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockPanelLayoutState {
    pub size: f32,
    pub collapse_state: PanelCollapseState,
    pub mode: PanelMode,
    pub tabs: Vec<TabLayoutState>,
    pub active_tab_index: Option<usize>,
}

/// Global DockManager layout state to be serialized/deserialized in workspace settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DockLayoutState {
    pub left: Option<DockPanelLayoutState>,
    pub right: Option<DockPanelLayoutState>,
    pub bottom: Option<DockPanelLayoutState>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_serialization() {
        let layout = DockLayoutState {
            left: Some(DockPanelLayoutState {
                size: 260.0,
                collapse_state: PanelCollapseState::Normal,
                mode: PanelMode::Normal,
                tabs: vec![TabLayoutState {
                    panel_id: "explorer".to_string(),
                    title: "Files".to_string(),
                    kind_str: "Explorer".to_string(),
                }],
                active_tab_index: Some(0),
            }),
            right: None,
            bottom: Some(DockPanelLayoutState {
                size: 220.0,
                collapse_state: PanelCollapseState::Collapsed,
                mode: PanelMode::Maximized,
                tabs: vec![],
                active_tab_index: None,
            }),
        };

        let serialized = serde_json::to_string(&layout).unwrap();
        let deserialized: DockLayoutState = serde_json::from_str(&serialized).unwrap();

        assert!(deserialized.left.is_some());
        assert_eq!(deserialized.left.as_ref().unwrap().size, 260.0);
        assert_eq!(deserialized.left.as_ref().unwrap().mode, PanelMode::Normal);
        assert_eq!(deserialized.left.as_ref().unwrap().tabs[0].panel_id, "explorer");

        assert!(deserialized.right.is_none());
        assert_eq!(deserialized.bottom.as_ref().unwrap().collapse_state, PanelCollapseState::Collapsed);
        assert_eq!(deserialized.bottom.as_ref().unwrap().mode, PanelMode::Maximized);
    }
}
