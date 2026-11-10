<#
.SYNOPSIS
  Produces melonDS field states with roaming monsters from the demo save, by real key presses
  (docs/re/encounter.md, "Claims checked"; docs/re/melonds-state.md, "Field states").

.DESCRIPTION
  Stage 1 (recipe): boots the ROM in the melonDS DS libretro core through scripts/melondsds_host.py,
  copies the save into the core's SAVE_RAM (the harness does not), presses START and A (Continue),
  walks out of the Stornway church (C01M09), through the town (C01) and its south door onto
  Western Stornway Region (F02, map 20002), and saves the state `f02-enter` (core state, main RAM
  image, screenshot). Every step is a held key; nothing is written to the game's memory.

  Stage 2 (watch, unless -RecipeOnly): from `f02-enter` a seeded random walk patrols the field and
  logs, frame by frame, the twelve roamer objects of the map slot: `<name>.tsv` (spawns `S`, state
  changes `T`, contacts `C`, numeric columns documented in docs/re/encounter.md) and `<name>.jsonl`.
  A contact (a roamer reaching state 8) is logged, its battle screen is captured, and the run
  rolls back to an earlier snapshot of its own to keep patrolling. States are saved when the day
  clock passes each of -SaveClocks with at least two roamers on the map.

  The output directory then feeds `DQ9_FIELD_DIR` of `cargo test -p dq9-core --test encounter_rom`.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts/melonds-field.ps1 -Out build/field
  $env:DQ9_FIELD_DIR = (Resolve-Path build/field/run).Path
#>
param(
    [string] $Out = 'build/field',
    [string] $Rom,
    [string] $Save,
    [string] $Core,
    [string] $Dq9re,
    [int] $Seed = 4,
    [double] $EndClock = 424,
    [double[]] $SaveClocks = @(60, 120, 190, 250, 330, 395, 410),
    [int] $MaxContacts = 60,
    [switch] $RecipeOnly
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot

# The main checkout when this runs from a linked worktree (tools/, rom/ and build/ stay there).
function Get-MainRoot {
    $git = Join-Path $Root '.git'
    if (-not (Test-Path $git -PathType Leaf)) { return $Root }
    $line = Get-Content $git -TotalCount 1
    if ($line -match '^gitdir:\s*(.+)$') {
        $dir = [IO.Path]::GetFullPath($Matches[1].Trim())
        return (Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $dir)))
    }
    return $Root
}
function Find-Shared([string] $Relative) {
    foreach ($base in @($Root, (Get-MainRoot))) {
        $p = Join-Path $base $Relative
        if (Test-Path $p) { return (Resolve-Path $p).Path }
    }
    throw "not found: $Relative (looked in the worktree and the main checkout)"
}

if (-not $Rom) { $Rom = if ($env:DQ9_ROM) { $env:DQ9_ROM } else { (Get-ChildItem (Join-Path (Get-MainRoot) 'rom') -Filter *.nds | Select-Object -First 1).FullName } }
if (-not $Save) { $Save = Find-Shared 'build/sav/demo.sav' }
if (-not $Core) { $Core = Find-Shared 'tools/build/melondsds_libretro.dll' }
if (-not $Dq9re) {
    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
    $Dq9re = Join-Path $target 'release/dq9re.exe'
    if (-not (Test-Path $Dq9re)) { throw "build it first: cargo build --release -p dq9re ($Dq9re)" }
}
foreach ($f in @($Rom, $Save, $Core)) { if (-not $f -or -not (Test-Path $f)) { throw "missing input: $f" } }

$Out = [IO.Path]::GetFullPath((Join-Path $Root $Out))
New-Item -ItemType Directory -Force $Out | Out-Null
$sys = Join-Path $Out 'sys'; $savedir = Join-Path $Out 'sav'
New-Item -ItemType Directory -Force $sys, $savedir | Out-Null

$Py = Join-Path $Out 'melonds_field.py'
$RecipeFile = Join-Path $Out 'recipe.txt'
$utf8 = New-Object System.Text.UTF8Encoding($false)
[IO.File]::WriteAllText($Py, @'

"""Field-state driver for melonds-field.ps1: demo.sav -> Continue -> a field map, by real key presses.

Uses scripts/melondsds_host.py (the libretro harness) as a module. The harness does not load a
`.sav` (the core takes it from SAVE_RAM), so the save is copied into SAVE_RAM right after
`retro_load_game`. No GameState write: every state comes from play.
"""
import argparse, ctypes as C, importlib.util, json, math, os, random, struct, subprocess, sys

def load_host(path):
    spec = importlib.util.spec_from_file_location("melondsds_host", path)
    m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m); return m

class Driver:
    GS = 0x020F33D8          # GameState instance (docs/re/melonds-state.md)
    ZONE = 0x020FB3F0        # g_Zone3D: low 16 bits of the first word = map id
    def __init__(self, mh, core, rom, sysdir, savedir, sav):
        self.mh = mh
        self.h = mh.Host(core, sysdir, savedir, None)
        self.h.init()
        if not self.h.load(rom):
            raise SystemExit("retro_load_game refused")
        data = open(sav, "rb").read()
        ptr = self.h.lib.retro_get_memory_data(0)
        if not ptr or self.h.lib.retro_get_memory_size(0) != len(data):
            raise SystemExit("SAVE_RAM missing or of another size than the save")
        C.memmove(ptr, data, len(data))
        self.log = []
    def run(self, frames): self.h.run(frames)
    def hold(self, keys, frames):
        m = 0
        for k in keys: m |= 1 << self.mh.JOY[k]
        self.h.joypad = m; self.h.run(frames); self.h.joypad = 0
    def tap(self, keys, frames=6, after=0):
        self.hold(keys, frames)
        if after: self.h.run(after)
    def u32(self, a): return struct.unpack("<I", self.h.read_ram(a, 4))[0]
    def i32(self, a): return struct.unpack("<i", self.h.read_ram(a, 4))[0]
    def map_id(self): return self.u32(self.ZONE) & 0xFFFF
    def hero(self):
        o = self.u32(self.GS + 8 + 4 * self.u32(self.GS + 0x3AC))
        return tuple(self.i32(o + 0x44 + 4 * k) / 4096 for k in range(3))
    def snapshot(self): return self.h.serialize()[1]
    def restore(self, data):
        buf = C.create_string_buffer(data, len(data))
        if not self.h.lib.retro_unserialize(buf, len(data)): raise SystemExit("unserialize refused")
    def save(self, out, name, dq9re):
        os.makedirs(out, exist_ok=True)
        ok, st = self.h.serialize()
        if not ok: raise SystemExit("serialize failed")
        open(os.path.join(out, name + "-state.bin"), "wb").write(st)
        ram = os.path.join(out, name + "-ram.bin")
        open(ram, "wb").write(self.h.ram())
        self.h.save_png(os.path.join(out, name + ".png"))
        return ram


def parse_recipe(text):
    steps = []
    for n, line in enumerate(text.splitlines(), 1):
        line = line.split("#", 1)[0].strip()
        if not line: continue
        w = line.split()
        steps.append((n, w[0], w[1:]))
    return steps

def play(d, steps, out, dq9re, rom, log):
    for n, op, a in steps:
        if op == "wait": d.run(int(a[0]))
        elif op == "hold": d.hold(a[0].split("+"), int(a[1]))
        elif op == "tap": d.tap(a[0].split("+"), 6, int(a[1]) if len(a) > 1 else 0)
        elif op == "expect-map":
            m = d.map_id()
            if m != int(a[0]): raise SystemExit(f"recipe line {n}: map {m}, expected {a[0]}")
        elif op == "state":
            ram = d.save(out, a[0], dq9re)
            r = subprocess.run([dq9re, "state", "--ram", ram, "--json"], capture_output=True, text=True)
            if r.returncode: raise SystemExit(r.stdout + r.stderr)
            s = json.loads(r.stdout)
            line = {"state": a[0], "frames": d.h.frames, "vblank": s["vblank"], "map": s["map"],
                    "daytime": s["daytime"], "hero": s["hero"]}
            log.write(json.dumps(line) + "\n"); log.flush(); print(json.dumps(line), flush=True)
        elif op == "pos":
            print(f"frame {d.h.frames} map {d.map_id()} hero {tuple(round(v, 2) for v in d.hero())}", flush=True)
        else: raise SystemExit(f"recipe line {n}: unknown step {op}")


def watch(a, mh, d):
    out = a["out"]; os.makedirs(out, exist_ok=True)
    log = open(os.path.join(out, a["name"] + ".jsonl"), "w")
    GS = d.GS; SL = 0x020FDD44; STRIDE = 0x198
    def clock(): return struct.unpack("<f", d.h.read_ram(GS + 0x3CC, 4))[0]
    def phase(): return d.u32(GS + 0x3DC)
    def obj_ptr(i): return d.u32(GS + 8 + 4 * i)
    base = obj_ptr(0x70)
    assert all(obj_ptr(0x70 + k) == base + k * STRIDE for k in range(12)), "group 0 not contiguous"
    def hero_obj(): return d.u32(GS + 8 + 4 * d.u32(GS + 0x3AC))
    def table(): return d.h.read_ram(base, 12 * STRIDE)
    def parse(t, k):
        o = k * STRIDE
        s16 = lambda off: struct.unpack_from("<h", t, o + off)[0]
        u32 = lambda off: struct.unpack_from("<I", t, o + off)[0]
        i32 = lambda off: struct.unpack_from("<i", t, o + off)[0]
        return {"slot": k, "monster": s16(2), "rpos": [i32(0x44 + 4 * j) for j in range(3)], "pos": [round(i32(0x44 + 4 * j) / 4096, 3) for j in range(3)],
                "angle": i32(0x54), "state": u32(0x130) & 0xFF, "state_ms": i32(0x14C), "awake_ms": i32(0x154),
                "node": s16(0xB8), "node_id": s16(0x164), "zone": s16(0x168), "scale": i32(0x5C),
                "wander": t[o + 0x138], "walk": t[o + 0x139], "vision": t[o + 0x13A]}
    def hero():
        o = hero_obj()
        return {"rpos": [d.i32(o + 0x44 + 4 * j) for j in range(3)], "pos": [round(d.i32(o + 0x44 + 4 * j) / 4096, 3) for j in range(3)], "angle": d.i32(o + 0x54),
                "node": struct.unpack("<h", d.h.read_ram(o + 0xB8, 2))[0], "c2": d.h.read_ram(o + 0xC2, 1)[0]}
    def slot0():
        return {"map": struct.unpack("<H", d.h.read_ram(SL, 2))[0], "spawned": struct.unpack("<H", d.h.read_ram(SL + 6, 2))[0],
                "timer": d.u32(SL + 8)}
    tsv = open(os.path.join(out, a["name"] + ".tsv"), "w")
    def T(*cols): tsv.write("\t".join(str(c) for c in cols) + "\n")
    def emit(kind, **kw):
        kw.update(kind=kind, frame=d.h.frames, clock=round(clock(), 3), phase=phase()); log.write(json.dumps(kw) + "\n")
    rng = random.Random(a.get("seed", 1))
    KEYS = ["up", "down", "left", "right", "up+left", "up+right", "down+left", "down+right"]
    prev = [parse(table(), k) for k in range(12)]
    ring = []   # (frame, snapshot)
    end_clock = a.get("end_clock"); max_frames = a.get("frames", 30000)
    saves = a.get("save_clocks", []); done_saves = set()
    contacts = 0; held = []; hold_left = 0; idle_until = 0; t_start = d.h.frames
    emit("start", hero=hero(), slot=slot0(), roamers=[r for r in prev if r["monster"] >= 0])
    while d.h.frames - t_start < max_frames:
        if end_clock is not None and clock() >= end_clock: break
        if hold_left <= 0:
            held = rng.choice(KEYS).split("+") if rng.random() > a.get("idle", 0.15) else []
            hold_left = rng.randint(30, 150)
        # stop walking when a roamer is close ahead (avoid contact) once avoidance is on
        m = 0
        for k in held: m |= 1 << mh.JOY[k]
        hero0 = hero(); slot00 = slot0(); clock0 = clock()
        d.h.joypad = m; d.h.run(1); d.h.joypad = 0; hold_left -= 1
        if d.h.frames % 300 == 0:
            ring.append((d.h.frames, d.snapshot())); ring = ring[-3:]
        t = table(); cur = [parse(t, k) for k in range(12)]; restored = False
        for k in range(12):
            p, c = prev[k], cur[k]
            if p["monster"] < 0 <= c["monster"]:
                r0 = c; h0 = hero0
                T("S", d.h.frames, a["map"], round(clock0 * 1000), phase(), h0["rpos"][0], h0["rpos"][1], h0["rpos"][2], h0["angle"], h0["node"], slot00["timer"],
                  k, r0["monster"], r0["zone"], r0["node"], r0["node_id"], *r0["rpos"], len([x for x in prev if x["monster"] >= 0]),
                  *[v for x in prev if x["monster"] >= 0 for v in (*x["rpos"], x["node"], x["state"], x["node_id"])])
                emit("spawn", slot=k, roamer=c, hero=hero(), slot0=slot0(), hero_before=hero0, slot0_before=slot00, clock_before=round(clock0, 3),
                     others=[x for x in cur if x["monster"] >= 0 and x["slot"] != k],
                     others_before=[x for x in prev if x["monster"] >= 0])
            elif p["monster"] >= 0 and c["monster"] < 0:
                emit("gone", slot=k, last=p, hero=hero())
            elif c["monster"] >= 0 and p["state"] != c["state"]:
                T("T", d.h.frames, k, c["monster"], p["state"], c["state"], p["state_ms"], p["awake_ms"], c["wander"], c["walk"], c["vision"])
                emit("state", slot=k, old=p["state"], new=c["state"], old_state_ms=p["state_ms"], old_awake_ms=p["awake_ms"],
                     roamer=c, hero=hero())
            if c["monster"] >= 0 and c["state"] == 8 and p["state"] != 8:
                contacts += 1
                T("C", d.h.frames, a["map"], round(clock0 * 1000), phase(), k, c["monster"], c["zone"], c["state"], *hero0["rpos"], hero0["angle"], *hero()["rpos"], hero()["angle"], *p["rpos"], p["angle"], *c["rpos"], c["angle"], p["state"], p["state_ms"])
                emit("contact", slot=k, roamer=c, roamer_before=p, hero=hero(), hero_before=hero0, roamers=[x for x in cur if x["monster"] >= 0])
                if a.get("save_contact") and contacts <= a.get("save_contact"):
                    d.save(out, f"{a['name']}-contact{contacts}", a["dq9re"])
                d.run(a.get("battle_wait", 150)); d.h.save_png(os.path.join(out, f"{a['name']}-contact{contacts}-battle.png"))
                emit("after_contact", map=d.map_id(), roamers=[parse(table(), j) for j in range(12) if parse(table(), j)["monster"] >= 0])
                if ring and contacts < a.get("max_contacts", 4):
                    f, snap = ring[0]; d.restore(snap); ring = [(f, snap)]; hold_left = 0
                    held = []; d.run(a.get("rest", 200))
                    cur = [parse(table(), j) for j in range(12)]
                    restored = True
                    break
                else:
                    emit("stop", why="contact limit"); return
        if restored:
            prev = cur; continue
        prev = cur
        for sc in saves:
            if sc not in done_saves and clock() >= sc and sum(1 for r in cur if r["monster"] >= 0) >= a.get("min_roamers", 2):
                done_saves.add(sc)
                name = f"{a['name']}-c{int(sc)}"
                d.save(out, name, a["dq9re"]); emit("saved", name=name, roamers=[r for r in cur if r["monster"] >= 0], hero=hero(), slot0=slot0())
    tsv.close()
    emit("end", hero=hero(), slot0=slot0(), roamers=[r for r in prev if r["monster"] >= 0])
    print("done", d.h.frames, clock())


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("mode", choices=["recipe", "watch"])
    ap.add_argument("--host", required=True); ap.add_argument("--core", required=True)
    ap.add_argument("--rom", required=True); ap.add_argument("--sav", required=True)
    ap.add_argument("--sysdir", required=True); ap.add_argument("--savedir", required=True)
    ap.add_argument("--dq9re", required=True); ap.add_argument("--out", required=True)
    ap.add_argument("--recipe"); ap.add_argument("--log")
    ap.add_argument("--state"); ap.add_argument("--args", help="file with the JSON watch parameters")
    a = ap.parse_args()
    mh = load_host(a.host)
    d = Driver(mh, a.core, a.rom, a.sysdir, a.savedir, a.sav)
    if a.mode == "recipe":
        steps = parse_recipe(open(a.recipe).read())
        with open(a.log, "w") as log:
            play(d, steps, a.out, a.dq9re, a.rom, log)
    else:
        w = json.load(open(a.args)); w.update(out=a.out, dq9re=a.dq9re, state=a.state)
        d.restore(open(a.state, "rb").read())
        watch(w, mh, d)

if __name__ == "__main__":
    main()
'@, $utf8)
[IO.File]::WriteAllText($RecipeFile, @'
# demo.sav -> title -> Continue -> the Stornway church (night, 19.2.1) -> the town -> F02.
# Frames are 60 Hz emulated frames; every key is a real press, no GameState write.
wait 1300
tap start 10
wait 190
tap a 6              # Continue your adventure
wait 600
tap a 120            # close the greeting
tap a 100
expect-map 109       # C01M09, the church
state church
hold down 180        # out of the church onto C01 (Stornway)
expect-map 100
wait 120
state town
hold down 80
hold right 16
hold down 32
hold down+right 16
hold right 16
hold down+right 16
hold down+left 16
hold down 16
hold down+left 32
hold down 64
hold down+left 64
hold left 16
hold up 16
hold left 48
hold down+left 16
hold left 16
hold down+left 16
hold down 16
hold left 48
hold down 96
hold down+left 16
hold down 80
hold down 120        # through the door at the south of the town
wait 200
expect-map 20002     # F02, Western Stornway Region
state f02-enter
'@, $utf8)

$common = @('--host', (Join-Path $PSScriptRoot 'melondsds_host.py'), '--core', $Core, '--rom', $Rom,
    '--sav', $Save, '--sysdir', $sys, '--savedir', $savedir, '--dq9re', $Dq9re)
$states = Join-Path $Out 'states'
Write-Host "recipe: $Rom"
# The host prints its banner on stderr: judge by the exit code, not by the stream.
$ErrorActionPreference = 'Continue'
& python $Py recipe @common --recipe $RecipeFile --out $states --log (Join-Path $Out 'recipe.log')
if ($LASTEXITCODE -ne 0) { throw "recipe failed ($LASTEXITCODE)" }
if ($RecipeOnly) { return }

$watch = @{ name = 'w'; seed = $Seed; map = 20002; end_clock = $EndClock; frames = 60000; save_clocks = $SaveClocks;
            min_roamers = 2; max_contacts = $MaxContacts; save_contact = 3; battle_wait = 420 } | ConvertTo-Json -Compress
$run = Join-Path $Out 'run'
Write-Host "watch: seed $Seed until clock $EndClock"
$ArgsFile = Join-Path $Out 'watch.json'
[IO.File]::WriteAllText($ArgsFile, $watch, $utf8)
& python $Py watch @common --state (Join-Path $states 'f02-enter-state.bin') --out $run --args $ArgsFile
if ($LASTEXITCODE -ne 0) { throw "watch failed ($LASTEXITCODE)" }
Write-Host "states and logs in $run"
