//! Protocol regressions for private offers, independent of GUI widget behavior.

use std::cell::Cell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::AsFd as _;
use std::os::unix::net::UnixStream;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use niri_ipc::{PasteShortcut, PasteTextOutcome, PasteTextRequest, PasteTextResult};
use smithay::reexports::wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_offer_v1::{self, ExtDataControlOfferV1},
};
use smithay::reexports::wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
    zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1},
    zwlr_data_control_offer_v1::{self, ZwlrDataControlOfferV1},
};
use smithay::wayland::selection::data_device::data_device_selection_serial;
use wayland_client::protocol::{
    wl_data_device::{self, WlDataDevice},
    wl_data_device_manager::WlDataDeviceManager,
    wl_data_offer::{self, WlDataOffer},
    wl_data_source::{self, WlDataSource},
    wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, Proxy as _, QueueHandle};

use super::client::{Client, ClientId, State};
use super::Fixture;

#[derive(Default)]
pub struct TestClipboard {
    pub manager: Option<WlDataDeviceManager>,
    pub device: Option<WlDataDevice>,
    pub selection: Option<WlDataOffer>,
    pub offers: HashMap<WlDataOffer, Vec<String>>,
    pub selections: Vec<Vec<String>>,
    pub wlr: Option<ZwlrDataControlDeviceV1>,
    pub wlr_selection: Option<ZwlrDataControlOfferV1>,
    pub wlr_offers: HashMap<ZwlrDataControlOfferV1, Vec<String>>,
    pub wlr_selections: Vec<Vec<String>>,
    pub ext: Option<ExtDataControlDeviceV1>,
    pub ext_selection: Option<ExtDataControlOfferV1>,
    pub ext_offers: HashMap<ExtDataControlOfferV1, Vec<String>>,
    pub ext_selections: Vec<Vec<String>>,
}

struct SourceData {
    payloads: HashMap<String, Vec<u8>>,
    cancellations: Arc<AtomicU32>,
}

impl Client {
    fn bind_clipboard(&mut self, history: bool) {
        let seat = self.state.seats.keys().next().unwrap().clone();
        let global = self
            .state
            .globals
            .iter()
            .find(|g| g.interface == WlDataDeviceManager::interface().name)
            .unwrap();
        let manager: WlDataDeviceManager =
            self.registry
                .bind(global.name, global.version.min(3), &self.qh, ());
        self.state.clipboard.device = Some(manager.get_data_device(&seat, &self.qh, ()));
        self.state.clipboard.manager = Some(manager);
        if history {
            let global = self
                .state
                .globals
                .iter()
                .find(|g| g.interface == ZwlrDataControlManagerV1::interface().name)
                .unwrap();
            let manager: ZwlrDataControlManagerV1 =
                self.registry
                    .bind(global.name, global.version.min(2), &self.qh, ());
            self.state.clipboard.wlr = Some(manager.get_data_device(&seat, &self.qh, ()));
            let global = self
                .state
                .globals
                .iter()
                .find(|g| g.interface == ExtDataControlManagerV1::interface().name)
                .unwrap();
            let manager: ExtDataControlManagerV1 =
                self.registry
                    .bind(global.name, global.version.min(1), &self.qh, ());
            self.state.clipboard.ext = Some(manager.get_data_device(&seat, &self.qh, ()));
        }
        self.connection.flush().unwrap();
    }

    fn copy_non_text(&mut self, bytes: &[u8]) -> Arc<AtomicU32> {
        let cancellations = Arc::new(AtomicU32::new(0));
        let payloads = [
            ("image/png".to_owned(), bytes.to_vec()),
            (
                "application/x-niri-test".to_owned(),
                vec![0, 255, 128, 0, 1],
            ),
        ]
        .into_iter()
        .collect();
        let source = self
            .state
            .clipboard
            .manager
            .as_ref()
            .unwrap()
            .create_data_source(
                &self.qh,
                SourceData {
                    payloads,
                    cancellations: cancellations.clone(),
                },
            );
        source.offer("image/png".to_owned());
        source.offer("application/x-niri-test".to_owned());
        self.state
            .clipboard
            .device
            .as_ref()
            .unwrap()
            .set_selection(Some(&source), 0);
        self.connection.flush().unwrap();
        cancellations
    }
}

wayland_client::delegate_noop!(State: ignore WlDataDeviceManager);
wayland_client::delegate_noop!(State: ignore ZwlrDataControlManagerV1);
wayland_client::delegate_noop!(State: ignore ExtDataControlManagerV1);

impl Dispatch<WlDataDevice, ()> for State {
    wayland_client::event_created_child!(State, WlDataDevice, [0 => (WlDataOffer, ())]);
    fn event(
        state: &mut Self,
        _proxy: &WlDataDevice,
        event: wl_data_device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_device::Event::DataOffer { id } => {
                state.clipboard.offers.insert(id, Vec::new());
            }
            wl_data_device::Event::Selection { id } => {
                let mimes = id
                    .as_ref()
                    .map(|id| state.clipboard.offers[id].clone())
                    .unwrap_or_default();
                state.clipboard.selections.push(mimes);
                state.clipboard.selection = id;
            }
            _ => (),
        }
    }
}

impl Dispatch<WlDataOffer, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &WlDataOffer,
        event: wl_data_offer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_data_offer::Event::Offer { mime_type } = event {
            state
                .clipboard
                .offers
                .get_mut(proxy)
                .unwrap()
                .push(mime_type);
        }
    }
}

impl Dispatch<WlDataSource, SourceData> for State {
    fn event(
        _: &mut Self,
        _proxy: &WlDataSource,
        event: wl_data_source::Event,
        data: &SourceData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_source::Event::Send { mime_type, fd } => {
                File::from(fd)
                    .write_all(&data.payloads[&mime_type])
                    .unwrap();
            }
            wl_data_source::Event::Cancelled => {
                data.cancellations.fetch_add(1, Ordering::Relaxed);
            }
            _ => (),
        }
    }
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for State {
    wayland_client::event_created_child!(State, ZwlrDataControlDeviceV1, [0 => (ZwlrDataControlOfferV1, ())]);
    fn event(
        state: &mut Self,
        _: &ZwlrDataControlDeviceV1,
        event: zwlr_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_data_control_device_v1::Event::DataOffer { id } => {
                state.clipboard.wlr_offers.insert(id, Vec::new());
            }
            zwlr_data_control_device_v1::Event::Selection { id } => {
                state.clipboard.wlr_selections.push(
                    id.as_ref()
                        .map(|id| state.clipboard.wlr_offers[id].clone())
                        .unwrap_or_default(),
                );
                state.clipboard.wlr_selection = id;
            }
            _ => (),
        }
    }
}

impl Dispatch<ZwlrDataControlOfferV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &ZwlrDataControlOfferV1,
        event: zwlr_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_offer_v1::Event::Offer { mime_type } = event {
            state
                .clipboard
                .wlr_offers
                .get_mut(proxy)
                .unwrap()
                .push(mime_type);
        }
    }
}

impl Dispatch<ExtDataControlDeviceV1, ()> for State {
    wayland_client::event_created_child!(State, ExtDataControlDeviceV1, [0 => (ExtDataControlOfferV1, ())]);
    fn event(
        state: &mut Self,
        _: &ExtDataControlDeviceV1,
        event: ext_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_data_control_device_v1::Event::DataOffer { id } => {
                state.clipboard.ext_offers.insert(id, Vec::new());
            }
            ext_data_control_device_v1::Event::Selection { id } => {
                state.clipboard.ext_selections.push(
                    id.as_ref()
                        .map(|id| state.clipboard.ext_offers[id].clone())
                        .unwrap_or_default(),
                );
                state.clipboard.ext_selection = id;
            }
            _ => (),
        }
    }
}

impl Dispatch<ExtDataControlOfferV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &ExtDataControlOfferV1,
        event: ext_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_data_control_offer_v1::Event::Offer { mime_type } = event {
            state
                .clipboard
                .ext_offers
                .get_mut(proxy)
                .unwrap()
                .push(mime_type);
        }
    }
}

fn window(f: &mut Fixture) -> (ClientId, WlSurface, u64) {
    let client = f.add_client();
    f.client(client).bind_clipboard(false);
    let window = f.client(client).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.set_size(300, 200);
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let id = f
        .niri()
        .layout
        .windows()
        .map(|(_, win)| win.id().get())
        .max()
        .unwrap();
    (client, surface, id)
}

fn setup() -> (Fixture, ClientId, ClientId, u64, Arc<AtomicU32>) {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let (source, _, _) = window(&mut f);
    let cancellations = f
        .client(source)
        .copy_non_text(b"\x89PNG\r\n\x1a\noriginal binary clipboard\0\xff");
    f.double_roundtrip(source);
    let (target, _, id) = window(&mut f);
    (f, source, target, id, cancellations)
}

fn begin(
    f: &mut Fixture,
    id: u64,
    text: &str,
    shortcut: PasteShortcut,
    timeout_ms: u32,
) -> (
    Rc<Cell<bool>>,
    async_channel::Receiver<Result<PasteTextResult, String>>,
) {
    let owner = Rc::new(Cell::new(true));
    let (tx, rx) = async_channel::bounded(1);
    f.niri_state()
        .start_temporary_paste(
            PasteTextRequest {
                id,
                text: text.to_owned(),
                shortcut,
                timeout_ms,
                context_revision: None,
            },
            Rc::downgrade(&owner),
            tx,
        )
        .unwrap();
    (owner, rx)
}

fn receive(f: &mut Fixture, client: ClientId, offer: &WlDataOffer, mime: &str) -> Vec<u8> {
    let (mut read, write) = UnixStream::pair().unwrap();
    read.set_nonblocking(true).unwrap();
    offer.receive(mime.to_owned(), write.as_fd());
    f.client(client).connection.flush().unwrap();
    drop(write);
    let mut result = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        f.dispatch();
        let mut buf = [0; 4096];
        match read.read(&mut buf) {
            Ok(0) => return result,
            Ok(n) => result.extend_from_slice(&buf[..n]),
            Err(err) if err.kind() == ErrorKind::WouldBlock => (),
            err => panic!("read failed: {err:?}"),
        }
        assert!(Instant::now() < deadline);
    }
}

fn finish(
    f: &mut Fixture,
    rx: &async_channel::Receiver<Result<PasteTextResult, String>>,
) -> PasteTextResult {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        // The fixture nests the compositor event loop; its timers need an explicit
        // dispatch when there is no Wayland traffic to wake the outer fixture loop.
        f.state.server.dispatch();
        f.dispatch();
        if let Ok(result) = rx.try_recv() {
            return result.unwrap();
        }
        assert!(Instant::now() < deadline);
    }
}

#[test]
fn private_paste_preserves_binary_source_and_both_history_protocols() {
    let (mut f, source, target, id, cancellations) = setup();
    let history = f.add_client();
    f.client(history).bind_clipboard(true);
    f.double_roundtrip(history);
    let serial = data_device_selection_serial(&f.niri().seat);
    let initial_wlr = f.client(history).state.clipboard.wlr_selections.clone();
    let initial_ext = f.client(history).state.clipboard.ext_selections.clone();
    let text = "Hello 🌍\nΚαλημέρα 世界\n".repeat(8192);
    let (_owner, rx) = begin(&mut f, id, &text, PasteShortcut::CtrlShiftV, 5000);
    f.double_roundtrip(target);
    let private = f.client(target).state.clipboard.selection.clone().unwrap();
    assert!(f.client(target).state.clipboard.offers[&private]
        .contains(&"text/plain;charset=utf-8".to_owned()));
    // A history client joining DURING the operation must also get the untouched real source.
    let joining = f.add_client();
    f.client(joining).bind_clipboard(true);
    f.double_roundtrip(joining);
    for history in [history, joining] {
        let cb = &f.client(history).state.clipboard;
        assert!(cb
            .wlr_selections
            .iter()
            .all(|m| m == &vec!["image/png", "application/x-niri-test"]));
        assert!(cb
            .ext_selections
            .iter()
            .all(|m| m == &vec!["image/png", "application/x-niri-test"]));
    }
    assert_eq!(
        receive(&mut f, target, &private, "text/plain;charset=utf-8"),
        text.as_bytes()
    );
    f.double_roundtrip(target);
    // The selection event is already real again; the retained private object can still
    // serve another MIME for the same paste, including another asynchronous large transfer.
    let real = f.client(target).state.clipboard.selection.clone().unwrap();
    assert_ne!(real, private);
    assert_eq!(
        receive(&mut f, target, &private, "text/plain"),
        text.as_bytes()
    );
    private.destroy();
    f.double_roundtrip(target);
    let result = finish(&mut f, &rx);
    assert_eq!(result.outcome, PasteTextOutcome::DataServed);
    assert_eq!(result.requests, 2);
    assert_eq!(result.transfers_completed, 2);
    assert_eq!(result.bytes_written, (text.len() * 2) as u64);
    assert_eq!(data_device_selection_serial(&f.niri().seat), serial);
    assert_eq!(
        receive(&mut f, target, &real, "application/x-niri-test"),
        vec![0, 255, 128, 0, 1]
    );
    assert_eq!(
        receive(&mut f, target, &real, "image/png"),
        b"\x89PNG\r\n\x1a\noriginal binary clipboard\0\xff"
    );
    f.double_roundtrip(source);
    f.double_roundtrip(history);
    assert_eq!(cancellations.load(Ordering::Relaxed), 0);
    assert_eq!(
        f.client(history).state.clipboard.wlr_selections,
        initial_wlr
    );
    assert_eq!(
        f.client(history).state.clipboard.ext_selections,
        initial_ext
    );
}

#[test]
fn private_paste_cancels_for_focus_close_disconnect_copy_and_timeout() {
    for reason in [
        PasteTextOutcome::FocusChanged,
        PasteTextOutcome::DestinationClosed,
        PasteTextOutcome::Cancelled,
        PasteTextOutcome::ClipboardChanged,
        PasteTextOutcome::TimedOut,
    ] {
        let (mut f, source, target, id, cancellations) = setup();
        let (owner, rx) = begin(&mut f, id, "private secret", PasteShortcut::CtrlV, 100);
        f.double_roundtrip(target);
        let private = f.client(target).state.clipboard.selection.clone().unwrap();
        match reason {
            PasteTextOutcome::FocusChanged => {
                let other = f
                    .niri()
                    .layout
                    .windows()
                    .find(|(_, win)| win.id().get() != id)
                    .unwrap()
                    .1
                    .window
                    .clone();
                f.niri().layout.activate_window(&other);
            }
            PasteTextOutcome::DestinationClosed => {
                let window = &f.client(target).state.windows[0];
                window.xdg_toplevel.destroy();
                window.xdg_surface.destroy();
                window.surface.destroy();
                f.double_roundtrip(target);
            }
            PasteTextOutcome::Cancelled => {
                owner.set(false);
            }
            PasteTextOutcome::ClipboardChanged => {
                f.client(target).copy_non_text(b"concurrent new clipboard");
            }
            PasteTextOutcome::TimedOut => (),
            _ => unreachable!(),
        }
        let result = finish(&mut f, &rx);
        assert_eq!(result.outcome, reason);
        assert_eq!(result.bytes_written, 0);
        if reason != PasteTextOutcome::DestinationClosed {
            assert!(receive(&mut f, target, &private, "text/plain").is_empty());
        }
        f.double_roundtrip(source);
        assert_eq!(
            cancellations.load(Ordering::Relaxed),
            u32::from(reason == PasteTextOutcome::ClipboardChanged)
        );
        assert!(f.niri().temporary_paste.is_none());
        if reason == PasteTextOutcome::ClipboardChanged {
            let real = f.client(target).state.clipboard.selection.clone().unwrap();
            assert_eq!(
                receive(&mut f, target, &real, "image/png"),
                b"concurrent new clipboard"
            );
        }
    }
}

#[test]
fn private_paste_revokes_blocked_transfers_and_stale_offers() {
    for timeout in [false, true] {
        let (mut f, source, target, id, cancellations) = setup();
        let text = "x".repeat(1024 * 1024);
        let (owner, rx) = begin(
            &mut f,
            id,
            &text,
            PasteShortcut::CtrlV,
            if timeout { 100 } else { 5000 },
        );
        f.double_roundtrip(target);
        let private = f.client(target).state.clipboard.selection.clone().unwrap();
        let (mut read, write) = UnixStream::pair().unwrap();
        read.set_nonblocking(true).unwrap();
        private.receive("text/plain".to_owned(), write.as_fd());
        drop(write);
        f.double_roundtrip(target);
        f.state.server.dispatch();
        // Leave the read end undrained, forcing backpressure and an incomplete transfer.
        if !timeout {
            owner.set(false);
        }
        let result = finish(&mut f, &rx);
        assert_eq!(
            result.outcome,
            if timeout {
                PasteTextOutcome::TimedOut
            } else {
                PasteTextOutcome::Cancelled
            }
        );
        assert_eq!(result.transfers_completed, 0);
        assert!(result.bytes_written > 0);
        assert!(result.bytes_written < text.len() as u64);
        let mut partial = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let mut buf = [0; 4096];
            match read.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => partial.extend_from_slice(&buf[..n]),
                Err(err) if err.kind() == ErrorKind::WouldBlock => {
                    f.state.server.dispatch();
                    f.dispatch();
                    assert!(
                        Instant::now() < deadline,
                        "cancelled transfer did not close"
                    );
                }
                err => panic!("read failed: {err:?}"),
            }
        }
        assert_eq!(partial.len() as u64, result.bytes_written);
        assert!(receive(&mut f, target, &private, "text/plain").is_empty());
        f.double_roundtrip(source);
        assert_eq!(cancellations.load(Ordering::Relaxed), 0);
    }
}

#[test]
fn private_paste_handles_dismissed_offer_and_broken_pipe() {
    for broken_pipe in [false, true] {
        let (mut f, _, target, id, cancellations) = setup();
        let (_owner, rx) = begin(&mut f, id, "private text", PasteShortcut::CtrlV, 5000);
        f.double_roundtrip(target);
        let private = f.client(target).state.clipboard.selection.clone().unwrap();
        if broken_pipe {
            let (read, write) = UnixStream::pair().unwrap();
            drop(read);
            private.receive("text/plain".to_owned(), write.as_fd());
        } else {
            private.destroy();
        }
        f.double_roundtrip(target);
        let result = finish(&mut f, &rx);
        assert_eq!(
            result.outcome,
            if broken_pipe {
                PasteTextOutcome::TransferFailed
            } else {
                PasteTextOutcome::OfferDismissed
            }
        );
        assert_eq!(result.transfers_completed, 0);
        assert_eq!(result.bytes_written, 0);
        assert_eq!(cancellations.load(Ordering::Relaxed), 0);
    }
}

#[test]
fn private_paste_preflight_errors_leave_selection_and_focus_untouched() {
    let (mut f, source, target, id, cancellations) = setup();
    let serial = data_device_selection_serial(&f.niri().seat);
    let focus = f.niri().seat.get_keyboard().unwrap().current_focus();
    let owner = Rc::new(Cell::new(true));
    for (id, text, timeout_ms) in [
        (id, String::new(), 5000),
        (id, "x".repeat(1024 * 1024 + 1), 5000),
        (id, String::from("text"), 0),
        (u64::MAX, String::from("text"), 5000),
    ] {
        let (tx, _) = async_channel::bounded(1);
        assert!(f
            .niri_state()
            .start_temporary_paste(
                PasteTextRequest {
                    id,
                    text,
                    timeout_ms,
                    shortcut: PasteShortcut::CtrlV,
                    context_revision: None,
                },
                Rc::downgrade(&owner),
                tx
            )
            .is_err());
        assert!(f.niri().temporary_paste.is_none());
        assert_eq!(data_device_selection_serial(&f.niri().seat), serial);
        assert_eq!(f.niri().seat.get_keyboard().unwrap().current_focus(), focus);
    }
    f.client(target)
        .state
        .clipboard
        .device
        .as_ref()
        .unwrap()
        .release();
    f.double_roundtrip(target);
    let (tx, _) = async_channel::bounded(1);
    let error = f
        .niri_state()
        .start_temporary_paste(
            PasteTextRequest {
                id,
                text: String::from("text"),
                timeout_ms: 5000,
                shortcut: PasteShortcut::CtrlV,
                context_revision: None,
            },
            Rc::downgrade(&owner),
            tx,
        )
        .unwrap_err();
    assert!(error.starts_with("unsupported destination:"));
    assert_eq!(data_device_selection_serial(&f.niri().seat), serial);
    f.double_roundtrip(source);
    assert_eq!(cancellations.load(Ordering::Relaxed), 0);
}
