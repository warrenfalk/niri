use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use niri_ipc::{CommitTextRequest, PasteShortcut, PasteTextRequest, TextInputContext};
use smithay::reexports::wayland_protocols::wp::text_input::zv3::client::zwp_text_input_manager_v3::ZwpTextInputManagerV3;
use smithay::reexports::wayland_protocols::wp::text_input::zv3::client::zwp_text_input_v3::{
    self, ContentHint, ContentPurpose, ZwpTextInputV3,
};
use smithay::wayland::text_input::TextInputSeat;
use wayland_client::{Connection, Dispatch, QueueHandle};

use super::client::{ClientId, State as ClientState};
use super::Fixture;
use crate::text_context::STALE_CONTEXT;

#[derive(Default)]
struct Events {
    enters: AtomicU32,
    leaves: AtomicU32,
    done: AtomicU32,
    commits: Mutex<Vec<String>>,
    deletes: AtomicU32,
    preedits: AtomicU32,
}

impl Dispatch<ZwpTextInputManagerV3, ()> for ClientState {
    fn event(
        _: &mut Self,
        _: &ZwpTextInputManagerV3,
        _: <ZwpTextInputManagerV3 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpTextInputV3, Arc<Events>> for ClientState {
    fn event(
        _: &mut Self,
        _: &ZwpTextInputV3,
        event: zwp_text_input_v3::Event,
        events: &Arc<Events>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_text_input_v3::Event::CommitString { text } => {
                events
                    .commits
                    .lock()
                    .unwrap()
                    .push(text.unwrap_or_default());
            }
            zwp_text_input_v3::Event::DeleteSurroundingText { .. } => {
                events.deletes.fetch_add(1, Ordering::Relaxed);
            }
            zwp_text_input_v3::Event::PreeditString { .. } => {
                events.preedits.fetch_add(1, Ordering::Relaxed);
            }
            zwp_text_input_v3::Event::Enter { .. } => {
                events.enters.fetch_add(1, Ordering::Relaxed);
            }
            zwp_text_input_v3::Event::Leave { .. } => {
                events.leaves.fetch_add(1, Ordering::Relaxed);
            }
            zwp_text_input_v3::Event::Done { .. } => {
                events.done.fetch_add(1, Ordering::Relaxed);
            }
            _ => (),
        }
    }
}

fn window(f: &mut Fixture) -> (ClientId, u64) {
    let id = f.add_client();
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);
    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.set_size(300, 200);
    window.ack_last_and_commit();
    f.double_roundtrip(id);
    let window_id = f
        .niri()
        .layout
        .windows()
        .map(|(_, win)| win.id().get())
        .max()
        .unwrap();
    (id, window_id)
}

fn input(f: &mut Fixture, id: ClientId) -> (ZwpTextInputV3, Arc<Events>) {
    let client = f.client(id);
    let global = client
        .state
        .globals
        .iter()
        .find(|global| global.interface == "zwp_text_input_manager_v3")
        .unwrap();
    let manager: ZwpTextInputManagerV3 = client.registry.bind(global.name, 1, &client.qh, ());
    let seat = client.state.seats.keys().next().unwrap();
    let events = Arc::new(Events::default());
    let input = manager.get_text_input(seat, &client.qh, events.clone());
    f.roundtrip(id);
    (input, events)
}

fn read(
    f: &mut Fixture,
    window: u64,
    owner: &Rc<Cell<bool>>,
    focus: bool,
) -> async_channel::Receiver<Result<Option<TextInputContext>, String>> {
    let (tx, rx) = async_channel::bounded(1);
    f.niri_state()
        .read_text_context(Some(window), focus, owner, tx)
        .unwrap();
    rx
}

fn result(
    f: &mut Fixture,
    client: ClientId,
    rx: async_channel::Receiver<Result<Option<TextInputContext>, String>>,
) -> Option<TextInputContext> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        f.roundtrip(client);
        if let Ok(result) = rx.try_recv() {
            return result.unwrap();
        }
        assert!(Instant::now() < deadline, "context read timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn update(input: &ZwpTextInputV3, text: &str, cursor: i32, anchor: i32, enable: bool) {
    if enable {
        input.enable();
    }
    input.set_content_type(ContentHint::None, ContentPurpose::Normal);
    input.set_surrounding_text(text.to_owned(), cursor, anchor);
    input.commit();
}

#[test]
fn text_context_commits_are_scoped_guarded_and_connection_owned() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let (client, window) = window(&mut f);
    let (input, events) = input(&mut f, client);
    assert_eq!(events.enters.load(Ordering::Relaxed), 0);
    let owner = Rc::new(Cell::new(true));
    let rx = read(&mut f, window, &owner, false);
    f.roundtrip(client);
    assert!(events.enters.load(Ordering::Relaxed) > 0);
    update(&input, "Hi 🌍 there", 8, 8, true);
    let snapshot = result(&mut f, client, rx).unwrap();
    assert_eq!(snapshot.text, "Hi 🌍 there");
    assert_eq!(snapshot.cursor, 8);
    f.niri_state()
        .validate_text_context(window, snapshot.revision)
        .unwrap();
    assert!(events.done.load(Ordering::Relaxed) > 0);
    // Uncommitted state must not affect either the snapshot or its guard.
    input.set_surrounding_text("Not committed".into(), 0, 0);
    f.roundtrip(client);
    f.niri_state()
        .validate_text_context(window, snapshot.revision)
        .unwrap();
    input.commit();
    f.roundtrip(client);
    assert_eq!(
        f.niri_state()
            .validate_text_context(window, snapshot.revision)
            .unwrap_err(),
        STALE_CONTEXT
    );
    let (tx, _) = async_channel::bounded(1);
    let error = f
        .niri_state()
        .start_temporary_paste(
            PasteTextRequest {
                id: window,
                text: "new words".into(),
                shortcut: PasteShortcut::CtrlV,
                timeout_ms: 5000,
                context_revision: Some(snapshot.revision),
            },
            Rc::downgrade(&owner),
            tx,
        )
        .unwrap_err();
    assert_eq!(error, STALE_CONTEXT);
    assert!(f.niri().temporary_paste.is_none());
    let other = Rc::new(Cell::new(true));
    let (tx, _) = async_channel::bounded(1);
    assert!(f
        .niri_state()
        .read_text_context(Some(window), false, &other, tx)
        .is_err());
    owner.set(false);
    f.niri_state().refresh_text_context();
    assert!(f.niri().text_context.is_none());
    assert!(!f.niri().seat.text_input().compositor_input_method());
}

#[test]
fn text_context_supports_lazy_binding_and_resets_when_a_field_is_reenabled() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let (client, window) = window(&mut f);
    let owner = Rc::new(Cell::new(true));
    let rx = read(&mut f, window, &owner, false);
    let (input, events) = input(&mut f, client);
    assert!(events.enters.load(Ordering::Relaxed) > 0);
    update(&input, "Same text", 4, 4, true);
    let snapshot = result(&mut f, client, rx).unwrap();
    update(&input, "Same text", 4, 4, true);
    f.roundtrip(client);
    assert!(f
        .niri_state()
        .validate_text_context(window, snapshot.revision)
        .is_err());
    input.disable();
    input.commit();
    f.roundtrip(client);
    assert!(f.niri().seat.text_input().current_context().is_none());
}

#[test]
fn text_context_refuses_sensitive_or_invalid_surrounding_text() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let (client, window) = window(&mut f);
    let (input, _) = input(&mut f, client);
    let owner = Rc::new(Cell::new(true));
    for (text, cursor, hint, purpose) in [
        (
            "secret",
            0,
            ContentHint::SensitiveData,
            ContentPurpose::Normal,
        ),
        ("secret", 0, ContentHint::None, ContentPurpose::Password),
        ("🌍", 1, ContentHint::None, ContentPurpose::Normal),
    ] {
        let rx = read(&mut f, window, &owner, false);
        f.roundtrip(client);
        input.enable();
        input.set_content_type(hint, purpose);
        input.set_surrounding_text(text.into(), cursor, cursor);
        input.commit();
        assert!(result(&mut f, client, rx).is_none());
    }
}

#[test]
fn text_input_commit_is_guarded_single_use_and_never_deletes_or_creates_a_paste() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let (client, window) = window(&mut f);
    let (input, events) = input(&mut f, client);
    let owner = Rc::new(Cell::new(true));
    let rx = read(&mut f, window, &owner, false);
    f.roundtrip(client);
    update(&input, "Existing text", 13, 13, true);
    let snapshot = result(&mut f, client, rx).unwrap();
    let request = CommitTextRequest {
        id: window,
        text: " new 🌍 words".into(),
        context_revision: snapshot.revision,
    };
    f.niri_state().commit_text_context(request.clone()).unwrap();
    f.roundtrip(client);
    assert_eq!(*events.commits.lock().unwrap(), [" new 🌍 words"]);
    assert_eq!(events.deletes.load(Ordering::Relaxed), 0);
    assert_eq!(events.preedits.load(Ordering::Relaxed), 0);
    assert!(f.niri().temporary_paste.is_none());
    assert_eq!(
        f.niri_state().commit_text_context(request).unwrap_err(),
        STALE_CONTEXT
    );
    assert!(f
        .niri_state()
        .commit_text_context(CommitTextRequest {
            id: window,
            text: "x".repeat(4001),
            context_revision: snapshot.revision,
        })
        .is_err());
    f.roundtrip(client);
    assert_eq!(events.commits.lock().unwrap().len(), 1);
}
