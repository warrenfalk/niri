//! Binding-owned stdin streams: a release line means release; EOF means cancellation.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::io::{self, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use smithay::backend::input::Keycode;
use smithay::input::keyboard::KeyboardSource;

#[derive(Default)]
pub struct KeyReleaseBindings {
    held: HashMap<(KeyboardSource, Keycode), Hold>,
}

struct Hold {
    started: Instant,
    released_after: Option<Duration>,
    wait_for_all_keys: bool,
    writer: UnixStream,
}

impl Hold {
    fn send_release(&mut self) {
        // A fresh socket has room for this one short line. Never block the
        // compositor, even if the child failed to start or stopped reading.
        let _ = writeln!(self.writer, "{}", self.released_after.unwrap().as_millis());
    }
}

impl KeyReleaseBindings {
    pub fn start(
        &mut self,
        source: KeyboardSource,
        key: Keycode,
        wait_for_all_keys: bool,
    ) -> io::Result<Option<UnixStream>> {
        let Entry::Vacant(entry) = self.held.entry((source, key)) else {
            // Device autorepeat must not create additional children.
            return Ok(None);
        };
        let (writer, reader) = UnixStream::pair()?;
        writer.set_nonblocking(true)?;
        entry.insert(Hold {
            started: Instant::now(),
            released_after: None,
            wait_for_all_keys,
            writer,
        });
        Ok(Some(reader))
    }

    pub fn release(&mut self, source: KeyboardSource, key: Keycode) {
        if let Entry::Occupied(mut entry) = self.held.entry((source, key)) {
            let hold = entry.get_mut();
            // Freeze the trigger's duration even if modifiers stay held, or
            // the trigger is pressed and released again before the seat idles.
            hold.released_after
                .get_or_insert_with(|| hold.started.elapsed());
            if !hold.wait_for_all_keys {
                entry.remove().send_release();
            }
        }
    }

    pub fn finish_if_idle(&mut self, idle: bool) {
        if !idle {
            return;
        }
        self.held.retain(|_, hold| {
            if hold.released_after.is_none() {
                return true;
            }
            hold.send_release();
            false
        });
    }

    pub fn cancel_source(&mut self, source: KeyboardSource) {
        self.held
            .retain(|(held_source, _), _| *held_source != source);
    }

    pub fn cancel_all(&mut self) {
        self.held.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    #[test]
    fn release_follows_trigger_key_and_source_not_modifiers() {
        let mut bindings = KeyReleaseBindings::default();
        let source = KeyboardSource::MAIN;
        let key = Keycode::from(40u32);
        let mut input = bindings.start(source, key, false).unwrap().unwrap();
        assert!(bindings.start(source, key, false).unwrap().is_none());
        bindings.release(source, Keycode::from(133u32));
        bindings.release(KeyboardSource::new_auxiliary(), key);
        assert_eq!(bindings.held.len(), 1);
        bindings.release(source, key);
        let mut line = String::new();
        input.read_to_string(&mut line).unwrap();
        assert!(line.ends_with('\n'));
        assert!(line.trim().parse::<u128>().is_ok());
        assert!(bindings.held.is_empty());
    }

    #[test]
    fn cancellation_is_eof_without_a_release_line() {
        for all in [false, true] {
            let mut bindings = KeyReleaseBindings::default();
            let mut input = bindings
                .start(KeyboardSource::MAIN, Keycode::from(40u32), false)
                .unwrap()
                .unwrap();
            if all {
                bindings.cancel_all();
            } else {
                bindings.cancel_source(KeyboardSource::MAIN);
            }
            let mut line = String::new();
            input.read_to_string(&mut line).unwrap();
            assert_eq!(line, "");
        }
    }

    #[test]
    fn waits_for_idle_after_trigger_release_without_changing_duration() {
        let mut bindings = KeyReleaseBindings::default();
        let source = KeyboardSource::MAIN;
        let key = Keycode::from(40u32);
        let mut input = bindings.start(source, key, true).unwrap().unwrap();
        input.set_nonblocking(true).unwrap();
        bindings.release(source, key);
        let duration = bindings.held[&(source, key)].released_after.unwrap();
        bindings.finish_if_idle(false);
        assert_eq!(
            input.read(&mut [0]).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        // Waiting for Mod must not turn a tap into a hold. Re-pressing the
        // trigger while waiting also cannot create a second child or duration.
        bindings.held.get_mut(&(source, key)).unwrap().started -= Duration::from_secs(3);
        assert!(bindings.start(source, key, true).unwrap().is_none());
        bindings.release(source, key);
        bindings.release(source, Keycode::from(133u32));
        bindings.finish_if_idle(true);
        let mut line = String::new();
        input.read_to_string(&mut line).unwrap();
        assert_eq!(line, format!("{}\n", duration.as_millis()));
        assert!(bindings.held.is_empty());
    }

    #[test]
    fn modifier_first_release_still_waits_for_the_trigger() {
        let mut bindings = KeyReleaseBindings::default();
        let source = KeyboardSource::MAIN;
        let key = Keycode::from(40u32);
        let mut input = bindings.start(source, key, true).unwrap().unwrap();
        bindings.release(source, Keycode::from(133u32));
        bindings.finish_if_idle(false);
        assert!(bindings.held[&(source, key)].released_after.is_none());
        // Another source releasing D must not complete this source's hold.
        bindings.release(KeyboardSource::new_auxiliary(), key);
        bindings.finish_if_idle(true);
        assert_eq!(bindings.held.len(), 1);
        bindings.release(source, key);
        bindings.finish_if_idle(true);
        let mut line = String::new();
        input.read_to_string(&mut line).unwrap();
        assert!(line.trim().parse::<u128>().is_ok());
    }

    #[test]
    fn cancellation_while_waiting_for_modifiers_never_sends() {
        for all in [false, true] {
            let mut bindings = KeyReleaseBindings::default();
            let source = KeyboardSource::MAIN;
            let key = Keycode::from(40u32);
            let mut input = bindings.start(source, key, true).unwrap().unwrap();
            bindings.release(source, key);
            bindings.finish_if_idle(false);
            if all {
                bindings.cancel_all();
            } else {
                bindings.cancel_source(source);
            }
            bindings.finish_if_idle(true);
            let mut line = String::new();
            input.read_to_string(&mut line).unwrap();
            assert_eq!(line, "");
        }
    }
}
