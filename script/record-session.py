#!/usr/bin/env python3
"""Records the Pececito window to a GIF for the README.

Run it before you start fishing, pick the Pececito window in the share dialog, then press Ctrl+C
when you're done. It keeps the full recording next to the GIF, so you can make a new GIF from it
with different settings:
  script/record-session.py                         # records to pececito-session.gif
  script/record-session.py --speed 4 out.gif       # plays back 4x faster
  script/record-session.py --from out.mkv --width 480 small.gif

Linux only. It records through the desktop portal and PipeWire, so it needs python-gobject,
GStreamer with its PipeWire plugin, x264enc (gst-plugins-ugly) and ffmpeg.
"""

import argparse
import os
import random
import shutil
import signal
import string
import subprocess
import sys
from pathlib import Path

try:
    import gi

    gi.require_version("Gst", "1.0")
    from gi.repository import Gio, GLib, Gst
except (ImportError, ValueError) as error:
    sys.exit(f"This script needs python-gobject and GStreamer: {error}")

try:
    gi.require_version("GLibUnix", "2.0")
    from gi.repository import GLibUnix

    signal_add = GLibUnix.signal_add
except (ImportError, ValueError):
    signal_add = GLib.unix_signal_add

PORTAL = "org.freedesktop.portal.Desktop"
PORTAL_PATH = "/org/freedesktop/portal/desktop"
SOURCE_WINDOW = 2
CURSOR_HIDDEN = 1
PERSIST_UNTIL_REVOKED = 2
TOKEN_FILE = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache")) / "pececito" / "screencast-token"


class Failure(Exception):
    pass


class Portal:
    """Asks xdg-desktop-portal for a PipeWire stream of one window."""

    def __init__(self, loop):
        self.loop = loop
        self.bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        self.screencast = Gio.DBusProxy.new_sync(
            self.bus, Gio.DBusProxyFlags.NONE, None, PORTAL, PORTAL_PATH,
            "org.freedesktop.portal.ScreenCast", None,
        )
        self.sender = self.bus.get_unique_name()[1:].replace(".", "_")
        self.session = None

    def token(self):
        return "pececito_" + "".join(random.choices(string.ascii_lowercase, k=8))

    def request(self, method, arguments, options):
        """Calls a portal method and waits for its Response signal."""
        handle = self.token()
        options["handle_token"] = GLib.Variant("s", handle)
        path = f"/org/freedesktop/portal/desktop/request/{self.sender}/{handle}"
        reply = {}

        def on_response(_bus, _sender, _path, _interface, _signal, parameters):
            code, results = parameters.unpack()
            reply["code"] = code
            reply["results"] = results
            self.loop.quit()

        subscription = self.bus.signal_subscribe(
            PORTAL, "org.freedesktop.portal.Request", "Response", path, None,
            Gio.DBusSignalFlags.NO_MATCH_RULE, on_response,
        )
        signature = "(" + "".join(kind for kind, _ in arguments) + "a{sv})"
        values = tuple(value for _, value in arguments) + (options,)
        self.screencast.call_sync(method, GLib.Variant(signature, values), Gio.DBusCallFlags.NONE, -1, None)
        self.loop.run()
        self.bus.signal_unsubscribe(subscription)
        if reply.get("code") != 0:
            raise Failure(f"The screen share was cancelled or refused ({method}).")
        return reply["results"]

    def open_window_stream(self):
        results = self.request("CreateSession", [], {"session_handle_token": GLib.Variant("s", self.token())})
        self.session = results["session_handle"]

        options = {
            "types": GLib.Variant("u", SOURCE_WINDOW),
            "multiple": GLib.Variant("b", False),
            "cursor_mode": GLib.Variant("u", CURSOR_HIDDEN),
            "persist_mode": GLib.Variant("u", PERSIST_UNTIL_REVOKED),
        }
        if TOKEN_FILE.is_file():
            options["restore_token"] = GLib.Variant("s", TOKEN_FILE.read_text().strip())
        self.request("SelectSources", [("o", self.session)], options)

        results = self.request("Start", [("o", self.session), ("s", "")], {})
        if "restore_token" in results:
            TOKEN_FILE.parent.mkdir(parents=True, exist_ok=True)
            TOKEN_FILE.write_text(results["restore_token"])
        streams = results.get("streams", [])
        if not streams:
            raise Failure("The portal shared no window.")
        node = streams[0][0]

        reply, fds = self.screencast.call_with_unix_fd_list_sync(
            "OpenPipeWireRemote", GLib.Variant("(oa{sv})", (self.session, {})),
            Gio.DBusCallFlags.NONE, -1, None, None,
        )
        fd = fds.get(reply.unpack()[0])
        return fd, node

    def close(self):
        if self.session is None:
            return
        try:
            self.bus.call_sync(
                PORTAL, self.session, "org.freedesktop.portal.Session", "Close",
                None, None, Gio.DBusCallFlags.NONE, -1, None,
            )
        except GLib.Error:
            pass


def record(video, fps):
    Gst.init(None)
    for element in ("pipewiresrc", "x264enc", "matroskamux"):
        if Gst.ElementFactory.find(element) is None:
            raise Failure(f"GStreamer has no {element} element; install the plugin that provides it.")

    loop = GLib.MainLoop()
    portal = Portal(loop)
    try:
        fd, node = portal.open_window_stream()
        # Y444 keeps odd window sizes encodable; quantizer 0 is lossless so the GIF is made from clean frames.
        pipeline = Gst.parse_launch(
            f"pipewiresrc fd={fd} path={node} do-timestamp=true keepalive-time=100 "
            f"! videoconvert ! videorate ! video/x-raw,framerate={fps}/1 "
            f"! videoconvert ! video/x-raw,format=Y444 "
            f"! x264enc pass=quant quantizer=0 speed-preset=ultrafast "
            f"! matroskamux ! filesink name=output"
        )
        pipeline.get_by_name("output").set_property("location", str(video))
        bus = pipeline.get_bus()
        bus.add_signal_watch()
        outcome = {}

        def on_message(_bus, message):
            if message.type == Gst.MessageType.EOS:
                loop.quit()
            elif message.type == Gst.MessageType.ERROR:
                error, _debug = message.parse_error()
                outcome["error"] = error.message
                loop.quit()

        bus.connect("message", on_message)

        def on_interrupt():
            print("\nStopping...")
            pipeline.send_event(Gst.Event.new_eos())
            return GLib.SOURCE_REMOVE

        signal_add(GLib.PRIORITY_DEFAULT, signal.SIGINT, on_interrupt)
        pipeline.set_state(Gst.State.PLAYING)
        print(f"Recording to {video}. Press Ctrl+C to stop.")
        loop.run()
        pipeline.set_state(Gst.State.NULL)
        if "error" in outcome:
            raise Failure(f"Recording failed: {outcome['error']}")
    finally:
        portal.close()


def make_gif(video, gif, fps, speed, width):
    scale = f"scale={width}:-1:flags=lanczos," if width else ""
    filters = (
        f"setpts=PTS/{speed},fps={fps},{scale}split[frames][copy];"
        "[copy]palettegen=stats_mode=diff[palette];"
        "[frames][palette]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle"
    )
    print(f"Writing {gif}...")
    result = subprocess.run(
        ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(video), "-vf", filters, "-loop", "0", str(gif)]
    )
    if result.returncode != 0:
        raise Failure("ffmpeg couldn't make the GIF.")
    size = gif.stat().st_size / (1024 * 1024)
    print(f"Wrote {gif} ({size:.1f} MiB). The full recording is {video}.")
    if size > 10:
        print("GitHub shows GIFs up to 10 MiB in a README; try --speed, --fps or --width.")


def main():
    parser = argparse.ArgumentParser(description="Records the Pececito window to a GIF.")
    parser.add_argument("gif", nargs="?", default="pececito-session.gif", type=Path, help="where to write the GIF")
    parser.add_argument("--from", dest="source", type=Path, help="make the GIF from an earlier recording instead of recording")
    parser.add_argument("--fps", type=int, default=10, help="frames per second, in the recording and the GIF (default 10)")
    parser.add_argument("--speed", type=float, default=1.0, help="playback speed of the GIF (default 1)")
    parser.add_argument("--width", type=int, help="scale the GIF to this width in pixels (default: the window's width)")
    arguments = parser.parse_args()

    if shutil.which("ffmpeg") is None:
        sys.exit("This script needs ffmpeg.")
    try:
        video = arguments.source
        if video is None:
            video = arguments.gif.with_suffix(".mkv")
            record(video, arguments.fps)
        if not video.is_file() or video.stat().st_size == 0:
            raise Failure(f"There's no recording at {video}.")
        make_gif(video, arguments.gif, arguments.fps, arguments.speed, arguments.width)
    except Failure as error:
        sys.exit(str(error))


if __name__ == "__main__":
    main()
