#!/usr/bin/env python3
"""Thin original host for the melonDS DS libretro core (public C ABI).

Drives the *official* melonDS DS core instead of the standalone emulator: joypad and
touch are supplied per emulated frame through the core's own input callback, video is
captured from the software renderer, and RAM / savestates are read through the core ABI.
No OS window, no focus, no posted messages, no GDB. The GPL core DLL stays outside the
repository (`tools/build/melondsds_libretro.dll`, unzipped from its archive).

Usage:
  python scripts/melondsds_host.py --rom ROM --input run.jsonl --frames 30000 \
      --checkpoints out/checkpoints.jsonl --dump-ram out/ram.bin --shot out/end.png

Options (all paths may be absolute; the core and the ROM live in the main checkout):
  --rom PATH          ROM to boot; default $DQ9_ROM, else the first `rom/*.nds`.
  --core PATH         libretro core; default `tools/build/melondsds_libretro.dll`,
                      else $DQ9_CORE.
  --sysdir DIR        core system directory (builtin firmware, no BIOS needed here).
  --savedir DIR       where the core writes the game's `.sav`; default build/melondsds.
  --input FILE        JSONL input, one entry per frame index (see below).
  --frames N          number of frames to run; default: one past the last input frame.
  --checkpoints FILE  JSONL of story checkpoints, same fields as
                      `dq9-app --checkpoints` (tick/seconds/kind/vblank/map/area/track/
                      daytime/tracks/flags/quests/event/hero). Each checkpoint runs
                      `dq9re state --ram` on a RAM image through `--dq9re`.
  --checkpoint-ram-dir DIR  keep the 4 MiB RAM image of each checkpoint in DIR.
  --checkpoint-every N      also checkpoint every N frames.
  --dq9re PATH        `dq9re` binary for checkpoints; default target/release, then
                      target/debug, else $DQ9RE.
  --dump-ram FILE     write the 4 MiB main RAM (ARM9 `0x02000000`) at the end.
  --state-out FILE    `retro_serialize` (core savestate) at the end.
  --state-in FILE     `retro_unserialize` before running (resume a real run).
  --shot FILE         PNG at the end.
  --shot-every N      PNG every N frames, into `--shot-dir` (default build/melondsds/shots).
  --core-log FILE     keep the core's stdout log (default: discarded).

Input format (JSONL; one JSON object per line, ascending `frame`, held until the next
line that changes it — so a sparse file is enough and a line per frame is exact):
  {"frame": 3600, "pad": [], "touch": [127, 377]}
  {"frame": 3700, "pad": ["a"], "checkpoint": "title"}
  {"frame": 3705, "pad": []}
    frame       index of the emulated frame (`retro_run` calls so far); omitted = previous
                frame + 1. Frames before the first line get no input.
    pad         joypad buttons held: a b x y l r start select up down left right. Omitted
                keeps the previous set; `[]` releases everything.
    touch       `[x, y]` in frame pixels (both screens stacked, 256x384, top screen first;
                the host maps them to libretro's pointer range and the core subtracts the
                touchscreen offset), or null to lift the stylus. Omitted keeps the touch.
    checkpoint  true, or a kind string, to capture a checkpoint before this frame runs.
    shot        PNG path to capture before this frame runs.
"""
import argparse
import ctypes as C
import glob
import json
import os
import subprocess
import sys
import tempfile

# libretro constants
DEVICE_JOYPAD = 1
DEVICE_POINTER = 6
JOY = {"b": 0, "y": 1, "select": 2, "start": 3, "up": 4, "down": 5,
       "left": 6, "right": 7, "a": 8, "x": 9, "l": 10, "r": 11, "l2": 12,
       "r2": 13, "l3": 14, "r3": 15}
PIXEL_0RGB1555, PIXEL_XRGB8888, PIXEL_RGB565 = 0, 1, 2

ENV_SET_PIXEL_FORMAT = 10
ENV_GET_SYSTEM_DIRECTORY = 9
ENV_GET_SAVE_DIRECTORY = 31
ENV_SET_VARIABLES = 16
ENV_GET_VARIABLE = 15
ENV_GET_VARIABLE_UPDATE = 17
ENV_GET_LOG_INTERFACE = 27
ENV_SET_INPUT_DESCRIPTORS = 11
ENV_GET_CORE_ASSETS_DIRECTORY = 30
ENV_SET_SUPPORT_NO_GAME = 18
ENV_GET_LANGUAGE = 39
ENV_SET_GEOMETRY = 37

MEMORY_SYSTEM_RAM = 2

# Fields of a `dq9-app --checkpoints` object, in order, so the two files read alike.
CHECKPOINT_FIELDS = ("vblank", "map", "area", "track", "daytime", "tracks", "flags",
                     "quests", "hero")


class GameInfo(C.Structure):
    _fields_ = [("path", C.c_char_p), ("data", C.c_void_p),
                ("size", C.c_size_t), ("meta", C.c_char_p)]


class SystemInfo(C.Structure):
    _fields_ = [("library_name", C.c_char_p), ("library_version", C.c_char_p),
                ("valid_extensions", C.c_char_p), ("need_fullpath", C.c_bool),
                ("block_extract", C.c_bool)]


class Variable(C.Structure):
    _fields_ = [("key", C.c_char_p), ("value", C.c_char_p)]


class LogCallback(C.Structure):
    # `void (*log)(enum retro_log_level, const char *fmt, ...)`; only the level and the
    # format string are read, the variadic arguments are ignored (they cannot be decoded
    # from Python), which still keeps the core's chatter out of the terminal.
    _fields_ = [("log", C.c_void_p)]


LOG_CB = C.CFUNCTYPE(None, C.c_int, C.c_char_p)


ENV_CB = C.CFUNCTYPE(C.c_bool, C.c_uint, C.c_void_p)
VIDEO_CB = C.CFUNCTYPE(None, C.c_void_p, C.c_uint, C.c_uint, C.c_size_t)
AUDIO_CB = C.CFUNCTYPE(None, C.c_int16, C.c_int16)
AUDIO_BATCH_CB = C.CFUNCTYPE(C.c_size_t, C.POINTER(C.c_int16), C.c_size_t)
POLL_CB = C.CFUNCTYPE(None)
INPUT_CB = C.CFUNCTYPE(C.c_int16, C.c_uint, C.c_uint, C.c_uint, C.c_uint)


def note(*parts):
    print(*parts, file=sys.stderr, flush=True)


class Host:
    def __init__(self, dll, sysdir, savedir, log=None):
        self.lib = C.CDLL(os.path.abspath(dll))
        self.log = log
        self.sysdir = C.c_char_p(os.path.abspath(sysdir).encode())
        self.savedir = C.c_char_p(os.path.abspath(savedir).encode())
        self.pixel_format = PIXEL_0RGB1555
        self.options = {}
        self.default_options = {
            "melonds_homebrew_readonly": "disabled",
            "melonds_homebrew_sync_sdcard_to_host": "enabled",
            "melonds_homebrew_sdcard": "disabled",
            "melonds_dsi_sdcard_readonly": "disabled",
            "melonds_dsi_sdcard_sync_sdcard_to_host": "enabled",
            "melonds_dsi_sdcard": "enabled",
            "melonds_firmware_language": "auto",
            "melonds_console_mode": "auto",
            "melonds_boot_mode": "native",
            "melonds_sysfile_mode": "builtin",
            "melonds_renderer": "software",
            "melonds_slot2_device": "auto",
        }
        self.option_strings = {}
        self.frame = None
        self.width = self.height = self.pitch = 0
        self.joypad = 0
        self.touch = None
        self.frames = 0
        self._keep = []
        self._bind()

    def _bind(self):
        L = self.lib
        L.retro_api_version.restype = C.c_uint
        L.retro_set_environment.argtypes = [ENV_CB]
        L.retro_set_video_refresh.argtypes = [VIDEO_CB]
        L.retro_set_audio_sample.argtypes = [AUDIO_CB]
        L.retro_set_audio_sample_batch.argtypes = [AUDIO_BATCH_CB]
        L.retro_set_input_poll.argtypes = [POLL_CB]
        L.retro_set_input_state.argtypes = [INPUT_CB]
        L.retro_init.argtypes = []
        L.retro_deinit.argtypes = []
        L.retro_get_system_info.argtypes = [C.POINTER(SystemInfo)]
        L.retro_load_game.argtypes = [C.POINTER(GameInfo)]
        L.retro_load_game.restype = C.c_bool
        L.retro_unload_game.argtypes = []
        L.retro_run.argtypes = []
        L.retro_serialize_size.restype = C.c_size_t
        L.retro_serialize.argtypes = [C.c_void_p, C.c_size_t]
        L.retro_serialize.restype = C.c_bool
        L.retro_unserialize.argtypes = [C.c_void_p, C.c_size_t]
        L.retro_unserialize.restype = C.c_bool
        L.retro_get_memory_data.argtypes = [C.c_uint]
        L.retro_get_memory_data.restype = C.c_void_p
        L.retro_get_memory_size.argtypes = [C.c_uint]
        L.retro_get_memory_size.restype = C.c_size_t

        self._env_cb = ENV_CB(self._environment)
        self._video_cb = VIDEO_CB(self._video)
        self._audio_cb = AUDIO_CB(lambda l, r: None)
        self._batch_cb = AUDIO_BATCH_CB(lambda data, frames: frames)
        self._poll_cb = POLL_CB(lambda: None)
        self._input_cb = INPUT_CB(self._input)
        self._log_cb = LOG_CB(self._log)

    def _log(self, level, message):
        if self.log:
            self.log.write(f"{level} {message.decode(errors='replace')}\n")

    def _environment(self, cmd, data):
        cmd &= 0xFFFF  # libretro ORs RETRO_ENVIRONMENT_EXPERIMENTAL (0x10000) into some calls
        if cmd == ENV_SET_PIXEL_FORMAT:
            self.pixel_format = C.cast(data, C.POINTER(C.c_uint))[0]
            return True
        if cmd == ENV_GET_SYSTEM_DIRECTORY:
            C.cast(data, C.POINTER(C.c_char_p))[0] = self.sysdir
            return True
        if cmd == ENV_GET_SAVE_DIRECTORY:
            C.cast(data, C.POINTER(C.c_char_p))[0] = self.savedir
            return True
        if cmd == ENV_GET_CORE_ASSETS_DIRECTORY:
            C.cast(data, C.POINTER(C.c_char_p))[0] = self.sysdir
            return True
        if cmd == ENV_SET_VARIABLES:
            arr = C.cast(data, C.POINTER(Variable))
            i = 0
            while arr[i].key:
                k = arr[i].key.decode()
                v = (arr[i].value or b"").decode()
                # SET_VARIABLES carries "Label; option1|option2"; the default is option1.
                if ";" in v:
                    v = v.split(";", 1)[1].split("|")[0].strip()
                self.options.setdefault(k, v)
                i += 1
            return True
        if cmd == ENV_GET_VARIABLE:
            ptr = C.cast(data, C.POINTER(Variable))
            key = ptr[0].key.decode()
            val = self.options.get(key, self.default_options.get(key))
            if val is None:
                note("core asks for an unknown option", key)
                return False
            if key not in self.option_strings:
                self.option_strings[key] = C.c_char_p(val.encode())
            ptr[0].value = self.option_strings[key]
            return True
        if cmd == ENV_GET_VARIABLE_UPDATE:
            C.cast(data, C.POINTER(C.c_bool))[0] = False
            return True
        if cmd == ENV_GET_LANGUAGE:
            C.cast(data, C.POINTER(C.c_uint))[0] = 0
            return True
        if cmd in (ENV_SET_INPUT_DESCRIPTORS, ENV_SET_SUPPORT_NO_GAME, ENV_SET_GEOMETRY):
            return True
        if cmd == ENV_GET_LOG_INTERFACE:
            C.cast(data, C.POINTER(LogCallback))[0].log = C.cast(self._log_cb, C.c_void_p)
            return True
        return False

    def _video(self, data, width, height, pitch):
        if not data:
            return
        self.frame = (C.string_at(data, pitch * height), width, height, pitch)
        self.width, self.height, self.pitch = width, height, pitch

    def _input(self, port, device, index, ident):
        if device == DEVICE_JOYPAD:
            return 1 if (self.joypad >> ident) & 1 else 0
        if device == DEVICE_POINTER:
            if self.touch is None:
                return 0
            x, y = self.touch
            if ident == 0:
                return self._normalize(x, self.width or 256)
            if ident == 1:
                return self._normalize(y, self.height or 384)
            if ident == 2:
                return 1
            if ident == 3:
                return 1
            return 0
        return 0

    @staticmethod
    def _normalize(pixel, size):
        """Frame pixel to libretro's pointer position (-0x8000..0x7fff over the frame)."""
        return max(-0x8000, min(0x7fff, round((pixel + 0.5) / size * 0x1_0000) - 0x8000))

    def init(self):
        self.lib.retro_set_environment(self._env_cb)
        self.lib.retro_set_video_refresh(self._video_cb)
        self.lib.retro_set_audio_sample(self._audio_cb)
        self.lib.retro_set_audio_sample_batch(self._batch_cb)
        self.lib.retro_set_input_poll(self._poll_cb)
        self.lib.retro_set_input_state(self._input_cb)
        self.lib.retro_init()
        info = SystemInfo()
        self.lib.retro_get_system_info(C.byref(info))
        note("core", (info.library_name or b"?").decode(),
             (info.library_version or b"?").decode(), "api", self.lib.retro_api_version())

    def load(self, rom):
        data = open(rom, "rb").read()
        buf = C.create_string_buffer(data, len(data))
        self._keep.append(buf)
        gi = GameInfo(os.path.abspath(rom).encode(), C.cast(buf, C.c_void_p),
                      len(data), None)
        ok = self.lib.retro_load_game(C.byref(gi))
        if not ok:
            note("retro_load_game refused", rom)
        return ok

    def run(self, n=1):
        for _ in range(n):
            self.lib.retro_run()
            self.frames += 1

    def save_png(self, path):
        if not self.frame:
            note("no video frame yet, cannot write", path)
            return
        raw, w, h, pitch = self.frame
        from PIL import Image
        if self.pixel_format == PIXEL_XRGB8888:
            im = Image.frombytes("RGB", (w, h), raw, "raw", "BGRX", pitch)
        elif self.pixel_format == PIXEL_RGB565:
            im = Image.frombytes("RGB", (w, h), raw, "raw", "BGR;16", pitch)
        else:
            im = Image.frombytes("RGB", (w, h), raw, "raw", "BGR;15", pitch)
        parent = os.path.dirname(os.path.abspath(path))
        os.makedirs(parent, exist_ok=True)
        im.save(path)

    def serialize(self):
        size = self.lib.retro_serialize_size()
        buf = C.create_string_buffer(size)
        ok = self.lib.retro_serialize(buf, size)
        return ok, bytes(buf.raw[:size])

    def ram(self, kind=MEMORY_SYSTEM_RAM):
        size = self.lib.retro_get_memory_size(kind)
        ptr = self.lib.retro_get_memory_data(kind)
        if not ptr or not size:
            return b""
        return C.string_at(ptr, size)


def read_input(path):
    """Parse the JSONL input file into per-frame segments, in frame order."""
    entries = []
    previous = -1
    with open(path, encoding="utf-8") as f:
        for number, line in enumerate(f, 1):
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            try:
                entry = json.loads(line)
            except json.JSONDecodeError as error:
                raise SystemExit(f"{path}:{number}: {error}")
            if not isinstance(entry, dict):
                raise SystemExit(f"{path}:{number}: not a JSON object")
            frame = entry.get("frame", previous + 1)
            if not isinstance(frame, int) or frame <= previous:
                raise SystemExit(f"{path}:{number}: frame {frame!r} is not after {previous}")
            previous = frame
            pad = entry.get("pad")
            if pad is not None:
                unknown = [b for b in pad if b not in JOY]
                if unknown:
                    raise SystemExit(f"{path}:{number}: unknown button {unknown[0]!r}")
            touch = entry.get("touch")
            if touch is not None and (not isinstance(touch, list) or len(touch) != 2
                                     or not all(isinstance(v, int) for v in touch)):
                raise SystemExit(f"{path}:{number}: touch must be [x, y] or null")
            entries.append({"frame": frame, "pad": pad, "touch": touch,
                            "touch_given": "touch" in entry,
                            "checkpoint": entry.get("checkpoint"),
                            "shot": entry.get("shot")})
    return entries


def find_dq9re(explicit):
    candidates = [explicit] if explicit else []
    candidates += [os.environ.get("DQ9RE"),
                   os.path.join("target", "release", "dq9re.exe"),
                   os.path.join("target", "debug", "dq9re.exe"),
                   os.path.join("target", "release", "dq9re"),
                   os.path.join("target", "debug", "dq9re")]
    for path in candidates:
        if path and os.path.exists(path):
            return os.path.abspath(path)
    return None


class Checkpoints:
    """Writes `dq9-app --checkpoints`-compatible JSONL from RAM images `dq9re` reads."""

    def __init__(self, path, dq9re, ramdir):
        self.path = path
        self.dq9re = dq9re
        self.ramdir = ramdir
        self.lines = 0
        os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
        open(self.path, "w", encoding="utf-8").close()

    def capture(self, host, frame, kind="frame"):
        ram = host.ram()
        if not ram:
            note("checkpoint at frame", frame, "skipped: the core exposes no main RAM")
            return
        with tempfile.TemporaryDirectory() as scratch:
            image = os.path.join(scratch, "ram.bin")
            with open(image, "wb") as f:
                f.write(ram)
            if self.ramdir:
                os.makedirs(self.ramdir, exist_ok=True)
                kept = os.path.join(self.ramdir, f"f{frame:06d}.bin")
                with open(kept, "wb") as f:
                    f.write(ram)
            out = subprocess.run([self.dq9re, "state", "--ram", image, "--json"],
                                 capture_output=True, text=True)
            if out.returncode != 0:
                note("checkpoint at frame", frame, "failed:", out.stderr.strip())
                return
            values = json.loads(out.stdout)
        line = {"tick": frame, "seconds": round(frame / 60.0, 2), "kind": kind}
        for field in CHECKPOINT_FIELDS:
            line[field] = values[field]
        line["event"] = None  # the original has no scene id we can read yet
        with open(self.path, "a", encoding="utf-8") as f:
            f.write(json.dumps(line, separators=(",", ":")) + "\n")
        self.lines += 1
        note("checkpoint", self.lines, "at frame", frame, kind,
             "map", line["map"], "vblank", line["vblank"])


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--rom", default=os.environ.get("DQ9_ROM"))
    ap.add_argument("--core", default=os.environ.get("DQ9_CORE",
                                                    "tools/build/melondsds_libretro.dll"))
    ap.add_argument("--sysdir", default="build/melondsds/sys")
    ap.add_argument("--savedir", default="build/melondsds")
    ap.add_argument("--input")
    ap.add_argument("--frames", type=int, default=0)
    ap.add_argument("--checkpoints")
    ap.add_argument("--checkpoint-ram-dir")
    ap.add_argument("--checkpoint-every", type=int, default=0)
    ap.add_argument("--dq9re")
    ap.add_argument("--dump-ram")
    ap.add_argument("--state-out")
    ap.add_argument("--state-in")
    ap.add_argument("--shot")
    ap.add_argument("--shot-every", type=int, default=0)
    ap.add_argument("--shot-dir", default="build/melondsds/shots")
    ap.add_argument("--core-log")
    args = ap.parse_args()

    rom = args.rom
    if not rom:
        found = sorted(glob.glob(os.path.join("rom", "*.nds")))
        rom = found[0] if found else None
    if not rom or not os.path.exists(rom):
        raise SystemExit("no ROM: pass --rom PATH or set DQ9_ROM")
    if not os.path.exists(args.core):
        raise SystemExit(f"no core at {args.core}: pass --core PATH or set DQ9_CORE")
    if not os.path.isdir(args.sysdir):
        os.makedirs(args.sysdir, exist_ok=True)
    if not os.path.isdir(args.savedir):
        os.makedirs(args.savedir, exist_ok=True)

    entries = read_input(args.input) if args.input else []
    starts = {e["frame"]: e for e in entries}
    total = args.frames or (entries[-1]["frame"] + 1 if entries else 0)
    if total <= 0:
        raise SystemExit("nothing to run: pass --frames N or --input FILE")

    dq9re = find_dq9re(args.dq9re) if args.checkpoints else None
    if args.checkpoints and not dq9re:
        raise SystemExit("--checkpoints needs dq9re: build it or pass --dq9re PATH")
    checkpoints = Checkpoints(args.checkpoints, dq9re, args.checkpoint_ram_dir) \
        if args.checkpoints else None

    # The core logs through the libretro log interface; keep it out of the terminal unless
    # a file is asked for.
    log = open(args.core_log, "w", encoding="utf-8", buffering=1) if args.core_log else None

    try:
        host = Host(args.core, args.sysdir, args.savedir, log)
        host.init()
        if not host.load(rom):
            return 2
        if args.state_in:
            data = open(args.state_in, "rb").read()
            buf = C.create_string_buffer(data, len(data))
            host._keep.append(buf)
            if not host.lib.retro_unserialize(buf, len(data)):
                note("retro_unserialize refused", args.state_in)
                return 3
            note("resumed", args.state_in)
        for frame in range(total):
            entry = starts.get(frame)
            if entry:
                if entry["pad"] is not None:
                    host.joypad = 0
                    for button in entry["pad"]:
                        host.joypad |= 1 << JOY[button]
                if entry["touch_given"]:
                    host.touch = tuple(entry["touch"]) if entry["touch"] else None
                if entry["shot"]:
                    host.save_png(entry["shot"])
                if entry["checkpoint"] and checkpoints:
                    kind = entry["checkpoint"]
                    checkpoints.capture(host, frame,
                                        kind if isinstance(kind, str) else "frame")
            if checkpoints and args.checkpoint_every and frame % args.checkpoint_every == 0:
                checkpoints.capture(host, frame)
            host.run()
            if args.shot_every and host.frames % args.shot_every == 0:
                host.save_png(os.path.join(args.shot_dir, f"f{host.frames:06d}.png"))
        if args.shot:
            host.save_png(args.shot)
            note("shot", args.shot, f"{host.width}x{host.height}", "format", host.pixel_format)
        if args.state_out:
            ok, data = host.serialize()
            with open(args.state_out, "wb") as f:
                f.write(data)
            note("state", "ok" if ok else "failed", len(data), "bytes ->", args.state_out)
        if args.dump_ram:
            data = host.ram()
            with open(args.dump_ram, "wb") as f:
                f.write(data)
            note("ram", len(data), "bytes ->", args.dump_ram)
        if checkpoints:
            note("checkpoints", checkpoints.lines, "->", args.checkpoints)
    finally:
        if log:
            log.close()
    note("frames run", host.frames)
    return 0


if __name__ == "__main__":
    sys.exit(main())
