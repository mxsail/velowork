//! Shell selector for tab groups

use crate::ActionDispatch;
use velowork_terminal::shell_config::ShellType;
use velowork_ui::theme::theme;
use velowork_ui::chip::shell_indicator_chip;
use crate::layout::layout_container::LayoutContainer;
use velowork_workspace::state::LayoutNode;
use velowork_i18n::i18n;
use gpui::prelude::*;
use gpui::*;
use velowork_ui::tooltip::Tooltip;

impl<D: ActionDispatch + Send + Sync> LayoutContainer<D> {
    pub(super) fn get_active_terminal_id(&self, active_tab: usize, cx: &Context<Self>) -> Option<String> {
        let ws = self.workspace.read(cx);
        match self.get_layout(ws) {
            Some(LayoutNode::Tabs { children, .. }) => {
                if let Some(LayoutNode::Terminal { terminal_id, .. }) = children.get(active_tab) {
                    return terminal_id.clone();
                }
            }
            Some(LayoutNode::Terminal { terminal_id, .. }) => {
                return terminal_id.clone();
            }
            _ => {}
        }
        None
    }

    pub(super) fn get_active_shell_type(&self, active_tab: usize, cx: &Context<Self>) -> ShellType {
        let ws = self.workspace.read(cx);
        match self.get_layout(ws) {
            Some(LayoutNode::Tabs { children, .. }) => {
                if let Some(LayoutNode::Terminal { shell_type, .. }) = children.get(active_tab) {
                    return shell_type.clone();
                }
            }
            Some(LayoutNode::Terminal { shell_type, .. }) => {
                return shell_type.clone();
            }
            _ => {}
        }
        ShellType::Default
    }

    pub(super) fn render_shell_indicator(&self, active_tab: usize, cx: &Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let shell_type = self.get_active_shell_type(active_tab, cx);
        let shell_name = match &shell_type {
            ShellType::Default => i18n!(cx, "common.default"),
            ShellType::Custom { .. } => i18n!(cx, "common.custom"),
            #[allow(unreachable_patterns)]
            _ => shell_type.short_display_name().to_string(),
        };
        let id_suffix = format!("tabs-{:?}", self.layout_path);
        let terminal_id = self.get_active_terminal_id(active_tab, cx);
        let project_id = self.project_id.clone();
        let request_broker = self.request_broker.clone();

        shell_indicator_chip(format!("shell-indicator-{}", id_suffix), &shell_name, &t, cx)
            .when_some(terminal_id, |el, tid| {
                el.on_mouse_down(MouseButton::Left, move |_, _window, cx| {
                    cx.stop_propagation();
                    request_broker.update(cx, |broker, cx| {
                        broker.push_overlay_request(velowork_workspace::requests::OverlayRequest::Project(velowork_workspace::requests::ProjectOverlay {
                            project_id: project_id.clone(),
                            kind: velowork_workspace::requests::ProjectOverlayKind::ShellSelector {
                                terminal_id: tid.clone(),
                                current_shell: shell_type.clone(),
                            },
                        }), cx);
                    });
                })
            })
            .tooltip(move |_, cx| { let __tip = i18n!(cx, "terminal.shell_selector_title"); cx.new(|_| Tooltip::new(__tip)).into() })
    }
}
