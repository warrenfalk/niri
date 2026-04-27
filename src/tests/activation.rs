use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{
    Anchor, KeyboardInteractivity,
};
use wayland_client::protocol::wl_surface::WlSurface as ClientWlSurface;

use super::client::LayerConfigureProps;
use super::*;
use crate::layout::LayoutElement as _;
use crate::niri::KeyboardFocus;
use crate::utils::with_toplevel_role;

fn map_window(
    f: &mut Fixture,
    id: client::ClientId,
    title: &str,
    size: (u16, u16),
) -> ClientWlSurface {
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.set_title(title);
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.set_size(size.0, size.1);
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    surface
}

fn map_window_with_app_id(
    f: &mut Fixture,
    id: client::ClientId,
    title: &str,
    app_id: &str,
    size: (u16, u16),
) -> ClientWlSurface {
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.set_title(title);
    window.set_app_id(app_id);
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.set_size(size.0, size.1);
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    surface
}

fn set_up_two_windows() -> (Fixture, client::ClientId, ClientWlSurface, ClientWlSurface) {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let id = f.add_client();
    let first = map_window(&mut f, id, "first", (100, 100));
    let second = map_window(&mut f, id, "second", (120, 120));

    (f, id, first, second)
}

fn set_up_two_windows_with_serialless_rule(
    rule: &str,
) -> (Fixture, client::ClientId, ClientWlSurface, ClientWlSurface) {
    let mut f = Fixture::with_config(niri_config::Config::parse_mem(rule).unwrap());
    f.add_output(1, (1920, 1080));

    let id = f.add_client();
    let first = map_window_with_app_id(&mut f, id, "first", "test-target", (100, 100));
    let second = map_window_with_app_id(&mut f, id, "second", "other", (120, 120));

    (f, id, first, second)
}

fn map_exclusive_top_layer(f: &mut Fixture, id: client::ClientId) -> ClientWlSurface {
    let layer = f
        .client(id)
        .create_layer(None, Layer::Top, "activation-test");
    let surface = layer.surface.clone();
    layer.set_configure_props(LayerConfigureProps {
        anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
        size: Some((0, 40)),
        kb_interactivity: Some(KeyboardInteractivity::Exclusive),
        ..Default::default()
    });
    layer.commit();
    f.roundtrip(id);

    let layer = f.client(id).layer(&surface);
    layer.attach_new_buffer();
    layer.set_size(1920, 40);
    layer.ack_last_and_commit();
    f.double_roundtrip(id);

    surface
}

fn mapped_window_by_title<'a>(f: &'a mut Fixture, title: &str) -> &'a crate::window::Mapped {
    f.niri()
        .layout
        .windows()
        .find_map(|(_, mapped)| {
            with_toplevel_role(mapped.toplevel(), |role| {
                (role.title.as_deref() == Some(title)).then_some(mapped)
            })
        })
        .unwrap()
}

fn simulate_mapped_window_activation(f: &mut Fixture, title: &str) {
    let target = {
        let mapped = mapped_window_by_title(f, title);
        mapped.window.clone()
    };
    let niri = f.niri();
    niri.layout.activate_window(&target);
    niri.layer_shell_on_demand_focus = None;
}

fn simulate_serialless_mapped_window_activation(f: &mut Fixture, title: &str) {
    let (target, target_surface, honor_serialless_activation) = {
        let mapped = mapped_window_by_title(f, title);
        (
            mapped.window.clone(),
            mapped.toplevel().wl_surface().clone(),
            mapped.rules().honor_xdg_activation_without_serial == Some(true),
        )
    };

    if honor_serialless_activation {
        let niri = f.niri();
        niri.layout.activate_window(&target);
        niri.layer_shell_on_demand_focus = None;
    } else {
        let niri = f.niri();
        let mapped = niri
            .layout
            .find_window_and_output_mut(&target_surface)
            .unwrap()
            .0;
        mapped.set_urgent(true);
        niri.queue_redraw_all();
    }
}

fn layout_focus_title(f: &mut Fixture) -> String {
    with_toplevel_role(f.niri().layout.focus().unwrap().toplevel(), |role| {
        role.title.clone().unwrap()
    })
}

fn keyboard_focus_layout_title(f: &mut Fixture) -> Option<String> {
    let surface = match &f.niri().keyboard_focus {
        KeyboardFocus::Layout {
            surface: Some(surface),
        } => surface.clone(),
        _ => return None,
    };

    Some(with_toplevel_role(
        f.niri()
            .layout
            .find_window_and_output(&surface)
            .unwrap()
            .0
            .toplevel(),
        |role| role.title.clone().unwrap(),
    ))
}

#[test]
fn mapped_activation_updates_keyboard_focus_without_focus_blockers() {
    let (mut f, id, first, _second) = set_up_two_windows();

    assert_eq!(layout_focus_title(&mut f), "second");
    assert_eq!(
        keyboard_focus_layout_title(&mut f).as_deref(),
        Some("second")
    );

    let _ = f.client(id).window(&first).format_recent_configures();
    simulate_mapped_window_activation(&mut f, "first");
    f.double_roundtrip(id);

    assert_eq!(layout_focus_title(&mut f), "first");
    assert_eq!(
        keyboard_focus_layout_title(&mut f).as_deref(),
        Some("first")
    );
    assert!(mapped_window_by_title(&mut f, "first").is_focused());
}

#[test]
fn mapped_activation_can_leave_keyboard_focus_on_exclusive_layer_shell() {
    let (mut f, id, first, _second) = set_up_two_windows();
    let _layer = map_exclusive_top_layer(&mut f, id);

    assert_eq!(layout_focus_title(&mut f), "second");
    assert!(matches!(
        f.niri().keyboard_focus,
        KeyboardFocus::LayerShell { .. }
    ));

    let _ = f.client(id).window(&first).format_recent_configures();
    simulate_mapped_window_activation(&mut f, "first");
    f.double_roundtrip(id);

    assert_eq!(layout_focus_title(&mut f), "first");
    assert!(matches!(
        f.niri().keyboard_focus,
        KeyboardFocus::LayerShell { .. }
    ));
    assert!(!mapped_window_by_title(&mut f, "first").is_focused());
    assert_eq!(f.client(id).window(&first).format_recent_configures(), "");
}

#[test]
fn serialless_mapped_activation_is_urgency_only_by_default() {
    let (mut f, id, first, _second) = set_up_two_windows_with_serialless_rule("");

    assert_eq!(layout_focus_title(&mut f), "second");
    assert_eq!(
        keyboard_focus_layout_title(&mut f).as_deref(),
        Some("second")
    );

    let _ = f.client(id).window(&first).format_recent_configures();
    simulate_serialless_mapped_window_activation(&mut f, "first");
    f.double_roundtrip(id);

    assert_eq!(layout_focus_title(&mut f), "second");
    assert_eq!(
        keyboard_focus_layout_title(&mut f).as_deref(),
        Some("second")
    );
    assert!(mapped_window_by_title(&mut f, "first").is_urgent());
    assert_eq!(f.client(id).window(&first).format_recent_configures(), "");
}

#[test]
fn serialless_mapped_activation_can_be_honored_by_window_rule() {
    let (mut f, id, first, _second) = set_up_two_windows_with_serialless_rule(
        r#"
        window-rule {
            match app-id="^test-target$"
            honor-xdg-activation-without-serial true
        }
        "#,
    );

    assert_eq!(layout_focus_title(&mut f), "second");
    assert_eq!(
        keyboard_focus_layout_title(&mut f).as_deref(),
        Some("second")
    );

    let _ = f.client(id).window(&first).format_recent_configures();
    simulate_serialless_mapped_window_activation(&mut f, "first");
    f.double_roundtrip(id);

    assert_eq!(layout_focus_title(&mut f), "first");
    assert_eq!(
        keyboard_focus_layout_title(&mut f).as_deref(),
        Some("first")
    );
    assert!(mapped_window_by_title(&mut f, "first").is_focused());
}
