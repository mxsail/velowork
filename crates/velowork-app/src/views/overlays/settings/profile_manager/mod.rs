mod actions;
mod render;

use gpui::*;
use velowork_core::profiles::ProfileEntry;
use velowork_i18n::i18n;
use velowork_ui::simple_input::SimpleInputState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PendingFocus {
    List,
    DeleteCancel,
    DeleteConfirm,
}

pub struct ProfileManager {
    pub(crate) focus_handle: FocusHandle,
    pub(crate) list_focus: FocusHandle,
    pub(crate) create_button_focus: FocusHandle,
    pub(crate) delete_cancel_focus: FocusHandle,
    pub(crate) delete_confirm_focus: FocusHandle,
    pub(crate) scroll_handle: ScrollHandle,
    pub(crate) pending_focus: Option<PendingFocus>,
    pub(crate) selected_index: usize,
    pub(crate) profiles: Vec<ProfileEntry>,
    pub(crate) active_id: String,
    pub(crate) default_profile_id: String,
    pub(crate) new_profile_input: Entity<SimpleInputState>,
    pub(crate) error_message: Option<String>,
    pub(crate) show_delete_confirmation: Option<String>,
}

impl ProfileManager {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let list_focus = cx.focus_handle();
        let create_button_focus = cx.focus_handle();
        let delete_cancel_focus = cx.focus_handle();
        let delete_confirm_focus = cx.focus_handle();
        let scroll_handle = ScrollHandle::new();

        let active_id = velowork_core::profiles::try_current()
            .map(|p| p.id.clone())
            .unwrap_or_default();

        let profiles = velowork_core::profiles::all_profiles().unwrap_or_default();
        let selected_index = profiles
            .iter()
            .position(|p| p.id == active_id)
            .unwrap_or(0);

        let default_profile_id = velowork_core::profiles::ProfileIndex::load(
            &velowork_core::profiles::config_root(),
        )
        .map(|idx| idx.default_profile)
        .unwrap_or_else(|_| "default".to_string());

        let name_ph = i18n!(cx, "profile.manager.name_placeholder");
        let new_profile_input = cx.new(|cx| {
            SimpleInputState::new(cx).placeholder(name_ph)
        });

        Self {
            focus_handle,
            list_focus,
            create_button_focus,
            delete_cancel_focus,
            delete_confirm_focus,
            scroll_handle,
            pending_focus: Some(PendingFocus::List),
            selected_index,
            profiles,
            active_id,
            default_profile_id,
            new_profile_input,
            error_message: None,
            show_delete_confirmation: None,
        }
    }
}

pub enum ProfileManagerEvent {
    Close,
    SwitchProfile(String),
}

impl EventEmitter<ProfileManagerEvent> for ProfileManager {}

impl_focusable!(ProfileManager);

#[cfg(test)]
mod tests;

