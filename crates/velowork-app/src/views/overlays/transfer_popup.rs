//! Transfer manager popup.
//!
//! Rendered at `WindowView` level via `OverlayManager` (like the other
//! positioned popups — context menus, color picker). The popup owns a
//! full-window backdrop that `occlude()`s and closes on any outside
//! `MouseDown`, so clicking elsewhere (including the status-bar trigger)
//! dismisses it without leaking into the terminal underneath.

use gpui::*;
use velowork_ui::icon::AppIcon;
use gpui::prelude::*;
use velowork_i18n::i18n;
use velowork_ui::theme::{surface_bg, theme, ThemeColors};
use velowork_ui::tooltip::{Tooltip, TooltipDirection};
use velowork_ui::tokens::{ui_text_md, SPACE_SM, SPACE_MD, SPACE_XL, RADIUS_MD, RADIUS_STD, RADIUS_LG, ICON_STD};
use velowork_ui::h_flex;
use velowork_views_terminal::transfer_store::{
    format_amount, format_bytes, format_speed, TransferDirection, TransferStatus, TransferStore,
    TransferTask,
};

/// Event emitted by `TransferPopup`.
pub enum TransferPopupEvent {
    /// Request to dismiss the popup (collapse button / outside click).
    Close,
}

/// Transfer manager popup shown above the status-bar transfer button.
pub struct TransferPopup {
    /// Backing store of in-flight / finished transfer tasks.
    store: Entity<TransferStore>,
    /// Anchor point (top-right of the trigger button) used to position the
    /// popup above it via `Anchor::BottomRight`.
    anchor: Point<Pixels>,
    /// Popup height in pixels.
    height: Pixels,
    /// Active drag resize state: (start_mouse_y, start_height).
    drag_start: Option<(Pixels, Pixels)>,
}

impl TransferPopup {
    pub fn new(
        store: Entity<TransferStore>,
        anchor: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> Self {
        let height = px(velowork_app_core::settings::settings(cx).transfer_popup_height);
        Self {
            store,
            anchor,
            height,
            drag_start: None,
        }
    }

    fn close(&self, cx: &mut Context<Self>) {
        if self.drag_start.is_none() {
            cx.emit(TransferPopupEvent::Close);
        }
    }
}

impl EventEmitter<TransferPopupEvent> for TransferPopup {}

impl Render for TransferPopup {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let anchor = self.anchor;

        let drag_overlay = if self.drag_start.is_some() {
            let ent = cx.entity().downgrade();
            Some(
                canvas(
                    |_bounds, window, _cx| {
                        window.insert_hitbox(
                            Bounds::new(point(px(0.0), px(0.0)), window.viewport_size()),
                            HitboxBehavior::BlockMouseExceptScroll,
                        )
                    },
                    move |_bounds, hitbox, window, _cx| {
                        window.set_cursor_style(CursorStyle::ResizeUpDown, &hitbox);

                        let ent_move = ent.clone();
                        window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                            if phase != DispatchPhase::Bubble {
                                return;
                            }
                            if let Some(entity) = ent_move.upgrade() {
                                let _ = entity.update(cx, |this, cx| {
                                    if let Some((start_y, start_h)) = this.drag_start {
                                        let viewport = window.viewport_size();
                                        let max_h = (viewport.height - px(80.0)).max(px(200.0));
                                        let delta_y = e.position.y - start_y;
                                        // Moving upward (negative delta) increases popup height
                                        let new_h = (start_h - delta_y).clamp(px(200.0), max_h);
                                        if this.height != new_h {
                                            this.height = new_h;
                                            cx.notify();
                                        }
                                    }
                                });
                            }
                        });

                        let ent_up = ent.clone();
                        window.on_mouse_event(move |e: &MouseUpEvent, phase, _window, cx| {
                            if phase != DispatchPhase::Bubble || e.button != MouseButton::Left {
                                return;
                            }
                            if let Some(entity) = ent_up.upgrade() {
                                let _ = entity.update(cx, |this, cx| {
                                    if let Some(_) = this.drag_start.take() {
                                        let h = f32::from(this.height);
                                        velowork_app_core::settings::settings_entity(cx).update(cx, |s, cx| {
                                            s.set_transfer_popup_height(h, cx);
                                        });
                                        cx.notify();
                                    }
                                });
                            }
                        });
                    },
                )
                .absolute()
                .inset_0(),
            )
        } else {
            None
        };

        div()
            .absolute()
            .inset_0()
            .occlude()
            .id("transfer-popup-backdrop")
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _w, cx| this.close(cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, _, _w, cx| this.close(cx)))
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(deferred(
                anchored()
                    .position(anchor)
                    .anchor(Anchor::BottomRight)
                    .snap_to_window()
                    .child(self.render_popup(&t, window, cx)),
            ))
            .children(drag_overlay)
    }
}

impl TransferPopup {
    /// Renders the popup panel (toolbar + scrollable task list).
    fn render_popup(&mut self, t: &ThemeColors, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let store = self.store.read(cx);
        let tasks = store.tasks.clone();
        // Whether at least one transfer is currently in-flight (active).
        let has_active = tasks.iter().any(|t| t.status == TransferStatus::Active);
        let title = i18n!(cx, "transfers.label");
        let clear_label = i18n!(cx, "transfers.clear_done");
        let pause_label = i18n!(cx, "transfers.pause_all");
        let collapse_label = i18n!(cx, "transfers.collapse");
        let empty_label = i18n!(cx, "transfers.empty");

        let store_clear = self.store.clone();
        let store_pause = self.store.clone();

        let items: Vec<AnyElement> = if tasks.is_empty() {
            vec![
                div()
                    .id("transfer-empty")
                    .p(SPACE_XL)
                    .text_size(ui_text_md(cx))
                    .text_color(rgb(t.text_muted))
                    .child(empty_label)
                    .into_any_element(),
            ]
        } else {
            tasks.iter().map(|task| self.render_item(task, t, window, cx)).collect()
        };

        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let max_h = (window.viewport_size().height - px(80.0)).max(px(200.0));
        let cur_h = self.height.clamp(px(200.0), max_h);

        let top_handle = div()
            .id("transfer-resize-top")
            .absolute()
            .top_0()
            .left_0()
            .right(px(84.0))
            .h(px(8.0))
            .cursor(CursorStyle::ResizeUpDown)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, _w, cx| {
                    cx.stop_propagation();
                    this.drag_start = Some((ev.position.y, this.height));
                    cx.notify();
                }),
            );

        div()
            .id("transfer-popup")
            .relative()
            .w(px(500.0))
            .h(cur_h)
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(p.surface_overlay)
            .border_1()
            .border_color(p.border_subtle)
            .rounded(RADIUS_LG)
            .shadow_lg()
            .child(top_handle)
            // Stop mousedown from bubbling up to the backdrop so in-panel
            // button clicks (clear / pause / collapse) fire their own `on_click`
            // instead of dismissing the popup prematurely.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            // Toolbar
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(10.0))
                    .py(SPACE_MD)
                    .border_b_1()
                    .border_color(p.border_subtle)
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(t.text_secondary))
                            .child(title),
                    )
                    .child({
                        let clear_trigger = div()
                            .id("transfer-clear-done")
                            .w(px(24.0))
                            .h(px(24.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(RADIUS_STD)
                            .text_color(rgb(t.text_muted))
                            .cursor_pointer()
                            .hover(|s| s.bg(surface_bg(t.bg_hover, cx)))
                            .on_click(cx.listener(move |_this, _ev, _w, cx| {
                                store_clear.update(cx, |s, cx| {
                                    s.clear_completed();
                                    cx.notify();
                                });
                            }))
                            .child(
                                AppIcon::Trash
                                    .size(px(13.0))
                                    .text_color(rgb(t.text_secondary)),
                            );
                        let clear_btn =
                            clear_trigger.tooltip(move |_, cx| {
                                cx.new(|_| Tooltip::new(clear_label.clone()).direction(TooltipDirection::Top))
                                    .into()
                            });

                        let pause_trigger = div()
                            .id("transfer-pause-all")
                            .w(px(24.0))
                            .h(px(24.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(RADIUS_STD)
                            .text_color(rgb(t.text_muted))
                            .when(has_active, |el| {
                                el.cursor_pointer()
                                    .hover(|s| s.bg(surface_bg(t.bg_hover, cx)))
                                    .on_click(cx.listener(move |_this, _ev, _w, cx| {
                                        store_pause.update(cx, |s, cx| {
                                            s.pause_all();
                                            cx.notify();
                                        });
                                    }))
                            })
                            .when(!has_active, |el| el.opacity(0.4).cursor_default())
                            .child(
                                AppIcon::Pause
                                    .size(px(13.0))
                                    .text_color(rgb(t.text_secondary)),
                            );
                        let pause_btn =
                            pause_trigger.tooltip(move |_, cx| {
                                cx.new(|_| Tooltip::new(pause_label.clone()).direction(TooltipDirection::Top))
                                    .into()
                            });

                        let collapse_trigger = div()
                            .id("transfer-collapse")
                            .w(px(24.0))
                            .h(px(24.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(RADIUS_STD)
                            .text_color(rgb(t.text_muted))
                            .cursor_pointer()
                            .hover(|s| s.bg(surface_bg(t.bg_hover, cx)))
                            .on_click(cx.listener(|this, _ev, _w, cx| {
                                this.close(cx);
                            }))
                            .child(
                                AppIcon::ChevronDown
                                    .size(px(13.0))
                                    .text_color(rgb(t.text_secondary)),
                            );
                        let collapse_btn = collapse_trigger.tooltip(
                            move |_, cx| {
                                cx.new(|_| Tooltip::new(collapse_label.clone()).direction(TooltipDirection::Top))
                                    .into()
                            },
                        );

                        h_flex()
                            .gap(px(2.0))
                            .child(clear_btn)
                            .child(pause_btn)
                            .child(collapse_btn)
                    }),
            )
            // List
            .child(
                div()
                    .id("transfer-list")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap(SPACE_SM)
                    .p(SPACE_MD)
                    .overflow_y_scroll()
                    .children(items),
            )
            .into_any_element()
    }

    /// Renders a single transfer row (mirrors `.transfer-item`).
    fn render_item(
        &self,
        task: &TransferTask,
        t: &ThemeColors,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let store = self.store.clone();
        let id = task.id.clone();
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);

        // Direction icon + accent color (upload = accent, download = green).
        let (icon_path, accent) = match task.direction {
            TransferDirection::Upload => (AppIcon::Upload, rgb(t.accent)),
            TransferDirection::Download => (AppIcon::Download, rgb(t.success)),
        };

        let progress = task.progress();
        let is_complete = task.status == TransferStatus::Complete;
        let is_error = task.status == TransferStatus::Error;
        let is_paused = task.status == TransferStatus::Paused;

        let has_local_path = !task.local_path.is_empty();
        let local_path_exists = has_local_path && std::path::Path::new(&task.local_path).exists();
        // File can be opened if it's completed (or upload) and exists locally.
        let can_open_file = local_path_exists && (is_complete || task.direction == TransferDirection::Upload);

        // Build rich tooltip containing full local and remote paths.
        let local_label = i18n!(cx, "transfers.local_path");
        let remote_label = i18n!(cx, "transfers.remote_path");
        let open_hint = if can_open_file {
            format!("\n({})", i18n!(cx, "transfers.open_file"))
        } else {
            String::new()
        };
        let tooltip_text = format!(
            "{}\n{}: {}\n{}: {}{}",
            task.name,
            local_label,
            task.local_path,
            remote_label,
            task.remote_path,
            open_hint,
        );

        // Interactive file name element
        let base_name = div()
            .id(ElementId::Name(format!("transfer-open-file-{}", id).into()))
            .text_size(ui_text_md(cx))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(t.text_primary))
            .child(task.name.clone());

        let name_el = if can_open_file {
            let local_path_for_click = task.local_path.clone();
            base_name
                .cursor_pointer()
                .hover(|s| s.text_color(rgb(t.accent)).underline())
                .on_click(cx.listener(move |_this, _ev, _w, _cx| {
                    let file_url = format!("file://{}", local_path_for_click);
                    velowork_core::process::open_url(&file_url);
                }))
        } else {
            base_name
        };

        let name_with_tip = name_el.tooltip(move |_, cx| {
            cx.new(|_| Tooltip::new(tooltip_text.clone()).direction(TooltipDirection::Top))
                .into()
        });

        // "Open Containing Folder" button if local folder exists/is accessible
        let open_folder_btn = if has_local_path {
            let local_path_for_folder = task.local_path.clone();
            Some(self.transfer_ctrl(
                cx,
                t,
                "transfer-open-folder",
                AppIcon::FolderOpen,
                i18n!(cx, "transfers.open_folder"),
                move |_this, _ev, _w, _cx| {
                    let path = std::path::Path::new(&local_path_for_folder);
                    let folder = if path.is_dir() {
                        path
                    } else if let Some(parent) = path.parent() {
                        parent
                    } else {
                        path
                    };
                    velowork_core::process::open_in_file_manager(folder);
                },
            ))
        } else {
            None
        };

        // Per-row controls: pause/resume + cancel, or remove when finished.
        let mut controls = Vec::new();
        if let Some(folder_btn) = open_folder_btn {
            if is_complete || task.direction == TransferDirection::Upload {
                controls.push(folder_btn);
            }
        }

        if is_complete || is_error {
            let store = store.clone();
            let id = id.clone();
            controls.push(self.transfer_ctrl(
                cx,
                t,
                "transfer-remove",
                AppIcon::Trash,
                i18n!(cx, "transfers.remove"),
                move |_this, _ev, _w, cx| {
                    store.update(cx, |s, cx| {
                        s.remove(&id);
                        cx.notify();
                    });
                },
            ));
        } else {
            let (pause_icon, pause_label) = if is_paused {
                (AppIcon::Play, i18n!(cx, "transfers.resume"))
            } else {
                (AppIcon::Pause, i18n!(cx, "transfers.pause"))
            };
            let new_status = if is_paused {
                TransferStatus::Active
            } else {
                TransferStatus::Paused
            };
            let store_p = store.clone();
            let id_p = id.clone();
            let store_c = store.clone();
            let id_c = id.clone();
            controls.push(self.transfer_ctrl(
                cx,
                t,
                "transfer-pause",
                pause_icon,
                pause_label,
                move |_this, _ev, _w, cx| {
                    store_p.update(cx, |s, cx| {
                        s.set_status(&id_p, new_status, None);
                        cx.notify();
                    });
                },
            ));
            controls.push(self.transfer_ctrl(
                cx,
                t,
                "transfer-cancel",
                AppIcon::Close,
                i18n!(cx, "common.action.cancel"),
                move |_this, _ev, _w, cx| {
                    store_c.update(cx, |s, cx| {
                        s.remove(&id_c);
                        cx.notify();
                    });
                },
            ));
        }

        let status_label = if is_complete {
            div()
                .text_size(ui_text_md(cx))
                .text_color(rgb(t.success))
                .child(i18n!(cx, "transfers.complete"))
        } else if is_error {
            div()
                .text_size(ui_text_md(cx))
                .text_color(rgb(t.error))
                .child(task.error.clone().unwrap_or_else(|| i18n!(cx, "transfers.error")))
        } else {
            div()
                .text_size(ui_text_md(cx))
                .text_color(rgb(t.text_muted))
                .child(format!("{}%", progress as i32))
        };

        let bottom_right = if is_complete {
            format_bytes(task.total_bytes.max(task.transferred_bytes))
        } else {
            format!(
                "{} · {}",
                format_speed(task.speed_bps),
                format_amount(task.transferred_bytes, task.total_bytes),
            )
        };

        div()
            .id(ElementId::Name(format!("transfer-item-{}", id).into()))
            .flex()
            .flex_col()
            .gap(SPACE_MD)
            .p(px(10.0))
            .rounded(px(6.0))
            .bg(surface_bg(t.bg_header, cx))
            .when(is_error, |el| el.border_l_2().border_color(rgb(t.error)))
            .when(is_paused, |el| el.border_l_2().border_color(rgb(t.warning)))
            .child(
                h_flex()
                    .gap(px(10.0))
                    .items_center()
                    .child(
                        div()
                            .w(px(28.0))
                            .h(px(28.0))
                            .rounded(RADIUS_STD)
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(accent)
                            .child(
                                icon_path
                                    .size(ICON_STD)
                                    .text_color(rgb(0xffffff)),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(name_with_tip),
                    )
                    .children(controls),
            )
            .child(
                div()
                    .w_full()
                    .h(px(4.0))
                    .rounded(px(2.0))
                    .bg(p.surface_card)
                    .child(
                        div()
                            .h_full()
                            .w(relative(progress / 100.0))
                            .rounded(px(2.0))
                            .bg(accent),
                    ),
            )
            .child(
                h_flex()
                    .justify_between()
                    .text_size(ui_text_md(cx))
                    .text_color(rgb(t.text_muted))
                    .child(status_label)
                    .child(div().child(bottom_right)),
            )
            .into_any_element()
    }

    /// Small square control button used inside a transfer row.
    ///
    /// `icon` is an SVG *asset path* (e.g. `AppIcon::Pause`). The SVG uses
    /// `currentColor`, so `text_color` tints it — `svg().path()` loads the asset
    /// and paints it, keeping the icon visible by default while the hover
    /// background still animates on the wrapping `div`.
    fn transfer_ctrl(
        &self,
        cx: &mut Context<Self>,
        t: &ThemeColors,
        id: &'static str,
        icon: AppIcon,
        tooltip: String,
        on_click: impl Fn(&mut TransferPopup, &ClickEvent, &mut Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let trigger = div()
            .id(id)
            .w(px(20.0))
            .h(px(20.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(RADIUS_MD)
            .cursor_pointer()
            .hover(|s| s.bg(surface_bg(t.bg_hover, cx)))
            .on_click(cx.listener(on_click))
            .child(
                icon
                    .size(px(12.0))
                    .text_color(rgb(t.text_secondary)),
            );
        trigger
            .tooltip(move |_, cx| {
                cx.new(|_| Tooltip::new(tooltip.clone()).direction(TooltipDirection::Top))
                    .into()
            })
            .into_any_element()
    }
}
