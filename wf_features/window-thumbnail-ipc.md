# Window Thumbnail IPC

This fork-local feature lets a client ask niri for a PNG thumbnail of a mapped
window when the client already knows the niri window id. The intended use case is
launchers, switchers, dashboards, or other trusted local tools that want a small
preview of a specific window without starting a full screencopy session.

This is a V1 interface and intentionally has no per-client authorization model.
Any process that can connect to `$NIRI_SOCKET` can ask for thumbnails of mapped
windows by id. The compositor does reject requests while locked, but this should
not be treated as a privacy boundary.

## IPC Contract

The request is a `Request::WindowThumbnail` JSON object:

```json
{"WindowThumbnail":{"id":12,"max_width":256,"max_height":160}}
```

- `id` is the niri window id, available from the existing `Windows` IPC request.
- `max_width` and `max_height` are physical-pixel bounds. Both must be non-zero.
- The returned image preserves the window aspect ratio and does not upscale the
  window if it is already smaller than the requested bounds.
- The V1.1 implementation captures any mapped window. If a window is not
  currently associated with an output, niri renders its last committed buffers
  with the active output scale, or scale `1.0` if no output is active.
- V1.1 does not force hidden or off-output clients to repaint. Live repainting
  is a separate future feature.

Successful replies contain a `Response::WindowThumbnail`:

```json
{
  "Ok": {
    "WindowThumbnail": {
      "id": 12,
      "width": 256,
      "height": 144,
      "png_base64": "..."
    }
  }
}
```

`png_base64` is a base64-encoded PNG. `width` and `height` are the actual
thumbnail dimensions in physical pixels.

## How It Is Built

To rebuild a feature like this, add a new IPC request and response type in
`niri-ipc`, then handle the request in `src/ipc/server.rs` by scheduling the
rendering work onto the compositor event loop. The compositor side should reuse
the same render element path as window screenshots so decorations, shadows,
rounded corners, and surface contents match screenshot behavior.

The renderer should:

1. Find the mapped window by niri window id.
2. Refuse capture while the session is locked.
3. Select a capture scale from the window's output, then the active output, then
   scale `1.0`.
4. Compute a thumbnail size that fits inside the requested bounds without
   upscaling.
5. Render the window elements into an offscreen pixel buffer.
6. Encode the buffer as PNG and return it as base64 through IPC.

Keep tests with the feature commit. The current coverage locks down the JSON
contract in `niri-ipc` and the thumbnail scale, size, and validation logic in
`src/niri.rs`. If the rendering path changes later, add focused tests around
whatever pure logic or state boundary protects the behavior.

## Usage

First find the target window id:

```sh
niri msg --json windows
```

Then send a raw IPC request to the socket. The `niri msg` CLI does not currently
have a wrapper subcommand for this fork-local request.

```sh
printf '%s\n' '{"WindowThumbnail":{"id":12,"max_width":256,"max_height":160}}' \
  | socat STDIO "$NIRI_SOCKET"
```

For a script, connect to `$NIRI_SOCKET`, write the JSON request followed by a
newline, read one JSON reply line, then base64-decode `Ok.WindowThumbnail.png_base64`
into PNG bytes.

```python
import base64
import json
import os
import socket

request = {"WindowThumbnail": {"id": 12, "max_width": 256, "max_height": 160}}

client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
client.connect(os.environ["NIRI_SOCKET"])
client.sendall(json.dumps(request).encode() + b"\n")
reply = json.loads(client.makefile("rb").readline())

thumbnail = reply["Ok"]["WindowThumbnail"]
png = base64.b64decode(thumbnail["png_base64"])

with open("window-12.png", "wb") as file:
    file.write(png)
```
