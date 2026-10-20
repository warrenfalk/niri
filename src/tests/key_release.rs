use std::io::{self, Read};

use smithay::backend::input::{InputEvent, InputTime, KeyState};
use smithay::input::keyboard::KeyboardSource;

use super::test_input_backend::{TestInputBackend, TestKeyboardKeyEvent};
use super::Fixture;

fn key(f: &mut Fixture, code: u32, state: KeyState) {
    f.niri_state()
        .process_input_event(InputEvent::<TestInputBackend>::Keyboard {
            event: TestKeyboardKeyEvent {
                time: InputTime::from_micros(0),
                code: code.into(),
                state,
                count: 1,
            },
        });
}

#[test]
fn key_release_waits_for_real_keyboard_state_in_either_release_order() {
    // XKB evdev codes for Super_L, D and an unrelated A key.
    for release_order in [[40, 133, 38], [133, 40, 38], [38, 133, 40]] {
        let mut f = Fixture::new();
        for code in [133, 40, 38] {
            key(&mut f, code, KeyState::Pressed);
        }
        let mut stream = f
            .niri()
            .key_release_bindings
            .start(KeyboardSource::MAIN, 40u32.into(), true)
            .unwrap()
            .unwrap();
        stream.set_nonblocking(true).unwrap();
        for code in &release_order[..2] {
            key(&mut f, *code, KeyState::Released);
            assert_eq!(
                stream.read(&mut [0]).unwrap_err().kind(),
                io::ErrorKind::WouldBlock
            );
        }
        key(&mut f, release_order[2], KeyState::Released);
        let mut line = String::new();
        stream.read_to_string(&mut line).unwrap();
        assert!(line.ends_with('\n'));
        assert!(line.trim().parse::<u128>().is_ok());
    }
}

#[test]
fn key_release_without_waiting_keeps_trigger_only_behavior() {
    let mut f = Fixture::new();
    key(&mut f, 133, KeyState::Pressed);
    key(&mut f, 40, KeyState::Pressed);
    let mut stream = f
        .niri()
        .key_release_bindings
        .start(KeyboardSource::MAIN, 40u32.into(), false)
        .unwrap()
        .unwrap();
    key(&mut f, 40, KeyState::Released);
    let mut line = String::new();
    stream.read_to_string(&mut line).unwrap();
    assert!(line.trim().parse::<u128>().is_ok());
}
