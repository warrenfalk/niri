use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use niri_ipc::{Reply, Request, Response};

use super::Fixture;
use crate::ipc::server::on_new_ipc_client;
use crate::layout::LayoutElement as _;

fn connect(f: &mut Fixture) -> UnixStream {
    let (server, client) = UnixStream::pair().unwrap();
    on_new_ipc_client(f.niri_state(), server);
    client.set_nonblocking(true).unwrap();
    client
}

fn request(f: &mut Fixture, client: &mut UnixStream, request: Request) -> Reply {
    let mut bytes = serde_json::to_vec(&request).unwrap();
    bytes.push(b'\n');
    client.write_all(&bytes).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut reply = Vec::new();
    loop {
        f.dispatch();
        let mut buf = [0; 4096];
        match client.read(&mut buf) {
            Ok(n) => reply.extend_from_slice(&buf[..n]),
            Err(err) if err.kind() == ErrorKind::WouldBlock => (),
            result => panic!("IPC read failed: {result:?}"),
        }
        if reply.last() == Some(&b'\n') {
            return serde_json::from_slice(&reply).unwrap();
        }
        assert!(Instant::now() < deadline, "IPC response timed out");
    }
}

fn wait_for_marker_clear(f: &mut Fixture) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while f.niri().layout.windows().next().unwrap().1.is_marked() {
        f.dispatch();
        assert!(Instant::now() < deadline, "marker cleanup timed out");
    }
}

#[test]
fn marker_lease_geometry_and_lifecycle() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.set_size(300, 200);
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    f.niri_complete_animations();

    let id = f.niri().layout.windows().next().unwrap().1.id();
    let layout_id = f.niri().layout.windows().next().unwrap().1.window.clone();
    let mut owner = connect(&mut f);
    let mut other = connect(&mut f);

    // Marking must not cause a configure or geometry change in any sizing mode.
    for mode in 0..3 {
        if mode == 1 {
            f.niri().layout.toggle_window_floating(Some(&layout_id));
            f.double_roundtrip(client);
        }
        if mode == 2 {
            f.niri().layout.set_fullscreen(&layout_id, true);
            f.double_roundtrip(client);
            let window = f.client(client).window(&surface);
            window.set_size(1920, 1080);
            window.ack_last_and_commit();
            f.double_roundtrip(client);
        }
        f.client(client)
            .window(&surface)
            .recent_configures()
            .count();
        let size = f.niri().layout.windows().next().unwrap().1.size();
        assert!(matches!(
            request(
                &mut f,
                &mut owner,
                Request::WindowMarker { id: Some(id.get()) }
            ),
            Ok(Response::Handled)
        ));
        assert!(f.niri().layout.windows().next().unwrap().1.is_marked());
        assert!(request(
            &mut f,
            &mut other,
            Request::WindowMarker { id: Some(id.get()) }
        )
        .is_err());
        assert!(request(&mut f, &mut other, Request::WindowMarker { id: None }).is_err());
        // An invalid retarget must preserve the previous marker.
        assert!(request(
            &mut f,
            &mut owner,
            Request::WindowMarker { id: Some(u64::MAX) }
        )
        .is_err());
        assert!(f.niri().layout.windows().next().unwrap().1.is_marked());
        assert!(request(&mut f, &mut owner, Request::WindowMarker { id: None }).is_ok());
        assert!(!f.niri().layout.windows().next().unwrap().1.is_marked());
        f.double_roundtrip(client);
        assert_eq!(size, f.niri().layout.windows().next().unwrap().1.size());
        assert_eq!(
            f.client(client)
                .window(&surface)
                .recent_configures()
                .count(),
            0
        );
    }

    assert!(request(
        &mut f,
        &mut owner,
        Request::WindowMarker { id: Some(id.get()) }
    )
    .is_ok());
    drop(owner);
    // Other IPC connections do not provide an ordering barrier for disconnect events.
    wait_for_marker_clear(&mut f);
    assert!(request(
        &mut f,
        &mut other,
        Request::WindowMarker { id: Some(id.get()) }
    )
    .is_ok());

    // Disconnect cleanup must also work while a request is awaiting interactive input.
    other.write_all(b"\"PickWindow\"\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while f.niri().pick_window.is_none() {
        f.dispatch();
        assert!(Instant::now() < deadline);
    }
    other.shutdown(std::net::Shutdown::Write).unwrap();
    wait_for_marker_clear(&mut f);
    let mut replacement = connect(&mut f);
    assert!(request(
        &mut f,
        &mut replacement,
        Request::WindowMarker { id: Some(id.get()) }
    )
    .is_ok());
    let mut other = replacement;

    // Keyboard focus is independent of the marker, and the owner can retarget it.
    let second = f.client(client).create_window();
    let second_surface = second.surface.clone();
    second.commit();
    f.roundtrip(client);
    let second = f.client(client).window(&second_surface);
    second.attach_new_buffer();
    second.set_size(200, 100);
    second.ack_last_and_commit();
    f.double_roundtrip(client);
    let second_id = f
        .niri()
        .layout
        .windows()
        .find(|(_, win)| win.id() != id)
        .unwrap()
        .1
        .id();
    let second_layout_id = f
        .niri()
        .layout
        .windows()
        .find(|(_, win)| win.id() == second_id)
        .unwrap()
        .1
        .window
        .clone();
    f.niri().layout.activate_window(&second_layout_id);
    f.double_roundtrip(client);
    assert!(f
        .niri()
        .layout
        .windows()
        .find(|(_, win)| win.id() == id)
        .unwrap()
        .1
        .is_marked());
    assert!(request(
        &mut f,
        &mut other,
        Request::WindowMarker {
            id: Some(second_id.get())
        }
    )
    .is_ok());
    assert!(!f
        .niri()
        .layout
        .windows()
        .find(|(_, win)| win.id() == id)
        .unwrap()
        .1
        .is_marked());
    assert!(request(
        &mut f,
        &mut other,
        Request::WindowMarker { id: Some(id.get()) }
    )
    .is_ok());
    f.client(client)
        .window(&second_surface)
        .xdg_toplevel
        .destroy();
    f.client(client)
        .window(&second_surface)
        .xdg_surface
        .destroy();
    second_surface.destroy();
    f.double_roundtrip(client);

    // Destroying the toplevel must release the marker even while its IPC owner stays connected.
    f.client(client).window(&surface).xdg_toplevel.destroy();
    f.client(client).window(&surface).xdg_surface.destroy();
    surface.destroy();
    f.double_roundtrip(client);
    assert!(f.niri().layout.windows().next().is_none());
    assert!(request(
        &mut f,
        &mut other,
        Request::WindowMarker { id: Some(id.get()) }
    )
    .is_err());
    assert!(request(&mut f, &mut other, Request::WindowMarker { id: None }).is_ok());
}
