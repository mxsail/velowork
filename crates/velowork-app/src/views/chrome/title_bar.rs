use crate::keybindings::{
    About, CheckForUpdates, Copy, NewWindow, Paste, Quit, ShowCommandPalette,
    ShowKeybindings, ShowProfileManager, ShowSettings, ShowThemeSelector, ToggleLeftDock,
    ToggleRightDock,
};
use crate::settings::settings_entity;
use crate::theme::theme;
use crate::ui::tokens::ui_text;
use crate::views::components::menu_item;
use gpui::prelude::*;
use gpui::*;
use std::time::Duration;
use velowork_i18n::i18n;
use velowork_ui::brand_logo;
use velowork_ui::decorations::{WindowButtonPosition, WindowDecorationConfig};
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::h_flex;
use velowork_ui::icon::AppIcon;
use velowork_ui::tokens::{RADIUS_LG, RADIUS_MD, SPACE_MD, SPACE_SM, SPACE_XS};
use velowork_workspace::settings::TitlebarStyle;

/// Helper to construct `(TitlebarOptions, WindowDecorations)` based on user's `TitlebarStyle` preference.
pub fn window_decorations_and_titlebar(
    style: TitlebarStyle,
    title: impl Into<SharedString>,
) -> (Option<TitlebarOptions>, Option<WindowDecorations>) {
    match style {
        TitlebarStyle::Custom => {
            if cfg!(target_os = "macos") {
                (
                    Some(TitlebarOptions {
                        title: Some(title.into()),
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    Some(WindowDecorations::Server),
                )
            } else if cfg!(target_os = "windows") {
                (
                    Some(TitlebarOptions {
                        title: Some(title.into()),
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    Some(WindowDecorations::Client),
                )
            } else {
                // Linux: no system titlebar, client-side decorations (CSD)
                (None, Some(WindowDecorations::Client))
            }
        }
        TitlebarStyle::Native => (
            Some(TitlebarOptions {
                title: Some(title.into()),
                appears_transparent: false,
                ..Default::default()
            }),
            Some(WindowDecorations::Server),
        ),
    }
}

/// Top-level menu categories for Zed-style menu expansion
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopMenuCategory {
    File,
    Edit,
    View,
    Window,
    Help,
}

impl TopMenuCategory {
    pub fn translation_key(self) -> &'static str {
        match self {
            TopMenuCategory::File => "menu.file",
            TopMenuCategory::Edit => "menu.edit",
            TopMenuCategory::View => "menu.view",
            TopMenuCategory::Window => "menu.window",
            TopMenuCategory::Help => "menu.help",
        }
    }

    pub fn all() -> &'static [TopMenuCategory] {
        &[
            TopMenuCategory::File,
            TopMenuCategory::Edit,
            TopMenuCategory::View,
            TopMenuCategory::Window,
            TopMenuCategory::Help,
        ]
    }
}

/// Title bar with window controls, sidebar toggles, and Zed-style animated hamburger menu
pub struct TitleBar {
    title: SharedString,
    menu_open: bool,
    active_top_menu: TopMenuCategory,
    anim_progress: f32,
    _anim_task: Option<Task<()>>,
    context_menu_open: bool,
    context_menu_pos: Point<Pixels>,
    sidebar_open: bool,
    right_sidebar_open: bool,
    close_handler: Option<std::sync::Arc<dyn Fn(&mut Window, &mut App)>>,
    /// Flag for Linux compositor-driven window move
    #[cfg(target_os = "linux")]
    should_move: bool,
}

impl TitleBar {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            menu_open: false,
            active_top_menu: TopMenuCategory::File,
            anim_progress: 0.0,
            _anim_task: None,
            context_menu_open: false,
            context_menu_pos: Point::default(),
            sidebar_open: true,
            right_sidebar_open: false,
            close_handler: None,
            #[cfg(target_os = "linux")]
            should_move: false,
        }
    }

    pub fn set_close_handler(
        &mut self,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) {
        self.close_handler = Some(std::sync::Arc::new(handler));
    }

    pub fn set_sidebar_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.sidebar_open != open {
            self.sidebar_open = open;
            cx.notify();
        }
    }

    pub fn set_right_sidebar_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.right_sidebar_open != open {
            self.right_sidebar_open = open;
            cx.notify();
        }
    }

    pub fn is_menu_open(&self) -> bool {
        self.menu_open
    }

    pub fn toggle_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu_open {
            self.close_menu(cx);
        } else {
            self.open_menu(cx);
        }
    }

    pub fn open_menu(&mut self, cx: &mut Context<Self>) {
        self.menu_open = true;
        self.active_top_menu = TopMenuCategory::File;
        self.animate_to(1.0, cx);
    }

    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        self.animate_to(0.0, cx);
    }

    fn animate_to(&mut self, target: f32, cx: &mut Context<Self>) {
        let is_opening = target > 0.5;
        let start = self.anim_progress;
        let steps = 10;
        let step_dur = Duration::from_millis(15);

        self._anim_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            for i in 1..=steps {
                smol::Timer::after(step_dur).await;

                let t = i as f32 / steps as f32;
                // Easing function: smooth step
                let progress = start + (target - start) * (3.0 * t * t - 2.0 * t * t * t);
                let result = this.update(cx, |this, cx| {
                    this.anim_progress = progress;
                    if !is_opening && progress <= 0.05 {
                        this.menu_open = false;
                        this.anim_progress = 0.0;
                    }
                    cx.notify();
                });
                if result.is_err() {
                    break;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.anim_progress = target;
                if !is_opening {
                    this.menu_open = false;
                }
                cx.notify();
            });
        }));
    }

    fn toggle_context_menu(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        self.context_menu_open = !self.context_menu_open;
        self.context_menu_pos = pos;
        cx.notify();
    }

    fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu_open {
            self.context_menu_open = false;
            cx.notify();
        }
    }

    /// Render the app dropdown menu overlay (must be called from a parent with full window coverage).
    pub fn render_menu(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let active_category = self.active_top_menu;

        let left_offset = if cfg!(target_os = "macos") {
            velowork_ui::tokens::TITLEBAR_MAC_OFFSET
        } else {
            match active_category {
                TopMenuCategory::File => velowork_ui::tokens::TITLEBAR_MENU_OFFSET,
                TopMenuCategory::Edit => px(100.0),
                TopMenuCategory::View => px(155.0),
                TopMenuCategory::Window => px(210.0),
                TopMenuCategory::Help => px(265.0),
            }
        };

        div()
            .id("app-menu-backdrop")
            .absolute()
            .inset_0()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _window, cx| {
                    cx.stop_propagation();
                    this.close_menu(cx);
                }),
            )
            .on_mouse_move(|_, _, cx| {
                cx.stop_propagation();
            })
            .child(
                div()
                    .absolute()
                    .top(px(32.0))
                    .left(left_offset)
                    .bg(rgb(t.bg_primary))
                    .border_1()
                    .border_color(p.border_subtle)
                    .rounded(RADIUS_MD)
                    .shadow_xl()
                    .min_w(px(210.0))
                    .py(SPACE_XS)
                    .id("app-menu-panel")
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .children(match active_category {
                        TopMenuCategory::File => vec![
                            menu_item(
                                "app-menu-new-window",
                                AppIcon::Plus,
                                i18n!(cx, "menu.new_window"),
                                &t,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_menu(cx);
                                window.dispatch_action(Box::new(NewWindow), cx);
                            }))
                            .into_any_element(),
                            menu_item(
                                "app-menu-settings",
                                AppIcon::Settings,
                                i18n!(cx, "menu.settings"),
                                &t,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_menu(cx);
                                window.dispatch_action(Box::new(ShowSettings), cx);
                            }))
                            .into_any_element(),
                            menu_item(
                                "app-menu-profiles",
                                AppIcon::Terminal,
                                i18n!(cx, "menu.profiles"),
                                &t,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_menu(cx);
                                window.dispatch_action(Box::new(ShowProfileManager), cx);
                            }))
                            .into_any_element(),
                            div()
                                .h(px(1.0))
                                .mx(SPACE_MD)
                                .my(SPACE_XS)
                                .bg(p.border_subtle)
                                .into_any_element(),
                            menu_item(
                                "app-menu-exit",
                                AppIcon::Close,
                                i18n!(cx, "menu.exit"),
                                &t,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_menu(cx);
                                window.dispatch_action(Box::new(Quit), cx);
                            }))
                            .into_any_element(),
                        ],
                        TopMenuCategory::Edit => vec![
                            menu_item(
                                "app-menu-copy",
                                AppIcon::Copy,
                                i18n!(cx, "common.action.copy"),
                                &t,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_menu(cx);
                                window.dispatch_action(Box::new(Copy), cx);
                            }))
                            .into_any_element(),
                            menu_item(
                                "app-menu-paste",
                                AppIcon::ClipboardPaste,
                                i18n!(cx, "common.action.paste"),
                                &t,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_menu(cx);
                                window.dispatch_action(Box::new(Paste), cx);
                            }))
                            .into_any_element(),
                        ],
                        TopMenuCategory::View => vec![
                            menu_item(
                                "app-menu-command-palette",
                                AppIcon::Search,
                                i18n!(cx, "menu.command_palette"),
                                &t,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_menu(cx);
                                window.dispatch_action(Box::new(ShowCommandPalette), cx);
                            }))
                            .into_any_element(),
                            menu_item(
                                "app-menu-theme",
                                AppIcon::Eye,
                                i18n!(cx, "menu.appearance"),
                                &t,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_menu(cx);
                                window.dispatch_action(Box::new(ShowThemeSelector), cx);
                            }))
                            .into_any_element(),
                            div()
                                .h(px(1.0))
                                .mx(SPACE_MD)
                                .my(SPACE_XS)
                                .bg(p.border_subtle)
                                .into_any_element(),
                            menu_item(
                                "app-menu-toggle-sidebar",
                                AppIcon::SidebarLeft,
                                i18n!(cx, "menu.toggle_left_dock"),
                                &t,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_menu(cx);
                                window.dispatch_action(Box::new(ToggleLeftDock), cx);
                            }))
                            .into_any_element(),
                            menu_item(
                                "app-menu-toggle-right-sidebar",
                                AppIcon::SidebarRight,
                                i18n!(cx, "menu.toggle_right_dock"),
                                &t,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_menu(cx);
                                window.dispatch_action(Box::new(ToggleRightDock), cx);
                            }))
                            .into_any_element(),
                            div()
                                .h(px(1.0))
                                .mx(SPACE_MD)
                                .my(SPACE_XS)
                                .bg(p.border_subtle)
                                .into_any_element(),
                            menu_item(
                                "app-menu-keybindings",
                                AppIcon::Keyboard,
                                i18n!(cx, "menu.shortcuts"),
                                &t,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_menu(cx);
                                window.dispatch_action(Box::new(ShowKeybindings), cx);
                            }))
                            .into_any_element(),
                        ],
                        TopMenuCategory::Window => vec![
                            menu_item(
                                "app-menu-win-new",
                                AppIcon::Plus,
                                i18n!(cx, "menu.new_window"),
                                &t,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_menu(cx);
                                window.dispatch_action(Box::new(NewWindow), cx);
                            }))
                            .into_any_element(),
                            menu_item(
                                "app-menu-win-close",
                                AppIcon::Close,
                                i18n!(cx, "menu.close_window"),
                                &t,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_menu(cx);
                                if let Some(ref handler) = this.close_handler {
                                    handler(window, cx);
                                } else {
                                    window.remove_window();
                                }
                            }))
                            .into_any_element(),
                        ],
                        TopMenuCategory::Help => vec![
                            menu_item(
                                "app-menu-about",
                                AppIcon::Info,
                                i18n!(cx, "menu.about"),
                                &t,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_menu(cx);
                                window.dispatch_action(Box::new(About), cx);
                            }))
                            .into_any_element(),
                            menu_item(
                                "app-menu-updates",
                                AppIcon::Download,
                                i18n!(cx, "menu.check_updates"),
                                &t,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_menu(cx);
                                window.dispatch_action(Box::new(CheckForUpdates), cx);
                            }))
                            .into_any_element(),
                        ],
                    }),
            )
    }

    /// Render context menu on right click
    fn render_context_menu(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = theme(cx);
        let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);
        let pos = self.context_menu_pos;

        div()
            .id("title-bar-context-menu-backdrop")
            .absolute()
            .inset_0()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.close_context_menu(cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.close_context_menu(cx);
                }),
            )
            .child(
                div()
                    .absolute()
                    .top(pos.y)
                    .left(pos.x)
                    .occlude()
                    .bg(p.surface_overlay)
                    .border_1()
                    .border_color(p.border_subtle)
                    .rounded(RADIUS_LG)
                    .shadow_xl()
                    .min_w(px(180.0))
                    .p(SPACE_XS)
                    .id("title-bar-context-menu-panel")
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(
                        menu_item(
                            "ctx-toggle-left-sidebar",
                            AppIcon::SidebarLeft,
                            i18n!(cx, "menu.toggle_left_dock"),
                            &t,
                            cx,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_context_menu(cx);
                            window.dispatch_action(Box::new(ToggleLeftDock), cx);
                        })),
                    )
                    .child(
                        menu_item(
                            "ctx-toggle-right-sidebar",
                            AppIcon::SidebarRight,
                            i18n!(cx, "menu.toggle_right_dock"),
                            &t,
                            cx,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_context_menu(cx);
                            window.dispatch_action(Box::new(ToggleRightDock), cx);
                        })),
                    )
                    .child(div().h(px(1.0)).my(SPACE_XS).bg(p.border_subtle))
                    .child(
                        menu_item(
                            "ctx-toggle-titlebar-style",
                            AppIcon::Settings,
                            i18n!(cx, "settings.titlebar_style.label"),
                            &t,
                            cx,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.close_context_menu(cx);
                            settings_entity(cx).update(cx, |state, cx| {
                                let cur = state.settings.titlebar_style;
                                let next = match cur {
                                    TitlebarStyle::Custom => TitlebarStyle::Native,
                                    TitlebarStyle::Native => TitlebarStyle::Custom,
                                };
                                state.set_titlebar_style(next, cx);
                            });
                            let toast = velowork_workspace::toast::Toast::warning(i18n!(
                                cx,
                                "settings.titlebar_style_changed_notice"
                            ))
                            .with_actions(vec![
                                velowork_workspace::toast::ToastAction::new(
                                    "restart_app",
                                    i18n!(cx, "update.restart"),
                                    velowork_workspace::toast::ToastActionStyle::Primary,
                                ),
                            ]);
                            velowork_workspace::toast::ToastManager::post(toast, cx);
                        })),
                    ),
            )
    }
}

impl Render for TitleBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let _p = velowork_ui::design::semantic::SemanticPalette::from_theme(&t);
        let settings = settings_entity(cx).read(cx).settings.clone();

        let style_override = match settings.titlebar_preset {
            velowork_workspace::settings::CustomTitlebarPreset::Auto => None,
            velowork_workspace::settings::CustomTitlebarPreset::MacOS => {
                Some(velowork_ui::decorations::WindowControlStyle::MacOS)
            }
            velowork_workspace::settings::CustomTitlebarPreset::Windows11 => {
                Some(velowork_ui::decorations::WindowControlStyle::Windows11)
            }
            velowork_workspace::settings::CustomTitlebarPreset::LinuxCSD => {
                Some(velowork_ui::decorations::WindowControlStyle::LinuxCSD)
            }
            velowork_workspace::settings::CustomTitlebarPreset::KDEBreeze => {
                Some(velowork_ui::decorations::WindowControlStyle::KDEBreeze)
            }
        };
        let pos_override = match settings.titlebar_position {
            velowork_workspace::settings::CustomTitlebarPosition::Auto => None,
            velowork_workspace::settings::CustomTitlebarPosition::Left => {
                Some(velowork_ui::decorations::WindowButtonPosition::Left)
            }
            velowork_workspace::settings::CustomTitlebarPosition::Right => {
                Some(velowork_ui::decorations::WindowButtonPosition::Right)
            }
        };

        let decoration_config = WindowDecorationConfig::from_custom(
            style_override,
            pos_override,
            Some(settings.window_control_button_gap),
            Some(settings.window_control_margin),
        );

        let needs_controls = velowork_ui::overlay::detached_needs_controls(window);
        let is_left_controls = decoration_config.position == WindowButtonPosition::Left;
        let is_right_controls = decoration_config.position == WindowButtonPosition::Right;

        let scale = velowork_ui::tokens::ui_scale_factor(cx);
        let custom_margin = settings.window_control_margin * scale;

        let left_padding = if is_left_controls {
            px(custom_margin)
        } else if cfg!(target_os = "macos") && !needs_controls {
            px(80.0 * scale)
        } else {
            px(12.0 * scale)
        };

        let right_padding = if is_right_controls {
            px(custom_margin)
        } else {
            SPACE_SM
        };

        let base_height = settings.titlebar_height;
        let title_bar_height = px(base_height * scale);
        let custom_icon_sz = settings.window_control_icon_size;

        let context_menu_open = self.context_menu_open;

        let is_custom_titlebar = if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
            settings.titlebar_style == TitlebarStyle::Custom
        } else {
            matches!(window.window_decorations(), Decorations::Client { .. })
        };
        let window_corner_radius = settings.window_corner_radius;
        let is_maximized = window.is_maximized();
        let is_fullscreen = window.is_fullscreen();
        let has_rounded_corners =
            is_custom_titlebar && !is_maximized && !is_fullscreen && window_corner_radius > 0.0;
        let radius = px(window_corner_radius);

        let app_icon_sz = px((base_height * 0.52).clamp(14.0, 24.0) * scale);

        let p = SemanticPalette::from_context(cx);

        div()
            .id("title-bar")
            .occlude()
            .h(title_bar_height)
            .w_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .bg(p.surface_card)
            .border_b_1()
            .border_color(p.border_subtle)
            .when(has_rounded_corners, |d| {
                d.rounded_tl(radius).rounded_tr(radius)
            })
            .window_control_area(WindowControlArea::Drag)
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.toggle_context_menu(event.position, cx);
                }),
            )
            .when(cfg!(target_os = "linux"), |d| {
                d.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, _cx| {
                        #[cfg(target_os = "linux")]
                        {
                            this.should_move = true;
                        }
                        #[cfg(not(target_os = "linux"))]
                        {
                            let _ = this;
                        }
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _, _, _cx| {
                        #[cfg(target_os = "linux")]
                        {
                            this.should_move = false;
                        }
                        #[cfg(not(target_os = "linux"))]
                        {
                            let _ = this;
                        }
                    }),
                )
                .on_mouse_move(cx.listener(|this, _, window, _cx| {
                    #[cfg(target_os = "linux")]
                    if this.should_move {
                        this.should_move = false;
                        window.start_window_move();
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        let _ = (this, window);
                    }
                }))
                .on_click(|event: &ClickEvent, window, _| {
                    if event.click_count() == 2 {
                        window.zoom_window();
                    }
                })
            })
            .child(
                // Left side - Window Controls (if Left position) + App Icon
                h_flex()
                    .gap(SPACE_SM)
                    .pl(left_padding)
                    .when(needs_controls && is_left_controls, |d| {
                        let custom_close = self.close_handler.clone();
                        d.child(velowork_ui::title_bar::render_window_controls(
                            "main-win-ctrl-left",
                            window,
                            &decoration_config,
                            Some(custom_icon_sz),
                            custom_close,
                            &t,
                            cx,
                        ))
                    })
                    .child(
                        h_flex()
                            .id("titlebar-app-icon")
                            .items_center()
                            .child(brand_logo(app_icon_sz, window, cx)),
                    ),
            )
            .child(
                // Center - App Title
                div().flex_1().flex().items_center().justify_center().child(
                    h_flex().gap(SPACE_XS).items_center().child(
                        div()
                            .text_size(ui_text(13.0, cx))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(t.text_primary))
                            .child(self.title.clone()),
                    ),
                ),
            )
            .child(
                // Right side - Window Controls (if Right position)
                h_flex()
                    .pr(right_padding)
                    .when(needs_controls && is_right_controls, |d| {
                        let custom_close = self.close_handler.clone();
                        d.child(velowork_ui::title_bar::render_window_controls(
                            "main-win-ctrl-right",
                            window,
                            &decoration_config,
                            Some(custom_icon_sz),
                            custom_close,
                            &t,
                            cx,
                        ))
                    }),
            )
            .when(context_menu_open, |d| d.child(self.render_context_menu(cx)))
    }
}
