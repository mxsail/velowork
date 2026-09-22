use super::{PendingFocus, ProfileManager};
use crate::init::init_settings;
use gpui::{AppContext as _, Context, KeyDownEvent, Keystroke, TestAppContext};
use velowork_core::profiles::ProfileEntry;
use velowork_i18n::{init_locale, t, Locale};

fn key_down_event(key: &str) -> KeyDownEvent {
    KeyDownEvent {
        keystroke: Keystroke::parse(key).expect("valid keystroke"),
        is_held: false,
        prefer_character_input: false,
    }
}

fn create_test_manager(cx: &mut Context<ProfileManager>) -> ProfileManager {
    let mut manager = ProfileManager::new(cx);
    manager.profiles = vec![
        ProfileEntry {
            id: "default".to_string(),
            display_name: "Default".to_string(),
            color: None,
            icon: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
        },
        ProfileEntry {
            id: "work".to_string(),
            display_name: "Work".to_string(),
            color: None,
            icon: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
        },
        ProfileEntry {
            id: "personal".to_string(),
            display_name: "Personal".to_string(),
            color: None,
            icon: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
        },
    ];
    manager.active_id = "default".to_string();
    manager.default_profile_id = "default".to_string();
    manager.selected_index = 0;
    manager
}

#[gpui::test]
async fn test_profile_manager_i18n_keys(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_locale(Locale::Zh, cx);
        assert_eq!(t(cx, "profile.manager.empty_name"), "配置文件名称不能为空");
        assert_eq!(t(cx, "profile.manager.duplicate_name"), "已存在同名的配置文件");
        assert!(t(cx, "profile.manager.create_failed").contains("{error}"));

        init_locale(Locale::En, cx);
        assert_eq!(t(cx, "profile.manager.empty_name"), "Profile name cannot be empty");
        assert_eq!(t(cx, "profile.manager.duplicate_name"), "A profile with this name already exists");
        assert!(t(cx, "profile.manager.create_failed").contains("{error}"));
    });
}

#[gpui::test]
async fn test_profile_manager_navigation(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_locale(Locale::Zh, cx);
        init_settings(cx);
        let entity = cx.new(create_test_manager);

        entity.update(cx, |manager, cx| {
            assert_eq!(manager.selected_index, 0);

            // Down arrow moves to next item
            manager.handle_list_key_down(&key_down_event("down"), cx);
            assert_eq!(manager.selected_index, 1);

            manager.handle_list_key_down(&key_down_event("down"), cx);
            assert_eq!(manager.selected_index, 2);

            // Saturated at last item
            manager.handle_list_key_down(&key_down_event("down"), cx);
            assert_eq!(manager.selected_index, 2);

            // Up arrow moves back
            manager.handle_list_key_down(&key_down_event("up"), cx);
            assert_eq!(manager.selected_index, 1);

            // End key jumps to end
            manager.handle_list_key_down(&key_down_event("end"), cx);
            assert_eq!(manager.selected_index, 2);

            // Home key jumps to start
            manager.handle_list_key_down(&key_down_event("home"), cx);
            assert_eq!(manager.selected_index, 0);

            // Saturated at first item
            manager.handle_list_key_down(&key_down_event("up"), cx);
            assert_eq!(manager.selected_index, 0);
        });
    });
}

#[gpui::test]
async fn test_profile_manager_delete_protection_and_flow(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_locale(Locale::Zh, cx);
        init_settings(cx);
        let entity = cx.new(create_test_manager);

        entity.update(cx, |manager, cx| {
            // 1. Attempt delete on default/active profile (index 0)
            manager.selected_index = 0;
            manager.handle_list_key_down(&key_down_event("delete"), cx);
            assert!(manager.show_delete_confirmation.is_none());

            // 2. Navigate to deletable profile 'personal' (index 2)
            manager.selected_index = 2;
            manager.handle_list_key_down(&key_down_event("delete"), cx);
            assert_eq!(manager.show_delete_confirmation.as_deref(), Some("personal"));
            assert_eq!(manager.pending_focus, Some(PendingFocus::DeleteCancel));

            // 3. Cancel delete
            manager.cancel_delete(cx);
            assert!(manager.show_delete_confirmation.is_none());
            assert_eq!(manager.pending_focus, Some(PendingFocus::List));
        });
    });
}

#[gpui::test]
async fn test_profile_manager_empty_create_validation(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_locale(Locale::Zh, cx);
        init_settings(cx);
        let entity = cx.new(create_test_manager);

        entity.update(cx, |manager, cx| {
            assert!(manager.error_message.is_none());

            // 1. Empty string
            manager.new_profile_input.update(cx, |input, cx| {
                input.set_value("", cx);
            });
            manager.create_profile(cx);
            assert_eq!(
                manager.error_message.as_deref(),
                Some("配置文件名称不能为空")
            );

            // 2. Whitespace only
            manager.new_profile_input.update(cx, |input, cx| {
                input.set_value("    ", cx);
            });
            manager.create_profile(cx);
            assert_eq!(
                manager.error_message.as_deref(),
                Some("配置文件名称不能为空")
            );
        });
    });
}

#[gpui::test]
async fn test_profile_manager_duplicate_create_validation(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_locale(Locale::Zh, cx);
        init_settings(cx);
        let entity = cx.new(create_test_manager);

        entity.update(cx, |manager, cx| {
            assert!(manager.error_message.is_none());

            // Duplicate of display name: "Work"
            manager.new_profile_input.update(cx, |input, cx| {
                input.set_value("Work", cx);
            });
            manager.create_profile(cx);
            assert_eq!(
                manager.error_message.as_deref(),
                Some("已存在同名的配置文件")
            );

            // Duplicate of display name with case variation: "work"
            manager.new_profile_input.update(cx, |input, cx| {
                input.set_value("work", cx);
            });
            manager.create_profile(cx);
            assert_eq!(
                manager.error_message.as_deref(),
                Some("已存在同名的配置文件")
            );

            // Duplicate of id / name: "default"
            manager.new_profile_input.update(cx, |input, cx| {
                input.set_value("default", cx);
            });
            manager.create_profile(cx);
            assert_eq!(
                manager.error_message.as_deref(),
                Some("已存在同名的配置文件")
            );

            // Duplicate with extra spaces: "  Personal  "
            manager.new_profile_input.update(cx, |input, cx| {
                input.set_value("  Personal  ", cx);
            });
            manager.create_profile(cx);
            assert_eq!(
                manager.error_message.as_deref(),
                Some("已存在同名的配置文件")
            );
        });
    });
}
