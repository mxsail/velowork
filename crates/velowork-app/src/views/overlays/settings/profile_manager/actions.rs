use gpui::*;
use velowork_i18n::i18n;

use super::{PendingFocus, ProfileManager, ProfileManagerEvent};

impl ProfileManager {
    pub(super) fn close(&self, cx: &mut Context<Self>) {
        cx.emit(ProfileManagerEvent::Close);
    }

    pub(super) fn refresh_profiles(&mut self) {
        self.profiles = velowork_core::profiles::all_profiles().unwrap_or_default();
        self.error_message = None;
    }

    pub(super) fn create_profile(&mut self, cx: &mut Context<Self>) {
        let name = self.new_profile_input.read(cx).value().trim().to_string();
        if name.is_empty() {
            self.error_message = Some(i18n!(cx, "profile_manager.empty_name"));
            cx.notify();
            return;
        }

        if self.profiles.iter().any(|p| {
            p.display_name.trim().eq_ignore_ascii_case(&name)
                || p.id.trim().eq_ignore_ascii_case(&name)
        }) {
            self.error_message = Some(i18n!(cx, "profile_manager.duplicate_name"));
            cx.notify();
            return;
        }

        match velowork_core::profiles::create_profile(&name) {
            Ok(id) => {
                self.new_profile_input.update(cx, |input, cx| {
                    input.set_value("", cx);
                });
                self.refresh_profiles();
                if let Some(pos) = self.profiles.iter().position(|p| p.id == id) {
                    self.selected_index = pos;
                }
                self.pending_focus = Some(PendingFocus::List);
            }
            Err(e) => {
                self.error_message = Some(
                    i18n!(cx, "profile_manager.create_failed").replace("{error}", &e.to_string()),
                );
            }
        }
        cx.notify();
    }

    pub(super) fn open_profile_dir(&self, id: &str) {
        let dir = velowork_core::profiles::config_root().join("profiles").join(id);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!("Failed to ensure profile dir {dir:?}: {e}");
        }
        #[cfg(target_os = "macos")]
        let _ = velowork_core::process::spawn_and_reap(
            velowork_core::process::command("open").arg(&dir),
        );
        #[cfg(target_os = "linux")]
        let _ = velowork_core::process::spawn_and_reap(
            velowork_core::process::command("xdg-open").arg(&dir),
        );
        #[cfg(target_os = "windows")]
        let _ = velowork_core::process::spawn_and_reap(
            velowork_core::process::command("explorer").arg(&dir),
        );
    }

    pub(super) fn confirm_delete(&mut self, id: &str, cx: &mut Context<Self>) {
        self.show_delete_confirmation = Some(id.to_string());
        self.error_message = None;
        self.pending_focus = Some(PendingFocus::DeleteCancel);
        cx.notify();
    }

    pub(super) fn cancel_delete(&mut self, cx: &mut Context<Self>) {
        self.show_delete_confirmation = None;
        self.error_message = None;
        self.pending_focus = Some(PendingFocus::List);
        cx.notify();
    }

    pub(super) fn delete_profile(&mut self, id: &str, cx: &mut Context<Self>) {
        match velowork_core::profiles::delete_profile(id) {
            Ok(()) => {
                self.show_delete_confirmation = None;
                self.refresh_profiles();
                if self.selected_index >= self.profiles.len() {
                    self.selected_index = self.profiles.len().saturating_sub(1);
                }
                self.pending_focus = Some(PendingFocus::List);
            }
            Err(e) => {
                self.show_delete_confirmation = None;
                self.error_message = Some(format!("{e}"));
                self.pending_focus = Some(PendingFocus::List);
            }
        }
        cx.notify();
    }

    pub(super) fn switch_to(&mut self, id: String, cx: &mut Context<Self>) {
        cx.emit(ProfileManagerEvent::SwitchProfile(id));
    }

    pub(super) fn handle_list_key_down(
        &mut self,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) {
        match event.keystroke.key.as_str() {
            "up" => {
                cx.stop_propagation();
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                    self.scroll_handle.scroll_to_item(self.selected_index);
                    cx.notify();
                }
            }
            "down" => {
                cx.stop_propagation();
                if self.selected_index + 1 < self.profiles.len() {
                    self.selected_index += 1;
                    self.scroll_handle.scroll_to_item(self.selected_index);
                    cx.notify();
                }
            }
            "home" => {
                cx.stop_propagation();
                if !self.profiles.is_empty() {
                    self.selected_index = 0;
                    self.scroll_handle.scroll_to_item(0);
                    cx.notify();
                }
            }
            "end" => {
                cx.stop_propagation();
                if !self.profiles.is_empty() {
                    self.selected_index = self.profiles.len() - 1;
                    self.scroll_handle.scroll_to_item(self.selected_index);
                    cx.notify();
                }
            }
            "enter" | "space" => {
                cx.stop_propagation();
                if let Some(profile) = self
                    .profiles
                    .get(self.selected_index)
                    .filter(|p| p.id != self.active_id)
                {
                    self.switch_to(profile.id.clone(), cx);
                }
            }
            "delete" => {
                cx.stop_propagation();
                if let Some(profile) = self
                    .profiles
                    .get(self.selected_index)
                    .filter(|p| p.id != self.active_id && p.id != self.default_profile_id)
                {
                    let id = profile.id.clone();
                    self.confirm_delete(&id, cx);
                }
            }
            _ => {}
        }
    }
}

