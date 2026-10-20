use niri_config::{Action, Config};
use smithay::backend::input::{InputEvent, InputTime, KeyState};
use wayland_client::protocol::wl_pointer;

use super::*;
use crate::input::move_grab::MoveGrab;
use crate::input::resize_grab::ResizeGrab;
use crate::input::spatial_movement_grab::SpatialMovementGrab;
use crate::tests::test_input_backend::{TestInputBackend, TestKeyboardKeyEvent};

#[test]
fn modified_mouse_gestures_follow_dynamic_window_rules() {
    let reserve = r#"window-rule {
        match app-id="^dictation$"
        disable-modified-mouse-gestures true
    }"#;
    let override_false =
        format!("{reserve}\nwindow-rule {{ disable-modified-mouse-gestures false; }}");
    let explicit_bind = format!(
        r#"{reserve}
        binds {{ Mod+MouseLeft {{ spawn "unused"; }}; }}"#
    );
    for (rules, app_id, change_id, expect_grab, expect_action) in [
        ("", "dictation", false, true, false),
        (reserve, "dictation", false, false, false),
        (reserve, "other", false, true, false),
        (override_false.as_str(), "dictation", false, true, false),
        (reserve, "other", true, false, false),
        (explicit_bind.as_str(), "dictation", false, false, true),
    ] {
        for button in [272, 273, 274] {
            if expect_action && button != 272 {
                continue;
            }
            let mut config = Config::parse_mem(&format!(
                r#"
                input {{ mod-key "Super"; }}
                window-rule {{ open-floating true; }}
                {rules}
            "#
            ))
            .unwrap();
            for bind in &mut config.binds.0 {
                bind.action = Action::TestAction;
            }
            let mut f = Fixture::with_config(config);
            f.add_output(1, (1920, 1080));
            let id = f.add_client();
            let window = f.client(id).create_window();
            window.xdg_toplevel.set_app_id(app_id.to_owned());
            let surface = window.surface.clone();
            window.commit();
            f.roundtrip(id);
            let window = f.client(id).window(&surface);
            window.set_size(400, 300);
            window.attach_new_buffer();
            window.ack_last_and_commit();
            f.double_roundtrip(id);
            if change_id {
                let window = f.client(id).window(&surface);
                window.xdg_toplevel.set_app_id("dictation".to_owned());
                window.commit();
                f.double_roundtrip(id);
            }
            f.niri_state()
                .process_input_event(InputEvent::<TestInputBackend>::Keyboard {
                    event: TestKeyboardKeyEvent {
                        time: InputTime::from_micros(0),
                        code: 133u32.into(), // Super_L in the default evdev keymap.
                        state: KeyState::Pressed,
                        count: 1,
                    },
                });
            let client = f.client(id);
            let pointer = client
                .state
                .virtual_pointer_manager
                .as_ref()
                .unwrap()
                .create_virtual_pointer(None, &client.qh, ());
            pointer.motion_absolute(0, 800, 440, 1920, 1080);
            pointer.frame();
            f.roundtrip(id);
            pointer.button(0, button, wl_pointer::ButtonState::Pressed);
            pointer.frame();
            f.roundtrip(id);
            let grabbed = f
                .niri()
                .seat
                .get_pointer()
                .unwrap()
                .with_grab(|_, grab| {
                    let grab = grab.as_any();
                    grab.is::<MoveGrab>()
                        || grab.is::<ResizeGrab>()
                        || grab.is::<SpatialMovementGrab>()
                })
                .unwrap_or(false);
            assert_eq!(
                grabbed, expect_grab,
                "button {button}, app {app_id}, rules {rules}"
            );
            assert_eq!(f.niri().test_action_count, usize::from(expect_action));
            pointer.button(0, button, wl_pointer::ButtonState::Released);
            pointer.frame();
            f.roundtrip(id);
        }
    }
}

#[test]
fn axis_discrete_overflow() {
    let mut f = Fixture::new();
    let id = f.add_client();

    let client = f.client(id);
    let manager = client.state.virtual_pointer_manager.as_ref().unwrap();
    let pointer = manager.create_virtual_pointer(None, &client.qh, ());
    pointer.axis_discrete(0, wl_pointer::Axis::VerticalScroll, 0., i32::MAX);
    f.roundtrip(id);
}
