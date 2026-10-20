//! Private regular-data-device offers. The real seat selection is never replaced.

use std::cell::Cell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{ErrorKind, Write as _};
use std::os::fd::OwnedFd;
use std::rc::Weak;
use std::sync::Arc;
use std::time::Duration;

use async_channel::Sender;
use calloop::generic::Generic;
use calloop::timer::{TimeoutAction, Timer};
use calloop::{Interest, Mode, PostAction, RegistrationToken};
use niri_ipc::{PasteShortcut, PasteTextOutcome, PasteTextRequest, PasteTextResult};
use smithay::backend::input::{InputTime, KeyState};
use smithay::input::keyboard::{keysyms, FilterResult, KeyboardSource, Keysym};
use smithay::reexports::rustix::fs::{fcntl_getfl, fcntl_setfl, OFlags};
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_data_offer::{self, WlDataOffer};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, DataInit, DisplayHandle, Resource};
use smithay::utils::SERIAL_COUNTER;
use smithay::wayland::selection::data_device::{
    client_data_devices, data_device_selection_serial, resend_data_device_selection,
};
use smithay::wayland::Dispatch2;

use crate::niri::{ClientState, State};
use crate::utils::id::IdCounter;

const MIME_TYPES: &[&str] = &["text/plain;charset=utf-8", "text/plain", "UTF8_STRING"];
const MAX_REQUESTS: u32 = 32;
const MAX_TEXT_BYTES: usize = 1024 * 1024;
static IDS: IdCounter = IdCounter::new();

pub struct TemporaryPaste {
    id: u64,
    window: u64,
    surface: WlSurface,
    owner: Weak<Cell<bool>>,
    selection_serial: u64,
    text: Arc<[u8]>,
    offers: Vec<WlDataOffer>,
    transfers: HashMap<u64, RegistrationToken>,
    timer: RegistrationToken,
    restored: bool,
    requests: u32,
    completed: u32,
    bytes_written: u64,
    reply: Sender<Result<PasteTextResult, String>>,
}

// Offers carry an opaque operation id, never the transcript. Revocation drops the only
// persistent text buffer; stale Wayland objects cannot access it after cleanup.
pub struct PrivateOfferData {
    operation: u64,
}

fn validate_request(request: &PasteTextRequest) -> Result<(), String> {
    if request.text.is_empty() || request.text.len() > MAX_TEXT_BYTES {
        return Err(String::from(
            "paste text must contain 1 to 1048576 UTF-8 bytes",
        ));
    }
    if !(100..=30_000).contains(&request.timeout_ms) {
        return Err(String::from(
            "paste timeout_ms must be between 100 and 30000",
        ));
    }
    Ok(())
}

impl State {
    pub fn start_temporary_paste(
        &mut self,
        request: PasteTextRequest,
        owner: Weak<Cell<bool>>,
        reply: Sender<Result<PasteTextResult, String>>,
    ) -> Result<(), String> {
        validate_request(&request)?;
        self.refresh_temporary_paste();
        if self.niri.temporary_paste.is_some() {
            return Err(String::from("another temporary paste is active"));
        }
        if !owner.upgrade().is_some_and(|owner| owner.get()) {
            return Err(String::from("paste IPC connection has closed"));
        }
        let keyboard = self
            .niri
            .seat
            .get_keyboard()
            .ok_or("seat has no keyboard")?;
        let mods = keyboard.modifier_state();
        if self.niri.is_locked()
            || !self.niri.keyboard_focus.is_layout()
            || keyboard.is_grabbed()
            || !keyboard.pressed_keys().is_empty()
            || mods.ctrl
            || mods.shift
            || mods.alt
            || mods.logo
            || mods.iso_level3_shift
            || mods.iso_level5_shift
            || mods.serialized.depressed != 0
            || mods.serialized.latched != 0
        {
            return Err(String::from(
                "paste requires an unlocked layout with no held keys or keyboard grab",
            ));
        }
        if let Some(revision) = request.context_revision {
            self.validate_text_context(request.id, revision)?;
        }
        let (window, surface, credentials) = self
            .niri
            .layout
            .windows()
            .find(|(_, win)| win.id().get() == request.id)
            .and_then(|(output, win)| {
                output.map(|_| {
                    (
                        win.window.clone(),
                        win.toplevel().wl_surface().clone(),
                        win.credentials().cloned(),
                    )
                })
            })
            .ok_or("destination window is missing or has no output")?;
        let client = surface.client().ok_or("destination client has closed")?;
        if client
            .get_data::<ClientState>()
            .is_none_or(|data| data.credentials_unknown)
        {
            return Err(String::from(
                "unsupported destination: unknown client credentials",
            ));
        }
        let credentials = credentials.ok_or("unsupported destination: unknown process")?;
        let exe = std::fs::read_link(format!("/proc/{}/exe", credentials.pid))
            .map_err(|_| String::from("unsupported destination: cannot identify native client"))?;
        // A satellite presents X11 windows as xdg toplevels, but forwards regular offers to
        // the shared X11 CLIPBOARD. Refuse the bridge, including externally launched ones.
        if exe
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains("xwayland"))
        {
            return Err(String::from(
                "unsupported destination: Xwayland clipboard bridges are not private",
            ));
        }
        let devices = client_data_devices(&self.niri.seat, &client);
        if devices.is_empty() || keyboard.client_keyboards(&client).next().is_none() {
            return Err(String::from(
                "unsupported destination: no regular Wayland clipboard/keyboard device",
            ));
        }
        let mut keycode = |sym| {
            let symbol = Keysym::new(sym);
            let code = keyboard.keycode_for_keysym(symbol).ok_or_else(|| {
                String::from(
                    "unsupported destination: paste keys are unavailable in the active layout",
                )
            })?;
            let unshifted = keyboard.with_xkb_state(self, |context| {
                let xkb = context.xkb().lock().unwrap();
                xkb.raw_syms_for_key_in_layout(code, xkb.active_layout())
                    .contains(&symbol)
            });
            if !unshifted {
                return Err(String::from(
                    "unsupported destination: paste keys require a different layout level",
                ));
            }
            Ok(code)
        };
        let control = keycode(keysyms::KEY_Control_L)?;
        let shift = if request.shortcut == PasteShortcut::CtrlShiftV {
            Some(keycode(keysyms::KEY_Shift_L)?)
        } else {
            None
        };
        let v = keycode(keysyms::KEY_v)?;

        // Focus and offer creation happen in one compositor turn. Reject exclusive layers,
        // popup grabs, etc., rather than delivering a shortcut to a different surface.
        self.niri.layout.activate_window(&window);
        self.update_keyboard_focus();
        if keyboard.current_focus().as_ref() != Some(&surface) || keyboard.is_grabbed() {
            return Err(String::from("destination could not receive keyboard focus"));
        }

        let operation = IDS.next();
        let timer = self
            .niri
            .event_loop
            .insert_source(
                Timer::from_duration(Duration::from_millis(u64::from(request.timeout_ms))),
                move |_, _, state| {
                    if state
                        .niri
                        .temporary_paste
                        .as_ref()
                        .is_some_and(|paste| paste.id == operation)
                    {
                        state.finish_temporary_paste(PasteTextOutcome::TimedOut);
                    }
                    TimeoutAction::Drop
                },
            )
            .map_err(|_| String::from("could not schedule paste cleanup"))?;
        let mut offers = Vec::new();
        for device in devices {
            let offer = match client.create_resource::<WlDataOffer, _, State>(
                &self.niri.display_handle,
                device.version(),
                PrivateOfferData { operation },
            ) {
                Ok(offer) => offer,
                Err(_) => {
                    self.niri.event_loop.remove(timer);
                    resend_data_device_selection::<State>(
                        &self.niri.display_handle,
                        &self.niri.seat,
                    );
                    return Err(String::from("could not create private clipboard offer"));
                }
            };
            device.data_offer(&offer);
            for mime in MIME_TYPES {
                offer.offer((*mime).to_owned());
            }
            device.selection(Some(&offer));
            offers.push(offer);
        }
        self.niri.temporary_paste = Some(TemporaryPaste {
            id: operation,
            window: request.id,
            surface,
            owner,
            selection_serial: data_device_selection_serial(&self.niri.seat),
            text: Arc::from(request.text.into_bytes()),
            offers,
            transfers: HashMap::new(),
            timer,
            restored: false,
            requests: 0,
            completed: 0,
            bytes_written: 0,
            reply,
        });

        let source = KeyboardSource::new_auxiliary();
        let key = |state: &mut State, code, pressed| {
            keyboard.input_from_source(
                source,
                state,
                code,
                pressed,
                SERIAL_COUNTER.next_serial(),
                InputTime::now(),
                |_, _, _| FilterResult::<()>::Forward,
            );
        };
        key(self, control, KeyState::Pressed);
        if let Some(shift) = shift {
            key(self, shift, KeyState::Pressed);
        }
        key(self, v, KeyState::Pressed);
        key(self, v, KeyState::Released);
        if let Some(shift) = shift {
            key(self, shift, KeyState::Released);
        }
        key(self, control, KeyState::Released);
        keyboard.release_source(self, source);
        self.niri.queue_redraw_all();
        Ok(())
    }

    fn paste_cancel_reason(&self) -> Option<PasteTextOutcome> {
        let paste = self.niri.temporary_paste.as_ref()?;
        if !paste.owner.upgrade().is_some_and(|owner| owner.get()) {
            return Some(PasteTextOutcome::Cancelled);
        }
        if !paste.surface.is_alive()
            || !self
                .niri
                .layout
                .windows()
                .any(|(_, win)| win.id().get() == paste.window)
        {
            return Some(PasteTextOutcome::DestinationClosed);
        }
        let Some(keyboard) = self.niri.seat.get_keyboard() else {
            return Some(PasteTextOutcome::FocusChanged);
        };
        if self.niri.is_locked()
            || keyboard.is_grabbed()
            || keyboard.current_focus().as_ref() != Some(&paste.surface)
        {
            return Some(PasteTextOutcome::FocusChanged);
        }
        if data_device_selection_serial(&self.niri.seat) != paste.selection_serial {
            return Some(PasteTextOutcome::ClipboardChanged);
        }
        None
    }

    pub fn refresh_temporary_paste(&mut self) {
        if let Some(reason) = self.paste_cancel_reason() {
            self.finish_temporary_paste(reason);
            return;
        }
        let Some(paste) = &self.niri.temporary_paste else {
            return;
        };
        if paste.offers.iter().all(|offer| !offer.is_alive()) && paste.transfers.is_empty() {
            let outcome = if paste.completed > 0 {
                PasteTextOutcome::DataServed
            } else {
                PasteTextOutcome::OfferDismissed
            };
            self.finish_temporary_paste(outcome);
        }
    }

    fn restore_paste_selection(&mut self) {
        let Some(paste) = self.niri.temporary_paste.as_mut() else {
            return;
        };
        if !paste.restored {
            paste.restored = true;
            resend_data_device_selection::<State>(&self.niri.display_handle, &self.niri.seat);
        }
    }

    pub fn finish_temporary_paste(&mut self, outcome: PasteTextOutcome) {
        let Some(paste) = self.niri.temporary_paste.take() else {
            return;
        };
        // Always offer the CURRENT real selection, including copies made during the operation.
        // Taking the operation first revokes all stale private offers before sending anything.
        resend_data_device_selection::<State>(&self.niri.display_handle, &self.niri.seat);
        self.niri.event_loop.remove(paste.timer);
        for (_, token) in paste.transfers {
            self.niri.event_loop.remove(token);
        }
        let _ = paste.reply.try_send(Ok(PasteTextResult {
            id: paste.window,
            outcome,
            requests: paste.requests,
            transfers_completed: paste.completed,
            bytes_written: paste.bytes_written,
        }));
    }

    fn receive_private_paste(&mut self, operation: u64, mime: String, fd: OwnedFd) {
        self.refresh_temporary_paste();
        let Some(paste) = self
            .niri
            .temporary_paste
            .as_mut()
            .filter(|paste| paste.id == operation)
        else {
            return;
        };
        if !MIME_TYPES.contains(&mime.as_str()) || paste.requests >= MAX_REQUESTS {
            self.finish_temporary_paste(PasteTextOutcome::TransferFailed);
            return;
        }
        let Ok(flags) = fcntl_getfl(&fd) else {
            self.finish_temporary_paste(PasteTextOutcome::TransferFailed);
            return;
        };
        if fcntl_setfl(&fd, flags | OFlags::NONBLOCK).is_err() {
            self.finish_temporary_paste(PasteTextOutcome::TransferFailed);
            return;
        }
        paste.requests += 1;
        let text = paste.text.clone();
        let transfer = IDS.next();
        let mut offset = 0;
        let token = self.niri.event_loop.insert_source(
            Generic::new(File::from(fd), Interest::WRITE, Mode::Level),
            move |_, file, state| {
                if let Some(reason) = state.paste_cancel_reason() {
                    if let Some(paste) = state.niri.temporary_paste.as_mut() {
                        paste.transfers.remove(&transfer);
                    }
                    state.finish_temporary_paste(reason);
                    return Ok(PostAction::Remove);
                }
                let Some(paste) = state
                    .niri
                    .temporary_paste
                    .as_mut()
                    .filter(|paste| paste.id == operation)
                else {
                    return Ok(PostAction::Remove);
                };
                let mut file: &File = file;
                while offset < text.len() {
                    match file.write(&text[offset..]) {
                        Ok(0) => break,
                        Ok(n) => {
                            offset += n;
                            paste.bytes_written += n as u64;
                        }
                        Err(err) if err.kind() == ErrorKind::Interrupted => continue,
                        Err(err) if err.kind() == ErrorKind::WouldBlock => {
                            return Ok(PostAction::Continue)
                        }
                        Err(_) => break,
                    }
                }
                paste.transfers.remove(&transfer);
                if offset == text.len() {
                    paste.completed += 1;
                    state.refresh_temporary_paste();
                } else {
                    state.finish_temporary_paste(PasteTextOutcome::TransferFailed);
                }
                Ok(PostAction::Remove)
            },
        );
        match token {
            Ok(token) => {
                self.niri
                    .temporary_paste
                    .as_mut()
                    .unwrap()
                    .transfers
                    .insert(transfer, token);
            }
            Err(_) => {
                self.finish_temporary_paste(PasteTextOutcome::TransferFailed);
                return;
            }
        }
        // Restore after a real receive request, not an arbitrary delay. Existing private
        // offer objects remain usable for additional MIME requests until destroyed or revoked.
        self.restore_paste_selection();
    }
}

impl Dispatch2<WlDataOffer, State> for PrivateOfferData {
    fn request(
        &self,
        state: &mut State,
        _client: &Client,
        _resource: &WlDataOffer,
        request: wl_data_offer::Request,
        _dh: &DisplayHandle,
        _init: &mut DataInit<'_, State>,
    ) {
        if let wl_data_offer::Request::Receive { mime_type, fd } = request {
            state.receive_private_paste(self.operation, mime_type, fd);
        }
    }

    fn destroyed(&self, state: &mut State, _client: ClientId, _resource: &WlDataOffer) {
        state
            .niri
            .event_loop
            .insert_idle(|state| state.refresh_temporary_paste());
    }
}
