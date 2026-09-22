//! Context menu for terminal content (right-click on terminal area),
//! built using the unified `velowork_ui::menu::PopupMenu` component architecture.

use gpui::*;
use std::sync::Arc;
use velowork_i18n::i18n;
use velowork_ui::icon::AppIcon;
use velowork_ui::menu::{ContextMenu, PopupMenu, PopupMenuItem};
use velowork_ui::overlay_registry::OverlayRegistry;
use velowork_workspace::settings::SearchEngineConfig;
use velowork_workspace::state::SplitDirection;

/// Event emitted by TerminalContextMenu actions.
#[derive(Clone, Debug)]
pub enum TerminalContextMenuEvent {
    Close,
    Copy {
        terminal_id: String,
    },
    Paste {
        terminal_id: String,
    },
    Clear {
        terminal_id: String,
    },
    SelectAll {
        terminal_id: String,
    },
    Split {
        project_id: String,
        layout_path: Vec<usize>,
        direction: SplitDirection,
    },
    CloseTerminal {
        project_id: String,
        terminal_id: String,
    },
    OpenLink {
        url: String,
    },
    CopyLink {
        url: String,
    },
    ExportSelected {
        terminal_id: String,
    },
    ExportAll {
        terminal_id: String,
    },
    LogPause {
        terminal_id: String,
    },
    LogResume {
        terminal_id: String,
    },
    LogStop {
        terminal_id: String,
    },
    LogOpenFile {
        terminal_id: String,
    },
    LogOpenFolder {
        terminal_id: String,
    },
    ShowLogRecordDialog {
        terminal_id: String,
    },
    AIInterpret {
        terminal_id: String,
        text: String,
    },
    Find {
        terminal_id: String,
    },
    WebSearch {
        terminal_id: String,
    },
    SearchWithEngine {
        terminal_id: String,
        url: String,
    },
    ToggleWordWrap,
    ToggleLineNumbers,
    ClearScrollback {
        terminal_id: String,
    },
    ClearAll {
        terminal_id: String,
    },
    EditConfig {
        terminal_id: String,
    },
    ZmodemUpload {
        terminal_id: String,
    },
}

/// Percent-encode a search query for use in a URL (RFC 3986 unreserved set;
/// spaces become `+`, matching typical search engine behavior).
fn percent_encode(text: &str) -> String {
    let mut encoded = String::new();
    for b in text.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            b' ' => {
                encoded.push('+');
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", b));
            }
        }
    }
    encoded
}

/// Construct and open a unified terminal context menu.
#[allow(clippy::too_many_arguments)]
pub fn open_terminal_context_menu(
    terminal_id: String,
    project_id: String,
    layout_path: Vec<usize>,
    position: Point<Pixels>,
    has_selection: bool,
    selection: String,
    link_url: Option<String>,
    is_recording: bool,
    is_recording_paused: bool,
    word_wrap_enabled: bool,
    line_numbers_enabled: bool,
    ai_enabled: bool,
    search_engines: Vec<SearchEngineConfig>,
    overlay_registry: Option<Entity<OverlayRegistry>>,
    on_event: impl Fn(TerminalContextMenuEvent, &mut App) + Send + Sync + 'static,
    cx: &mut App,
) -> Entity<PopupMenu> {
    let on_event = Arc::new(on_event);
    let mut items = Vec::new();

    let open_in_browser = i18n!(cx, "terminal.open_in_browser");
    let copy_link = i18n!(cx, "terminal.copy_link");
    let copy = i18n!(cx, "common.action.copy");
    let paste = i18n!(cx, "common.action.paste");
    let select_all = i18n!(cx, "common.action.select_all");
    let split_horizontal = i18n!(cx, "terminal.split_horizontal");
    let split_vertical = i18n!(cx, "terminal.split_vertical");
    let edit_config = i18n!(cx, "terminal.edit_config");
    let close_term = i18n!(cx, "terminal.close");

    let ai_interpret = i18n!(cx, "terminal.ai_interpret");
    let find = i18n!(cx, "terminal.find");
    let _web_search = i18n!(cx, "terminal.web_search");
    let word_wrap = i18n!(cx, "terminal.word_wrap");
    let line_numbers = i18n!(cx, "terminal.line_numbers");
    let clear_scrollback = i18n!(cx, "terminal.clear_scrollback");
    let clear_all = i18n!(cx, "terminal.clear_all");

    let log_recording = i18n!(cx, "terminal.log_recording");
    let log_start = i18n!(cx, "terminal.log_start");
    let log_pause = i18n!(cx, "terminal.log_pause");
    let log_resume = i18n!(cx, "terminal.log_resume");
    let log_stop = i18n!(cx, "terminal.log_stop");
    let log_open_file = i18n!(cx, "terminal.log_open_file");
    let log_open_folder = i18n!(cx, "terminal.log_open_folder");

    // 1. Link actions
    if let Some(url) = link_url {
        let u1 = url.clone();
        let ev1 = on_event.clone();
        items.push(
            PopupMenuItem::item("ctx-open-link", open_in_browser, move |_, cx| {
                ev1(TerminalContextMenuEvent::OpenLink { url: u1.clone() }, cx);
            })
            .icon(AppIcon::ExternalLink),
        );

        let u2 = url;
        let ev2 = on_event.clone();
        items.push(
            PopupMenuItem::item("ctx-copy-link", copy_link, move |_, cx| {
                ev2(TerminalContextMenuEvent::CopyLink { url: u2.clone() }, cx);
            })
            .icon(AppIcon::Copy),
        );

        items.push(PopupMenuItem::separator());
    }

    // 2. Clipboard actions
    let tid_copy = terminal_id.clone();
    let ev_copy = on_event.clone();
    items.push(
        PopupMenuItem::item("ctx-copy", copy, move |_, cx| {
            ev_copy(
                TerminalContextMenuEvent::Copy {
                    terminal_id: tid_copy.clone(),
                },
                cx,
            );
        })
        .icon(AppIcon::Copy)
        .shortcut("Ctrl+Shift+C")
        .disabled(!has_selection),
    );

    let tid_paste = terminal_id.clone();
    let ev_paste = on_event.clone();
    items.push(
        PopupMenuItem::item("ctx-paste", paste, move |_, cx| {
            ev_paste(
                TerminalContextMenuEvent::Paste {
                    terminal_id: tid_paste.clone(),
                },
                cx,
            );
        })
        .icon(AppIcon::ClipboardPaste)
        .shortcut("Ctrl+Shift+V"),
    );

    let tid_selall = terminal_id.clone();
    let ev_selall = on_event.clone();
    items.push(
        PopupMenuItem::item("ctx-select-all", select_all, move |_, cx| {
            ev_selall(
                TerminalContextMenuEvent::SelectAll {
                    terminal_id: tid_selall.clone(),
                },
                cx,
            );
        })
        .icon(AppIcon::SelectAll)
        .shortcut("Ctrl+A"),
    );

    items.push(PopupMenuItem::separator());

    // 3. AI & Search
    // "AI Interpret" only shows when the AI assistant is enabled in settings.
    let tid_ai = terminal_id.clone();
    let ev_ai = on_event.clone();
    let ai_selection = selection.clone();
    if ai_enabled {
        items.push(
            PopupMenuItem::item("ctx-ai-interpret", ai_interpret, move |_, cx| {
                ev_ai(
                    TerminalContextMenuEvent::AIInterpret {
                        terminal_id: tid_ai.clone(),
                        text: ai_selection.clone(),
                    },
                    cx,
                );
            })
            .icon(AppIcon::AiAssistant)
            .disabled(!has_selection),
        );
    }

    let tid_find = terminal_id.clone();
    let ev_find = on_event.clone();
    items.push(
        PopupMenuItem::item("ctx-find", find, move |_, cx| {
            ev_find(
                TerminalContextMenuEvent::Find {
                    terminal_id: tid_find.clone(),
                },
                cx,
            );
        })
        .icon(AppIcon::Search)
        .shortcut("Ctrl+F"),
    );

    // "Search Online" submenu — lists enabled search engines from settings.
    let tid_web = terminal_id.clone();
    let ev_web = on_event.clone();
    let search_label = i18n!(cx, "terminal.context_menu.search_online");
    let mut search_sub_items: Vec<PopupMenuItem> = search_engines
        .iter()
        .map(|engine| {
            let tid = tid_web.clone();
            let ev = ev_web.clone();
            // Build the final search URL at menu-build time so it does not depend
            // on the terminal selection still being alive when the item is clicked.
            let mut final_url = engine.url.clone();
            if let Some(pos) = final_url.find("%s") {
                final_url.replace_range(pos..pos + 2, &percent_encode(&selection));
            }
            let url_moved = final_url.clone();
            let label = if engine.keyword.is_empty() {
                engine.name.clone()
            } else {
                format!("{}  {}", engine.name, engine.keyword)
            };
            PopupMenuItem::item(format!("ctx-search-{}", engine.id), label, move |_, cx| {
                ev(
                    TerminalContextMenuEvent::SearchWithEngine {
                        terminal_id: tid.clone(),
                        url: url_moved.clone(),
                    },
                    cx,
                );
            })
            .icon(AppIcon::Globe)
            .disabled(!has_selection)
        })
        .collect();
    if search_sub_items.is_empty() {
        // Provide a disabled placeholder so the submenu is still discoverable.
        search_sub_items.push(
            PopupMenuItem::item(
                "ctx-search-none",
                i18n!(cx, "settings.search_engines.empty"),
                |_, _| {},
            )
            .disabled(true),
        );
    }
    items.push(
        PopupMenuItem::submenu("ctx-web-search", search_label, search_sub_items)
            .icon(AppIcon::Globe)
            .disabled(!has_selection),
    );

    items.push(PopupMenuItem::separator());

    // 4. Log Recording Submenu
    let mut log_sub_items = Vec::new();
    if !is_recording {
        let tid = terminal_id.clone();
        let ev = on_event.clone();
        log_sub_items.push(
            PopupMenuItem::item("ctx-log-start", log_start, move |_, cx| {
                ev(
                    TerminalContextMenuEvent::ShowLogRecordDialog {
                        terminal_id: tid.clone(),
                    },
                    cx,
                );
            })
            .icon(AppIcon::Play),
        );
    } else {
        let tid_stop = terminal_id.clone();
        let ev_stop = on_event.clone();
        log_sub_items.push(
            PopupMenuItem::item("ctx-log-stop", log_stop, move |_, cx| {
                ev_stop(
                    TerminalContextMenuEvent::LogStop {
                        terminal_id: tid_stop.clone(),
                    },
                    cx,
                );
            })
            .icon(AppIcon::Stop),
        );

        let tid_pr = terminal_id.clone();
        let ev_pr = on_event.clone();
        if is_recording_paused {
            log_sub_items.push(
                PopupMenuItem::item("ctx-log-resume", log_resume, move |_, cx| {
                    ev_pr(
                        TerminalContextMenuEvent::LogResume {
                            terminal_id: tid_pr.clone(),
                        },
                        cx,
                    );
                })
                .icon(AppIcon::Play),
            );
        } else {
            log_sub_items.push(
                PopupMenuItem::item("ctx-log-pause", log_pause, move |_, cx| {
                    ev_pr(
                        TerminalContextMenuEvent::LogPause {
                            terminal_id: tid_pr.clone(),
                        },
                        cx,
                    );
                })
                .icon(AppIcon::Pause),
            );
        }
    }

    let tid_of = terminal_id.clone();
    let ev_of = on_event.clone();
    log_sub_items.push(
        PopupMenuItem::item("ctx-log-open-file", log_open_file, move |_, cx| {
            ev_of(
                TerminalContextMenuEvent::LogOpenFile {
                    terminal_id: tid_of.clone(),
                },
                cx,
            );
        })
        .icon(AppIcon::File)
        .disabled(!is_recording),
    );

    let tid_od = terminal_id.clone();
    let ev_od = on_event.clone();
    log_sub_items.push(
        PopupMenuItem::item("ctx-log-open-folder", log_open_folder, move |_, cx| {
            ev_od(
                TerminalContextMenuEvent::LogOpenFolder {
                    terminal_id: tid_od.clone(),
                },
                cx,
            );
        })
        .icon(AppIcon::Folder)
        .disabled(!is_recording),
    );

    items.push(
        PopupMenuItem::submenu("ctx-log-recording", log_recording, log_sub_items)
            .icon(AppIcon::Terminal),
    );

    let zmodem_upload_label = i18n!(cx, "terminal.context_menu.zmodem_upload");
    let tid_zm = terminal_id.clone();
    let ev_zm = on_event.clone();
    items.push(
        PopupMenuItem::item("ctx-zmodem-upload", zmodem_upload_label, move |_, cx| {
            ev_zm(
                TerminalContextMenuEvent::ZmodemUpload {
                    terminal_id: tid_zm.clone(),
                },
                cx,
            );
        })
        .icon(AppIcon::ArrowUp),
    );

    items.push(PopupMenuItem::separator());

    // 5. Layout & Split
    let pid_h = project_id.clone();
    let path_h = layout_path.clone();
    let ev_h = on_event.clone();
    items.push(
        PopupMenuItem::item("ctx-split-horizontal", split_horizontal, move |_, cx| {
            ev_h(
                TerminalContextMenuEvent::Split {
                    project_id: pid_h.clone(),
                    layout_path: path_h.clone(),
                    direction: SplitDirection::Horizontal,
                },
                cx,
            );
        })
        .icon(AppIcon::SplitHorizontal),
    );

    let pid_v = project_id.clone();
    let path_v = layout_path.clone();
    let ev_v = on_event.clone();
    items.push(
        PopupMenuItem::item("ctx-split-vertical", split_vertical, move |_, cx| {
            ev_v(
                TerminalContextMenuEvent::Split {
                    project_id: pid_v.clone(),
                    layout_path: path_v.clone(),
                    direction: SplitDirection::Vertical,
                },
                cx,
            );
        })
        .icon(AppIcon::SplitVertical),
    );

    items.push(PopupMenuItem::separator());

    // 6. View Options
    let ev_ww = on_event.clone();
    items.push(
        PopupMenuItem::item("ctx-word-wrap", word_wrap, move |_, cx| {
            ev_ww(TerminalContextMenuEvent::ToggleWordWrap, cx);
        })
        .icon(AppIcon::Wrapline)
        .checked(word_wrap_enabled),
    );

    let ev_ln = on_event.clone();
    items.push(
        PopupMenuItem::item("ctx-line-numbers", line_numbers, move |_, cx| {
            ev_ln(TerminalContextMenuEvent::ToggleLineNumbers, cx);
        })
        .icon(AppIcon::Code)
        .checked(line_numbers_enabled),
    );

    items.push(PopupMenuItem::separator());

    // 7. Clear & Close
    let tid_clear_sb = terminal_id.clone();
    let ev_clear_sb = on_event.clone();
    items.push(
        PopupMenuItem::item("ctx-clear-scrollback", clear_scrollback, move |_, cx| {
            ev_clear_sb(
                TerminalContextMenuEvent::ClearScrollback {
                    terminal_id: tid_clear_sb.clone(),
                },
                cx,
            );
        })
        .icon(AppIcon::PaintRoller),
    );

    let tid_clear_all = terminal_id.clone();
    let ev_clear_all = on_event.clone();
    items.push(
        PopupMenuItem::item("ctx-clear-all", clear_all, move |_, cx| {
            ev_clear_all(
                TerminalContextMenuEvent::ClearAll {
                    terminal_id: tid_clear_all.clone(),
                },
                cx,
            );
        })
        .icon(AppIcon::BrushCleaning),
    );

    items.push(PopupMenuItem::separator());

    // 8. Edit Configuration & Close
    let tid_cfg = terminal_id.clone();
    let ev_cfg = on_event.clone();
    items.push(
        PopupMenuItem::item("ctx-edit-config", edit_config, move |_, cx| {
            ev_cfg(
                TerminalContextMenuEvent::EditConfig {
                    terminal_id: tid_cfg.clone(),
                },
                cx,
            );
        })
        .icon(AppIcon::Settings),
    );

    let pid_close = project_id;
    let tid_close = terminal_id;
    let ev_close = on_event.clone();
    items.push(
        PopupMenuItem::item("ctx-close", close_term, move |_, cx| {
            ev_close(
                TerminalContextMenuEvent::CloseTerminal {
                    project_id: pid_close.clone(),
                    terminal_id: tid_close.clone(),
                },
                cx,
            );
        })
        .icon(AppIcon::Close),
    );

    let ev_on_close = on_event;
    let on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync> = Arc::new(move |_, cx| {
        ev_on_close(TerminalContextMenuEvent::Close, cx);
    });

    ContextMenu::build(cx, position, items, overlay_registry, Some(on_close))
}
