//! Binding-owned stdin streams: a release line means release; EOF means cancellation.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::io::{self, Write};
use std::os::unix::net::UnixStream;
use std::time::Instant;

use smithay::backend::input::Keycode;
use smithay::input::keyboard::KeyboardSource;

#[derive(Default)]
pub struct KeyReleaseBindings {
    held: HashMap<(KeyboardSource, Keycode), (Instant, UnixStream)>,
}

impl KeyReleaseBindings {
    pub fn start(
        &mut self,
        source: KeyboardSource,
        key: Keycode,
    ) -> io::Result<Option<UnixStream>> {
        let Entry::Vacant(entry) = self.held.entry((source, key)) else {
            // Device autorepeat must not create additional children.
            return Ok(None);
        };
        let (writer, reader) = UnixStream::pair()?;
        writer.set_nonblocking(true)?;
        entry.insert((Instant::now(), writer));
        Ok(Some(reader))
    }

    pub fn release(&mut self, source: KeyboardSource, key: Keycode) {
        if let Some((started, mut writer)) = self.held.remove(&(source, key)) {
            // A fresh socket has room for this one short line. Never block the
            // compositor, even if the child failed to start or stopped reading.
            let _ = writeln!(writer, "{}", started.elapsed().as_millis());
        }
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
        let mut input = bindings.start(source, key).unwrap().unwrap();
        assert!(bindings.start(source, key).unwrap().is_none());
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
                .start(KeyboardSource::MAIN, Keycode::from(40u32))
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
}
