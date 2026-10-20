//! Connection-owned surrounding-text snapshots; insertion remains ordinary private paste.

use std::cell::Cell;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use async_channel::Sender;
use calloop::timer::{TimeoutAction, Timer};
use calloop::RegistrationToken;
use niri_ipc::{CommitTextRequest, TextInputContext};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::input_method::InputMethodSeat;
use smithay::wayland::text_input::{TextInputContext as ClientContext, TextInputSeat};

use crate::niri::State;
use crate::utils::id::IdCounter;

static REVISIONS: IdCounter = IdCounter::new();
const SETTLE: Duration = Duration::from_millis(40);
const DEADLINE: Duration = Duration::from_millis(300);
const POLL: Duration = Duration::from_millis(10);
pub const STALE_CONTEXT: &str =
    "text input context changed before paste; review the draft and send again";

pub struct ContextLease {
    window: u64,
    surface: WlSurface,
    owner: Weak<Cell<bool>>,
    owns_mediation: bool,
    timer: Option<RegistrationToken>,
    snapshot: Option<(u64, ClientContext)>,
}

fn same_context(a: &ClientContext, b: &ClientContext) -> bool {
    a.surface == b.surface
        && a.text_input == b.text_input
        && a.activation_serial == b.activation_serial
        && a.surrounding_text == b.surrounding_text
        && a.content_type == b.content_type
}

fn usable(context: &ClientContext) -> bool {
    let Some((text, cursor, anchor)) = &context.surrounding_text else {
        return false;
    };
    let Some((hint, purpose)) = context.content_type else {
        return false;
    };
    // Hidden/sensitive fields and password/PIN purposes must never leave the compositor.
    let hint: u32 = hint.into();
    let purpose: u32 = purpose.into();
    hint & (0x40 | 0x80) == 0
        && !matches!(purpose, 8 | 9)
        && text.len() <= 4000
        && text.is_char_boundary(*cursor as usize)
        && text.is_char_boundary(*anchor as usize)
}

impl State {
    pub fn commit_text_context(&mut self, request: CommitTextRequest) -> Result<(), String> {
        if request.text.is_empty() || request.text.len() > 4000 {
            return Err(String::from(
                "text input commit must contain 1 to 4000 UTF-8 bytes",
            ));
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
        if self.niri.seat.input_method().has_instance() {
            return Err(String::from(
                "text input commit is unavailable while an input method is bound",
            ));
        }
        self.validate_text_context(request.id, request.context_revision)?;
        let input = self.niri.seat.text_input();
        input.with_active_text_input(|text_input, _| {
            text_input.commit_string(Some(request.text.clone()));
        });
        input.done(false);
        // A revision is single-use for insertion, including before the client
        // has committed its resulting surrounding-text update.
        self.niri.text_context.as_mut().unwrap().snapshot = None;
        Ok(())
    }

    fn clear_text_context(&mut self) {
        if let Some(lease) = self.niri.text_context.take() {
            if let Some(timer) = lease.timer {
                self.niri.event_loop.remove(timer);
            }
            if lease.owns_mediation {
                let input = self.niri.seat.text_input();
                input.update_compositor_input_method(false);
                if !self.niri.seat.input_method().has_instance() {
                    input.leave();
                }
            }
        }
    }

    pub fn refresh_text_context(&mut self) {
        let Some(lease) = &self.niri.text_context else {
            return;
        };
        if self.niri.is_locked()
            || !lease.owner.upgrade().is_some_and(|owner| owner.get())
            || !self.niri.layout.windows().any(|(_, win)| {
                win.id().get() == lease.window && win.toplevel().wl_surface() == &lease.surface
            })
        {
            self.clear_text_context();
        }
    }

    pub fn read_text_context(
        &mut self,
        id: Option<u64>,
        focus: bool,
        owner: &Rc<Cell<bool>>,
        reply: Sender<Result<Option<TextInputContext>, String>>,
    ) -> Result<(), String> {
        self.refresh_text_context();
        if self.niri.text_context.as_ref().is_some_and(|lease| {
            lease
                .owner
                .upgrade()
                .is_some_and(|current| !Rc::ptr_eq(&current, owner))
        }) {
            return Err(String::from(
                "text context is owned by another IPC connection",
            ));
        }
        let Some(id) = id else {
            self.clear_text_context();
            let _ = reply.try_send(Ok(None));
            return Ok(());
        };
        if self.niri.is_locked() || !self.niri.keyboard_focus.is_layout() || !owner.get() {
            return Err(String::from("text context requires an unlocked layout"));
        }
        let keyboard = self
            .niri
            .seat
            .get_keyboard()
            .ok_or("seat has no keyboard")?;
        let mods = keyboard.modifier_state();
        if focus
            && (keyboard.is_grabbed()
                || !keyboard.pressed_keys().is_empty()
                || mods.ctrl
                || mods.shift
                || mods.alt
                || mods.logo
                || mods.iso_level3_shift
                || mods.iso_level5_shift
                || mods.serialized.depressed != 0
                || mods.serialized.latched != 0)
        {
            // Same preflight as PasteText: no focus or input side effects yet.
            return Err(String::from(
                "paste requires an unlocked layout with no held keys or keyboard grab",
            ));
        }
        let (window, surface) = self
            .niri
            .layout
            .windows()
            .find(|(output, win)| output.is_some() && win.id().get() == id)
            .map(|(_, win)| (win.window.clone(), win.toplevel().wl_surface().clone()))
            .ok_or("text context destination is closed or has no output")?;
        if focus {
            self.niri.layout.activate_window(&window);
            self.update_keyboard_focus();
        }
        if keyboard.current_focus().as_ref() != Some(&surface) || keyboard.is_grabbed() {
            return Err(String::from("text context destination is not focused"));
        }
        if self
            .niri
            .text_context
            .as_ref()
            .is_some_and(|lease| lease.window != id)
        {
            self.clear_text_context();
        }
        if self.niri.text_context.is_none() {
            let input = self.niri.seat.text_input();
            let owns_mediation = !input.compositor_input_method();
            input.update_compositor_input_method(true);
            if owns_mediation && !self.niri.seat.input_method().has_instance() {
                input.enter();
            }
            self.niri.text_context = Some(ContextLease {
                window: id,
                surface: surface.clone(),
                owner: Rc::downgrade(owner),
                owns_mediation,
                timer: None,
                snapshot: None,
            });
        }
        if self.niri.text_context.as_ref().unwrap().timer.is_some() {
            return Err(String::from("another text context read is pending"));
        }
        let deadline = Instant::now() + DEADLINE;
        let mut stable_since = Instant::now();
        let mut candidate: Option<ClientContext> = None;
        let mut acknowledged = None;
        let token = self
            .niri
            .event_loop
            .insert_source(Timer::immediate(), move |_, _, state| {
                state.refresh_text_context();
                let Some(lease) = state.niri.text_context.as_ref() else {
                    let _ = reply.try_send(Err(String::from("text context lease ended")));
                    return TimeoutAction::Drop;
                };
                let input = state.niri.seat.text_input();
                let now = Instant::now();
                let current = input
                    .current_context()
                    .filter(|context| context.surface == surface);
                if keyboard.current_focus().as_ref() != Some(&surface) || keyboard.is_grabbed() {
                    state.niri.text_context.as_mut().unwrap().timer = None;
                    let _ =
                        reply.try_send(Err(String::from("text context destination changed focus")));
                    return TimeoutAction::Drop;
                }
                if let Some(context) = &current {
                    let key = (context.text_input.clone(), context.serial);
                    if acknowledged.as_ref() != Some(&key)
                        && !state.niri.seat.input_method().has_instance()
                    {
                        // Chrome waits for done before flushing pending surrounding text.
                        // Empty state acknowledgement never inserts/deletes/preedits text.
                        input.done(false);
                        acknowledged = Some(key);
                    }
                }
                let unchanged = match (&candidate, &current) {
                    (Some(a), Some(b)) => same_context(a, b),
                    (None, None) => true,
                    _ => false,
                };
                if !unchanged {
                    stable_since = now;
                    candidate = current.clone();
                }
                let ready = current.as_ref().is_some_and(usable)
                    && now.duration_since(stable_since) >= SETTLE;
                if ready || now >= deadline {
                    let result = if ready {
                        let context = current.unwrap();
                        let revision = REVISIONS.next();
                        let (text, cursor, anchor) = context.surrounding_text.clone().unwrap();
                        let (hint, purpose) = context.content_type.unwrap();
                        let snapshot = TextInputContext {
                            id: lease.window,
                            revision,
                            text,
                            cursor,
                            anchor,
                            content_hint: hint.into(),
                            content_purpose: purpose.into(),
                        };
                        state.niri.text_context.as_mut().unwrap().snapshot =
                            Some((revision, context));
                        Some(snapshot)
                    } else {
                        state.niri.text_context.as_mut().unwrap().snapshot = None;
                        None
                    };
                    state.niri.text_context.as_mut().unwrap().timer = None;
                    let _ = reply.try_send(Ok(result));
                    TimeoutAction::Drop
                } else {
                    TimeoutAction::ToDuration(POLL)
                }
            })
            .map_err(|_| String::from("could not schedule text context read"))?;
        self.niri.text_context.as_mut().unwrap().timer = Some(token);
        Ok(())
    }

    pub fn validate_text_context(&mut self, id: u64, revision: u64) -> Result<(), String> {
        self.refresh_text_context();
        let valid = self
            .niri
            .text_context
            .as_ref()
            .filter(|lease| lease.window == id)
            .and_then(|lease| lease.snapshot.as_ref())
            .filter(|(token, _)| *token == revision)
            .is_some_and(|(_, expected)| {
                self.niri
                    .seat
                    .text_input()
                    .current_context()
                    .as_ref()
                    .is_some_and(|current| usable(current) && same_context(expected, current))
            });
        if valid {
            Ok(())
        } else {
            Err(String::from(STALE_CONTEXT))
        }
    }
}
