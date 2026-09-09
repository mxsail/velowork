//! Lock screen overlay — blocks all content until the master password is entered.

use gpui::prelude::*;
use gpui::*;
use std::time::Duration;
use velowork_i18n::i18n;
use velowork_ui::brand_logo;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::icon::AppIcon;
use velowork_ui::icon_button::icon_button_sized_px;
use velowork_ui::input::focus_ring_shadows;
use velowork_ui::simple_input::{SimpleInput, SimpleInputState};
use velowork_ui::theme::{theme, with_alpha};
use velowork_ui::tokens::*;
use velowork_ui::tooltip::Tooltip;
use velowork_ui::{h_flex, v_flex};
use velowork_workspace::security::current_security_service;

/// Events emitted by the lock screen.
#[derive(Clone)]
pub enum LockScreenEvent {
    /// User entered the correct password.
    Unlocked,
}

fn calculate_cooldown_secs(failed_count: u32) -> u64 {
    match failed_count {
        0..=2 => 0,
        3 => 10,
        4 => 30,
        5 => 60,
        6 => 120,
        7 => 300,
        8 => 600,
        9 => 1800,
        10 => 3600,
        _ => 7200, // Capped at 2 hours (7200 seconds)
    }
}

fn format_remaining_duration(secs: u64, is_zh: bool) -> String {
    if secs >= 3600 {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        if is_zh {
            format!("{} 小时 {} 分", hours, mins)
        } else {
            format!("{}h {}m", hours, mins)
        }
    } else if secs >= 60 {
        let mins = secs / 60;
        let s = secs % 60;
        if is_zh {
            format!("{} 分 {} 秒", mins, s)
        } else {
            format!("{}m {}s", mins, s)
        }
    } else {
        if is_zh {
            format!("{} 秒", secs)
        } else {
            format!("{}s", secs)
        }
    }
}

/// Full-screen lock screen that blocks all content until the master password is entered.
pub struct LockScreen {
    password_input: Entity<SimpleInputState>,
    error_msg: Option<String>,
    busy: bool,
    focus_handle: FocusHandle,
    has_focused: bool,
    failed_attempts: u32,
    cooldown_until: Option<std::time::Instant>,
    password_visible: bool,
}

impl EventEmitter<LockScreenEvent> for LockScreen {}

impl LockScreen {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let placeholder = i18n!(cx, "lock_screen.password_placeholder");
        let password_input = cx.new(|cx| {
            SimpleInputState::new(cx)
                .placeholder(placeholder)
                .password()
        });

        // Load rate limiting & cooldown state directly from encrypted SQLite security DB
        let (saved_failed, saved_cooldown_until_unix) = smol::block_on(async {
            smol::unblock(|| match current_security_service() {
                Ok(svc) => svc.get_lock_cooldown_state().unwrap_or((0, 0)),
                Err(_) => (0, 0),
            })
            .await
        });

        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let (cooldown_until, in_cooldown) = if saved_cooldown_until_unix > now_unix {
            let remaining_secs = saved_cooldown_until_unix - now_unix;
            (
                Some(std::time::Instant::now() + std::time::Duration::from_secs(remaining_secs)),
                true,
            )
        } else {
            (None, false)
        };

        let mut error_msg = None;
        if in_cooldown {
            if let Some(until) = cooldown_until {
                let remaining_secs = (until - std::time::Instant::now()).as_secs() + 1;
                let is_zh = velowork_i18n::current_locale(cx) == velowork_i18n::Locale::Zh;
                let dur_str = format_remaining_duration(remaining_secs, is_zh);
                error_msg = Some(i18n!(cx, "lock_screen.cooldown").replace("{}", &dur_str));
            }
        }

        let lock_screen = Self {
            password_input,
            error_msg,
            busy: false,
            focus_handle: cx.focus_handle(),
            has_focused: false,
            failed_attempts: saved_failed,
            cooldown_until,
            password_visible: false,
        };

        if in_cooldown {
            if let Some(until) = cooldown_until {
                lock_screen.start_cooldown_timer(until, cx);
            }
        }

        lock_screen
    }

    fn try_unlock(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if let Some(until) = self.cooldown_until {
            let now = std::time::Instant::now();
            if now < until {
                let secs_remaining = (until - now).as_secs() + 1;
                let is_zh = velowork_i18n::current_locale(cx) == velowork_i18n::Locale::Zh;
                let dur_str = format_remaining_duration(secs_remaining, is_zh);
                let msg = i18n!(cx, "lock_screen.cooldown").replace("{}", &dur_str);
                self.error_msg = Some(msg);
                cx.notify();
                return;
            } else {
                self.cooldown_until = None;
            }
        }

        let password = self.password_input.read(cx).value().to_string();
        if password.is_empty() {
            return;
        }

        self.busy = true;
        self.error_msg = None;
        cx.notify();

        let password = password.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let (ok, failed_attempts, cooldown_secs, _until_unix) =
                smol::unblock(move || match current_security_service() {
                    Ok(mut svc) => {
                        if svc.unlock(&password).is_ok() {
                            let _ = svc.reset_lock_cooldown_state();
                            (true, 0, 0, 0)
                        } else {
                            let (cur_failed, _) = svc.get_lock_cooldown_state().unwrap_or((0, 0));
                            let new_failed = cur_failed + 1;
                            let cooldown_secs = calculate_cooldown_secs(new_failed);
                            let (_, until_unix) = svc
                                .record_failed_unlock_attempt(cooldown_secs)
                                .unwrap_or((new_failed, 0));
                            (false, new_failed, cooldown_secs, until_unix)
                        }
                    }
                    Err(_) => (false, 1, 0, 0),
                })
                .await;

            this.update(cx, |this, cx| {
                this.busy = false;
                if ok {
                    this.failed_attempts = 0;
                    this.cooldown_until = None;
                    cx.emit(LockScreenEvent::Unlocked);
                } else {
                    this.failed_attempts = failed_attempts;
                    if cooldown_secs > 0 {
                        let until = std::time::Instant::now()
                            + std::time::Duration::from_secs(cooldown_secs);
                        this.cooldown_until = Some(until);

                        let is_zh = velowork_i18n::current_locale(cx) == velowork_i18n::Locale::Zh;
                        let dur_str = format_remaining_duration(cooldown_secs, is_zh);
                        let msg = i18n!(cx, "lock_screen.cooldown").replace("{}", &dur_str);
                        this.error_msg = Some(msg);
                        this.start_cooldown_timer(until, cx);
                    } else {
                        this.error_msg = Some(i18n!(cx, "lock_screen.wrong_password"));
                    }

                    this.password_input.update(cx, |s, cx| {
                        s.set_value("", cx);
                    });
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn start_cooldown_timer(&self, until: std::time::Instant, cx: &mut Context<Self>) {
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            while std::time::Instant::now() < until {
                smol::Timer::after(std::time::Duration::from_secs(1)).await;
                let updated = this.update(cx, |this, cx| {
                    if let Some(c_until) = this.cooldown_until {
                        let now = std::time::Instant::now();
                        if now < c_until {
                            let secs_remaining = (c_until - now).as_secs() + 1;
                            let is_zh =
                                velowork_i18n::current_locale(cx) == velowork_i18n::Locale::Zh;
                            let dur_str = format_remaining_duration(secs_remaining, is_zh);
                            let msg = i18n!(cx, "lock_screen.cooldown").replace("{}", &dur_str);
                            this.error_msg = Some(msg);
                            cx.notify();
                        } else {
                            this.cooldown_until = None;
                            this.error_msg = None;
                            cx.notify();
                        }
                    }
                });
                if updated.is_err() {
                    break;
                }
            }
        })
        .detach();
    }
}

impl Render for LockScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        // Auto-focus input box when lock screen appears
        if !self.has_focused {
            self.has_focused = true;
            self.password_input.update(cx, |s, cx| s.focus(window, cx));
        }
        let is_busy = self.busy;
        let pw_visible = self.password_visible;

        let settings = &crate::settings::settings_entity(cx).read(cx).settings;
        let is_custom_titlebar =
            settings.titlebar_style == velowork_workspace::settings::TitlebarStyle::Custom;
        let window_corner_radius = settings.window_corner_radius;
        let is_maximized = window.is_maximized();
        let is_fullscreen = window.is_fullscreen();
        let has_rounded_corners =
            is_custom_titlebar && !is_maximized && !is_fullscreen && window_corner_radius > 0.0;
        let radius = px(window_corner_radius);

        let mut root = div()
            .id("lock-screen")
            .track_focus(&self.focus_handle)
            .size_full()
            .occlude()
            .bg(rgb(t.bg_primary))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, |_, _, _| {})
            .on_mouse_down(MouseButton::Right, |_, _, _| {})
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "enter" && !this.busy && this.cooldown_until.is_none() {
                    this.try_unlock(window, cx);
                }
            }));

        if has_rounded_corners {
            root = root.rounded_bl(radius).rounded_br(radius);
        }

        root.child(
            v_flex()
                .items_center()
                .gap(px(28.0))
                // 1. Brand Logo & Name
                .child(
                    v_flex()
                        .items_center()
                        .gap(SPACE_SM)
                        .child(brand_logo(px(64.0), window, cx))
                        .child(
                            div()
                                .text_size(px(20.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(t.text_primary))
                                .child("Velowork"),
                        ),
                )
                // 3. Locked Status Bar (Icon + Text)
                .child(
                    h_flex()
                        .items_center()
                        .gap(SPACE_XS)
                        .child(
                            AppIcon::Shield
                                .svg()
                                .size(px(13.0))
                                .text_color(with_alpha(t.text_muted, 0.85)),
                        )
                        .child(
                            div()
                                .text_size(px(13.0))
                                .text_color(with_alpha(t.text_muted, 0.85))
                                .child(i18n!(cx, "lock_screen.title")),
                        ),
                )
                // 4. Password Capsule Box [  ••••••••  (👁)  ]
                .child({
                    let is_focused = self.password_input.read(cx).focus_handle(cx).is_focused(window);
                    let p = SemanticPalette::from_theme(&t);
                    let ring = focus_ring_shadows(&t);

                    h_flex()
                        .id("password-capsule")
                        .h(px(40.0))
                        .w(px(260.0))
                        .pl(px(14.0))
                        .pr(px(4.0))
                        .rounded(radius)
                        .bg(if is_focused {
                            p.surface_hover
                        } else {
                            with_alpha(t.bg_secondary, 0.75)
                        })
                        .border_1()
                        .border_color(if is_focused {
                            p.border_active
                        } else {
                            with_alpha(t.text_primary, 0.22)
                        })
                        .when(is_focused, |s| s.shadow(ring))
                        .when(!is_focused, |s| {
                            s.hover(|h| {
                                h.border_color(p.surface_accent.opacity(0.6))
                                    .bg(p.surface_hover)
                            })
                        })
                        .track_focus(&self.password_input.read(cx).focus_handle(cx))
                        .on_mouse_down(MouseButton::Left, {
                            let input = self.password_input.clone();
                            move |_, window, cx| {
                                cx.stop_propagation();
                                input.update(cx, |s, cx| s.focus(window, cx));
                            }
                        })
                        .items_center()
                        .gap(SPACE_XS)
                        .child(
                            div().flex_1().h_full().flex().items_center().child(
                                div().w_full().child(
                                    SimpleInput::new(&self.password_input)
                                        .text_size(ui_text_md(cx))
                                        .borderless(true),
                                ),
                            ),
                        )
                        .child({
                            if is_busy {
                                // Spinning loader while verifying
                                div()
                                    .id("pw-toggle-busy")
                                    .size(px(28.0))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        div()
                                            .size_full()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .with_animation(
                                                "lock-verifying-spin",
                                                Animation::new(Duration::from_millis(1000))
                                                    .repeat(),
                                                move |this, delta| {
                                                    let angle = delta * std::f32::consts::TAU;
                                                    this.child(
                                                        AppIcon::LoaderCircle
                                                            .svg()
                                                            .size(px(14.0))
                                                            .text_color(with_alpha(
                                                                t.text_muted,
                                                                0.7,
                                                            ))
                                                            .with_transformation(
                                                                Transformation::rotate(radians(
                                                                    angle,
                                                                )),
                                                            ),
                                                    )
                                                },
                                            ),
                                    )
                                    .into_any_element()
                            } else {
                                // Eye / EyeOff toggle button
                                let toggle_icon = if pw_visible {
                                    AppIcon::EyeOff
                                } else {
                                    AppIcon::Eye
                                };
                                let tip = if pw_visible {
                                    i18n!(cx, "settings.security.hide_password")
                                } else {
                                    i18n!(cx, "settings.security.show_password")
                                };
                                icon_button_sized_px(
                                    "pw-visibility-toggle",
                                    toggle_icon,
                                    px(28.0),
                                    px(14.0),
                                    &t,
                                )
                                .tooltip(move |_, cx| {
                                    let __tip = tip.clone();
                                    cx.new(|_| Tooltip::new(__tip)).into()
                                })
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.password_visible = !this.password_visible;
                                    let visible = this.password_visible;
                                    this.password_input.update(cx, |st, _cx| {
                                        st.set_password(!visible);
                                    });
                                    cx.notify();
                                }))
                                .into_any_element()
                            }
                        })
                })
                // 5. Error Banner / Help Prompt
                .child(
                    v_flex()
                        .items_center()
                        .gap(SPACE_XS)
                        .when_some(self.error_msg.clone(), |d, err| {
                            d.child(
                                h_flex()
                                    .items_center()
                                    .gap(SPACE_XS)
                                    .text_size(ui_text_sm(cx))
                                    .text_color(rgb(t.error))
                                    .child(
                                        AppIcon::Close
                                            .svg()
                                            .size(px(13.0))
                                            .text_color(rgb(t.error)),
                                    )
                                    .child(err),
                            )
                        })
                        .when(self.error_msg.is_none(), |d| {
                            d.child(
                                div()
                                    .text_size(ui_text_sm(cx))
                                    .text_color(with_alpha(t.text_muted, 0.7))
                                    .child(i18n!(cx, "lock_screen.prompt")),
                            )
                        }),
                ),
        )
    }
}

velowork_ui::impl_focusable!(LockScreen);
