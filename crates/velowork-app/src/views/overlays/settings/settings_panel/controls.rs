use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;
use crate::settings::{settings_entity, SettingsState};
use crate::terminal::session_backend::SessionBackend;
use crate::terminal::shell_config::ShellType;
use crate::theme::theme;
use velowork_ui::select::Select;
use velowork_ui::input::{InputEvent, InputState};
use gpui::*;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::h_flex;
use velowork_ui::slider::{Slider, SliderEvent, SliderState, SliderValue};
use velowork_i18n::i18n;
use crate::ui::tokens::{
    SELECT_MAX_WIDTH_ADAPTIVE, SELECT_MIN_WIDTH_ADAPTIVE, SELECT_WIDTH_MD, SELECT_WIDTH_SM,
    SPACE_2XL, SPACE_MD, ui_text_md,
};

use super::components::*;
use super::SettingsPanel;

impl SettingsPanel {
    pub(super) fn get_or_create_stepper_input(
        &mut self,
        id: &str,
        val_str: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = self.stepper_inputs.get(id) {
            if input.read(cx).text().is_empty() {
                let val_str = val_str.to_string();
                input.update(cx, |s, cx| s.set_value(&val_str, cx));
            }
            input.clone()
        } else {
            let val_clone = val_str.to_string();
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .default_value(&val_clone)
            });
            self.stepper_inputs.insert(id.to_string(), input.clone());
            input
        }
    }

    // GPUI render helper: params are render inputs (value, bounds, callbacks).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_number_stepper(
        &mut self,
        id: &str,
        label: &str,
        value: f32,
        format: &str,
        min: f32,
        max: f32,
        step: f32,
        width: f32,
        has_border: bool,
        update_fn: impl Fn(&mut SettingsState, f32, &mut Context<SettingsState>) + 'static + Clone,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let dec_fn = update_fn.clone();
        let inc_fn = update_fn.clone();
        let commit_fn = update_fn;

        let format_str = format.to_string();
        let format_val = move |val: f32| -> String {
            let clamped = val.clamp(min, max);
            let val_num = if clamped.fract() == 0.0 {
                format!("{:.0}", clamped)
            } else {
                format!("{:.1}", clamped)
            };
            format_str.replace("{}", &val_num)
        };

        let parse_val = move |text: &str| -> Option<f32> {
            let cleaned = text.trim_end_matches('%').trim_end_matches("px").trim();
            cleaned.parse::<f32>().ok()
        };

        let current_val = value.clamp(min, max);
        let val_display = format_val(current_val);
        let input_entity = self.get_or_create_stepper_input(id, &val_display, window, cx);

        if !input_entity.read(cx).is_focused() {
            let cur_text = input_entity.read(cx).text().to_string();
            if cur_text.is_empty() {
                input_entity.update(cx, |s, cx| s.set_value(&val_display, cx));
            }
        }

        // 仅在首次创建/遇到此步进器时绑定一次防抖 + 失焦/回车提交逻辑，杜绝 render 递归雪崩
        if self.bound_stepper_inputs.insert(id.to_string()) {
            let input_sub = input_entity.clone();
            let commit_sub = commit_fn;
            let format_val_sub = format_val.clone();
            let parse_val_sub = parse_val;

            let last_change_time = Rc::new(Cell::new(Instant::now()));
            let is_timer_running = Rc::new(Cell::new(false));
            let time_clone = last_change_time.clone();
            let timer_clone = is_timer_running.clone();
            let input_weak = input_entity.downgrade();

            cx.subscribe(&input_entity, move |_this, _entity, event: &InputEvent, cx| {
                match event {
                    InputEvent::Change => {
                        time_clone.set(Instant::now());
                        if !timer_clone.get() {
                            timer_clone.set(true);
                            let last_time = time_clone.clone();
                            let is_running = timer_clone.clone();
                            let input_weak = input_weak.clone();
                            let commit_sub = commit_sub.clone();
                            let format_val_sub = format_val_sub.clone();

                            cx.spawn(async move |_this, cx| {
                                loop {
                                    let elapsed = last_time.get().elapsed();
                                    if elapsed < std::time::Duration::from_millis(300) {
                                        let remain = std::time::Duration::from_millis(300) - elapsed;
                                        cx.background_executor().timer(remain).await;
                                    }
                                    if !is_running.get() {
                                        break;
                                    }
                                    if last_time.get().elapsed() >= std::time::Duration::from_millis(300) {
                                        is_running.set(false);
                                        let _ = cx.update(|cx| {
                                            if let Some(input) = input_weak.upgrade() {
                                                let text = input.read(cx).text().to_string();
                                                let cleaned = text.trim_end_matches('%').trim_end_matches("px").trim();
                                                if let Ok(parsed) = cleaned.parse::<f32>() {
                                                    let final_val = parsed.clamp(min, max);
                                                    let formatted = format_val_sub(final_val);
                                                    input.update(cx, |s, cx| s.set_value(&formatted, cx));
                                                    settings_entity(cx).update(cx, |state, cx| {
                                                        commit_sub(state, final_val, cx);
                                                    });
                                                }
                                            }
                                        });
                                        break;
                                    }
                                }
                            })
                            .detach();
                        }
                    }
                    InputEvent::PressEnter | InputEvent::Blur => {
                        timer_clone.set(false);
                        let text = input_sub.read(cx).text().to_string();
                        let final_val = parse_val_sub(&text).unwrap_or(current_val).clamp(min, max);
                        let formatted = format_val_sub(final_val);
                        input_sub.update(cx, |s, cx| s.set_value(&formatted, cx));
                        let commit_sub = commit_sub.clone();
                        settings_entity(cx).update(cx, |state, cx| {
                            commit_sub(state, final_val, cx);
                        });
                    }
                    _ => {}
                }
            })
            .detach();
        }

        let input_dec = input_entity.clone();
        let input_inc = input_entity.clone();
        let format_dec = format_val.clone();
        let format_inc = format_val;

        settings_row(id.to_string(), label, &t, cx, has_border).child(
            velowork_ui::number_stepper(id.to_string(), input_entity, &t)
                .width(px(width))
                .min(min)
                .max(max)
                .step(step)
                .on_dec(cx.listener(move |_, _, _window, cx| {
                    let dec_fn = dec_fn.clone();
                    let new_val = (value - step).clamp(min, max);
                    let display_str = format_dec(new_val);
                    input_dec.update(cx, |s, cx| s.set_value(&display_str, cx));
                    settings_entity(cx).update(cx, |state, cx| {
                        dec_fn(state, new_val, cx);
                    });
                }))
                .on_inc(cx.listener(move |_, _, _window, cx| {
                    let inc_fn = inc_fn.clone();
                    let new_val = (value + step).clamp(min, max);
                    let display_str = format_inc(new_val);
                    input_inc.update(cx, |s, cx| s.set_value(&display_str, cx));
                    settings_entity(cx).update(cx, |state, cx| {
                        inc_fn(state, new_val, cx);
                    });
                })),
        )
    }

    // GPUI render helper: params are render inputs (value, bounds, callbacks).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_integer_stepper(
        &mut self,
        id: &str,
        label: &str,
        value: u32,
        min: u32,
        max: u32,
        step: u32,
        width: f32,
        has_border: bool,
        update_fn: impl Fn(&mut SettingsState, u32, &mut Context<SettingsState>) + 'static + Clone,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let dec_fn = update_fn.clone();
        let inc_fn = update_fn.clone();
        let commit_fn = update_fn;

        let parse_val = move |text: &str| -> Option<u32> {
            let cleaned = text.trim();
            cleaned.parse::<u32>().ok()
        };

        let current_val = value.clamp(min, max);
        let val_display = current_val.to_string();
        let input_entity = self.get_or_create_stepper_input(id, &val_display, window, cx);

        if !input_entity.read(cx).is_focused() {
            let cur_text = input_entity.read(cx).text().to_string();
            if cur_text.is_empty() {
                input_entity.update(cx, |s, cx| s.set_value(&val_display, cx));
            }
        }

        // 仅在首次创建/遇到此步进器时绑定一次防抖 + 失焦/回车提交逻辑，杜绝 render 递归雪崩
        if self.bound_stepper_inputs.insert(id.to_string()) {
            let input_sub = input_entity.clone();
            let commit_sub = commit_fn;

            let last_change_time = Rc::new(Cell::new(Instant::now()));
            let is_timer_running = Rc::new(Cell::new(false));
            let time_clone = last_change_time.clone();
            let timer_clone = is_timer_running.clone();
            let input_weak = input_entity.downgrade();

            cx.subscribe(&input_entity, move |_this, _entity, event: &InputEvent, cx| {
                match event {
                    InputEvent::Change => {
                        time_clone.set(Instant::now());
                        if !timer_clone.get() {
                            timer_clone.set(true);
                            let last_time = time_clone.clone();
                            let is_running = timer_clone.clone();
                            let input_weak = input_weak.clone();
                            let commit_sub = commit_sub.clone();

                            cx.spawn(async move |_this, cx| {
                                loop {
                                    let elapsed = last_time.get().elapsed();
                                    if elapsed < std::time::Duration::from_millis(300) {
                                        let remain = std::time::Duration::from_millis(300) - elapsed;
                                        cx.background_executor().timer(remain).await;
                                    }
                                    if !is_running.get() {
                                        break;
                                    }
                                    if last_time.get().elapsed() >= std::time::Duration::from_millis(300) {
                                        is_running.set(false);
                                        let _ = cx.update(|cx| {
                                            if let Some(input) = input_weak.upgrade() {
                                                let text = input.read(cx).text().to_string();
                                                let cleaned = text.trim();
                                                if let Ok(parsed) = cleaned.parse::<u32>() {
                                                    let final_val = parsed.clamp(min, max);
                                                    let formatted = final_val.to_string();
                                                    input.update(cx, |s, cx| s.set_value(&formatted, cx));
                                                    settings_entity(cx).update(cx, |state, cx| {
                                                        commit_sub(state, final_val, cx);
                                                    });
                                                }
                                            }
                                        });
                                        break;
                                    }
                                }
                            })
                            .detach();
                        }
                    }
                    InputEvent::PressEnter | InputEvent::Blur => {
                        timer_clone.set(false);
                        let text = input_sub.read(cx).text().to_string();
                        let final_val = parse_val(&text).unwrap_or(current_val).clamp(min, max);
                        let formatted = final_val.to_string();
                        input_sub.update(cx, |s, cx| s.set_value(&formatted, cx));
                        let commit_sub = commit_sub.clone();
                        settings_entity(cx).update(cx, |state, cx| {
                            commit_sub(state, final_val, cx);
                        });
                    }
                    _ => {}
                }
            })
            .detach();
        }

        let input_dec = input_entity.clone();
        let input_inc = input_entity.clone();

        settings_row(id.to_string(), label, &t, cx, has_border).child(
            velowork_ui::number_stepper(id.to_string(), input_entity, &t)
                .width(px(width))
                .min(min as f32)
                .max(max as f32)
                .step(step as f32)
                .on_dec(cx.listener(move |_, _, _window, cx| {
                    let dec_fn = dec_fn.clone();
                    let new_val = value.saturating_sub(step).clamp(min, max);
                    input_dec.update(cx, |s, cx| s.set_value(&new_val.to_string(), cx));
                    settings_entity(cx).update(cx, |state, cx| {
                        dec_fn(state, new_val, cx);
                    });
                }))
                .on_inc(cx.listener(move |_, _, _window, cx| {
                    let inc_fn = inc_fn.clone();
                    let new_val = (value + step).clamp(min, max);
                    input_inc.update(cx, |s, cx| s.set_value(&new_val.to_string(), cx));
                    settings_entity(cx).update(cx, |state, cx| {
                        inc_fn(state, new_val, cx);
                    });
                })),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_integer_stepper_with_desc(
        &mut self,
        id: &str,
        label: impl Into<SharedString>,
        desc: impl Into<SharedString>,
        value: u32,
        min: u32,
        max: u32,
        step: u32,
        width: f32,
        has_border: bool,
        update_fn: impl Fn(&mut SettingsState, u32, &mut Context<SettingsState>) + 'static + Clone,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let label_ss = label.into();
        let desc_ss = desc.into();
        let dec_fn = update_fn.clone();
        let inc_fn = update_fn.clone();
        let commit_fn = update_fn;

        let parse_val = move |text: &str| -> Option<u32> {
            let cleaned = text.trim();
            cleaned.parse::<u32>().ok()
        };

        let current_val = value.clamp(min, max);
        let val_display = current_val.to_string();
        let input_entity = self.get_or_create_stepper_input(id, &val_display, window, cx);

        if !input_entity.read(cx).is_focused() {
            let cur_text = input_entity.read(cx).text().to_string();
            if cur_text.is_empty() {
                input_entity.update(cx, |s, cx| s.set_value(&val_display, cx));
            }
        }

        // 仅在首次创建/遇到此步进器时绑定一次防抖 + 失焦/回车提交逻辑，杜绝 render 递归雪崩
        if self.bound_stepper_inputs.insert(id.to_string()) {
            let input_sub = input_entity.clone();
            let commit_sub = commit_fn;

            let last_change_time = Rc::new(Cell::new(Instant::now()));
            let is_timer_running = Rc::new(Cell::new(false));
            let time_clone = last_change_time.clone();
            let timer_clone = is_timer_running.clone();
            let input_weak = input_entity.downgrade();

            cx.subscribe(&input_entity, move |_this, _entity, event: &InputEvent, cx| {
                match event {
                    InputEvent::Change => {
                        time_clone.set(Instant::now());
                        if !timer_clone.get() {
                            timer_clone.set(true);
                            let last_time = time_clone.clone();
                            let is_running = timer_clone.clone();
                            let input_weak = input_weak.clone();
                            let commit_sub = commit_sub.clone();

                            cx.spawn(async move |_this, cx| {
                                loop {
                                    let elapsed = last_time.get().elapsed();
                                    if elapsed < std::time::Duration::from_millis(300) {
                                        let remain = std::time::Duration::from_millis(300) - elapsed;
                                        cx.background_executor().timer(remain).await;
                                    }
                                    if !is_running.get() {
                                        break;
                                    }
                                    if last_time.get().elapsed() >= std::time::Duration::from_millis(300) {
                                        is_running.set(false);
                                        let _ = cx.update(|cx| {
                                            if let Some(input) = input_weak.upgrade() {
                                                let text = input.read(cx).text().to_string();
                                                let cleaned = text.trim();
                                                if let Ok(parsed) = cleaned.parse::<u32>() {
                                                    let final_val = parsed.clamp(min, max);
                                                    let formatted = final_val.to_string();
                                                    input.update(cx, |s, cx| s.set_value(&formatted, cx));
                                                    settings_entity(cx).update(cx, |state, cx| {
                                                        commit_sub(state, final_val, cx);
                                                    });
                                                }
                                            }
                                        });
                                        break;
                                    }
                                }
                            })
                            .detach();
                        }
                    }
                    InputEvent::PressEnter | InputEvent::Blur => {
                        timer_clone.set(false);
                        let text = input_sub.read(cx).text().to_string();
                        let final_val = parse_val(&text).unwrap_or(current_val).clamp(min, max);
                        let formatted = final_val.to_string();
                        input_sub.update(cx, |s, cx| s.set_value(&formatted, cx));
                        let commit_sub = commit_sub.clone();
                        settings_entity(cx).update(cx, |state, cx| {
                            commit_sub(state, final_val, cx);
                        });
                    }
                    _ => {}
                }
            })
            .detach();
        }

        let input_dec = input_entity.clone();
        let input_inc = input_entity.clone();

        settings_row_with_desc(id.to_string(), &label_ss, &desc_ss, &t, cx, has_border).child(
            velowork_ui::number_stepper(id.to_string(), input_entity, &t)
                .width(px(width))
                .min(min as f32)
                .max(max as f32)
                .step(step as f32)
                .on_dec(cx.listener(move |_, _, _window, cx| {
                    let dec_fn = dec_fn.clone();
                    let new_val = value.saturating_sub(step).clamp(min, max);
                    input_dec.update(cx, |s, cx| s.set_value(&new_val.to_string(), cx));
                    settings_entity(cx).update(cx, |state, cx| {
                        dec_fn(state, new_val, cx);
                    });
                }))
                .on_inc(cx.listener(move |_, _, _window, cx| {
                    let inc_fn = inc_fn.clone();
                    let new_val = (value + step).clamp(min, max);
                    input_inc.update(cx, |s, cx| s.set_value(&new_val.to_string(), cx));
                    settings_entity(cx).update(cx, |state, cx| {
                        inc_fn(state, new_val, cx);
                    });
                })),
        )
    }

    /// 稳定获取或懒创建指定 button 的持久 FocusHandle（生命周期与面板绑定，重绘时不失焦）
    pub(super) fn get_or_create_button_focus_handle(
        &self,
        id: &str,
        cx: &App,
    ) -> FocusHandle {
        self.button_focus_handles
            .borrow_mut()
            .entry(id.to_string())
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }

    /// 稳定获取或懒创建指定 toggle 的持久 FocusHandle（生命周期与面板绑定，重绘时不失焦）
    pub(super) fn get_or_create_toggle_focus_handle(
        &self,
        id: &str,
        cx: &App,
    ) -> FocusHandle {
        self.toggle_focus_handles
            .borrow_mut()
            .entry(id.to_string())
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }

    /// 稳定获取或懒创建指定 radio group 的持久 FocusHandle（生命周期与面板绑定，重绘时不失焦）
    pub(super) fn get_or_create_radio_focus_handle(
        &self,
        id: &str,
        cx: &App,
    ) -> FocusHandle {
        self.radio_focus_handles
            .borrow_mut()
            .entry(id.to_string())
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }

    pub(super) fn render_toggle(
        &mut self,
        id: &str,
        label: &str,
        enabled: bool,
        has_border: bool,
        update_fn: impl Fn(&mut SettingsState, bool, &mut Context<SettingsState>) + 'static + Clone,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let focus_handle = self.get_or_create_toggle_focus_handle(id, cx);

        settings_row(id.to_string(), label, &t, cx, has_border).child(
            velowork_ui::Switch::new(format!("{}-toggle", id))
                .focus(&focus_handle)
                .checked(enabled)
                .on_click(cx.listener(move |_, checked: &bool, _, cx| {
                    let update_fn = update_fn.clone();
                    let new_val = *checked;
                    settings_entity(cx).update(cx, |state, cx| {
                        update_fn(state, new_val, cx);
                    });
                })),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_toggle_with_desc(
        &mut self,
        id: &str,
        label: &str,
        desc: &str,
        enabled: bool,
        has_border: bool,
        update_fn: impl Fn(&mut SettingsState, bool, &mut Context<SettingsState>) + 'static + Clone,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let focus_handle = self.get_or_create_toggle_focus_handle(id, cx);

        settings_row_with_desc(id.to_string(), label, desc, &t, cx, has_border).child(
            velowork_ui::Switch::new(format!("{}-toggle", id))
                .focus(&focus_handle)
                .checked(enabled)
                .on_click(cx.listener(move |_, checked: &bool, _, cx| {
                    let update_fn = update_fn.clone();
                    let new_val = *checked;
                    settings_entity(cx).update(cx, |state, cx| {
                        update_fn(state, new_val, cx);
                    });
                })),
        )
    }

    pub(super) fn render_font_dropdown_row(&mut self, _current: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let font_label = i18n!(cx, "settings.font.terminal_family");
        settings_row("font-family".to_string(), &font_label, &t, cx, true)
            .child(div().min_w(SELECT_MIN_WIDTH_ADAPTIVE).max_w(SELECT_MAX_WIDTH_ADAPTIVE).child(Select::new(&self.font_select)))
    }

    pub(super) fn render_ui_font_dropdown_row(&mut self, _current: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, "settings.font.ui_family");
        settings_row("ui-font-family".to_string(), &label, &t, cx, true)
            .child(div().min_w(SELECT_MIN_WIDTH_ADAPTIVE).max_w(SELECT_MAX_WIDTH_ADAPTIVE).child(Select::new(&self.ui_font_select)))
    }

    pub(super) fn render_font_weight_dropdown_row(&mut self, _current: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, "settings.font.weight");
        settings_row("font-weight".to_string(), &label, &t, cx, true)
            .child(div().w(SELECT_WIDTH_SM).child(Select::new(&self.font_weight_select)))
    }

    pub(super) fn render_text_antialiasing_dropdown_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let label = i18n!(cx, "settings.font.text_antialiasing");
        settings_row("text-antialiasing".to_string(), &label, &t, cx, true)
            .child(div().w(SELECT_WIDTH_MD).child(Select::new(&self.text_antialiasing_select)))
    }

    pub(super) fn render_shell_dropdown_row(&mut self, _current_shell: &ShellType, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let shell_label = i18n!(cx, "settings.default_shell");
        settings_row("default-shell".to_string(), &shell_label, &t, cx, true)
            .child(div().w(SELECT_WIDTH_MD).child(Select::new(&self.shell_select)))
    }

    pub(super) fn render_session_backend_dropdown_row(&mut self, _current_backend: &SessionBackend, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let session_label = i18n!(cx, "settings.session_backend");
        let session_desc = i18n!(cx, "common.requires_restart");
        settings_row_with_desc("session-backend".to_string(), &session_label, &session_desc, &t, cx, true)
            .child(div().w(SELECT_WIDTH_MD).child(Select::new(&self.session_backend_select)))
    }

    /// 通用滑块渲染：在 settings_row 中展示 Slider + 当前数值（百分比）文本。
    ///
    /// `value` 为滑块当前值（如百分比），范围由 `min`/`max`/`step` 决定。
    /// `update_fn` 接收原始滑块值（已按 step 对齐），由调用方负责映射到实际设置。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_slider(
        &mut self,
        id: &str,
        label: &str,
        value: f32,
        min: f32,
        max: f32,
        step: f32,
        has_border: bool,
        format_fn: impl Fn(f32) -> String + 'static + Clone,
        update_fn: impl Fn(&mut SettingsState, f32, &mut Context<SettingsState>) + 'static + Clone,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);

        // 懒初始化并缓存 SliderState，避免每次渲染重建导致拖动状态丢失；
        // 订阅只在创建时注册一次，避免重复订阅。
        let slider = match self.bg_opacity_slider.clone() {
            Some(entity) => {
                if !entity.read(cx).is_dragging() {
                    let cur = entity.read(cx).get_value().end();
                    if (cur - value).abs() > 0.001 {
                        entity.update(cx, |s, cx| s.set_value(value, cx));
                    }
                }
                entity
            }
            None => {
                let entity = cx.new(|_cx| {
                    SliderState::new(_cx)
                        .min(min)
                        .max(max)
                        .step(step)
                        .value(value)
                });

                let update_fn_sub = update_fn.clone();
                cx.subscribe(&entity, move |_this, _entity, ev: &SliderEvent, cx| {
                    match ev {
                        SliderEvent::Change(SliderValue::Single(v)) | SliderEvent::Release(SliderValue::Single(v)) => {
                            let val = *v;
                            let update_fn_sub = update_fn_sub.clone();
                            settings_entity(cx).update(cx, |state, cx| {
                                update_fn_sub(state, val, cx);
                            });
                        }
                        _ => {}
                    }
                    cx.notify();
                })
                .detach();

                self.bg_opacity_slider = Some(entity.clone());
                entity
            }
        };

        let current = slider.read(cx).get_value().end();
        let val_display = format_fn(current);

        settings_row(id.to_string(), label, &t, cx, has_border).child(
            h_flex()
                .gap(SPACE_MD)
                .items_center()
                .child(div().w(SPACE_2XL).child(Slider::new(&slider).horizontal().disabled(false)))
                .child(
                    div()
                        .w(px(44.0))
                        .text_align(TextAlign::Right)
                        .text_size(ui_text_md(cx))
                        .text_color(p.text_primary)
                        .child(val_display),
                ),
        )
    }
}
