#!/usr/bin/env python3
"""The usual stack vs the island, under identical conditions.

Both run in a private headless Mango with the same windows, the same tray app and
media player on a private bus, and the real audio server read-only. Each bar is
measured with its child processes included, since the island keeps a `pactl`
listener and Waybar's custom modules spawn helpers.

    bench/run.py                       3 runs of 30 s idle, prints JSON
    WAYBAR_CONFIG=... bench/run.py     another Waybar config

Waybar's workspace pills need a feeder script. It runs during the Waybar pass but
isn't counted against Waybar, because it's a separate program in this setup.
"""
import json, os, shutil, statistics, subprocess, sys, time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
H = ROOT / "tests/headless"
RUNS = int(os.environ.get("RUNS", 3))
IDLE = int(os.environ.get("IDLE", 30))
SWITCHES = 40
STYLES = ("pill", "minimal", "split", "notch", "bar", "frame")
TICK = os.sysconf("SC_CLK_TCK")
HOME = Path.home()
WAYBAR_CONFIG = Path(os.environ.get("WAYBAR_CONFIG", HOME / ".config/waybar/config.jsonc"))
WAYBAR_STYLE = WAYBAR_CONFIG.with_name("style.css")
FEEDER = HOME / ".config/mango/scripts/mangobar"
# mangobar needs building (meson); its stock config is used, so no config file is read.
MANGOBAR = Path(os.environ.get("MANGOBAR", HOME / ".cache/claude-tmp/mb/src/build/mangobar"))
MANGOBAR_LIBS = os.environ.get("MANGOBAR_LIBS", "")  # LD_LIBRARY_PATH for a prefix-installed fcft
ISLAND = ROOT / "target/release/island"
MOCK = ROOT / "target/release/examples/mock"


def session():
    subprocess.run([H / "fresh.sh", "1"], stdout=subprocess.DEVNULL, check=True)
    env = dict(os.environ)
    env.pop("DISPLAY", None)  # run/env unsets it, and only its assignments are read here
    for line in (H / "run/env").read_text().split():
        if "=" in line:
            k, v = line.split("=", 1)
            env[k] = v
    return env


def children(pid):
    """Children of every thread: `pactl` is started from a worker, not the main one."""
    kids = []
    for task in Path(f"/proc/{pid}/task").glob("*/children"):
        try:
            kids += [int(k) for k in task.read_text().split()]
        except OSError:
            pass
    return kids + [g for k in kids for g in children(k)]


def family(pid):
    return [pid] + children(pid)


def cpu_ticks(pid):
    """User + system for the process, its reaped helpers, and live children."""
    total = 0
    for i, p in enumerate(family(pid)):
        try:
            f = Path(f"/proc/{p}/stat").read_text().rsplit(")", 1)[1].split()
        except OSError:
            continue
        total += int(f[11]) + int(f[12])
        if i == 0:
            total += int(f[13]) + int(f[14])
    return total


def wakeups(pid):
    total = 0
    for p in family(pid):
        for task in Path(f"/proc/{p}/task").glob("*/status"):
            try:
                total += next(int(l.split()[1]) for l in task.read_text().splitlines() if l.startswith("voluntary"))
            except (OSError, StopIteration):
                pass
    return total


def memory_kb(pid, field):
    total = 0
    for p in family(pid):
        try:
            total += next(int(l.split()[1]) for l in Path(f"/proc/{p}/smaps_rollup").read_text().splitlines()
                          if l.startswith(field + ":"))
        except (OSError, StopIteration):
            pass
    return total


def threads(pid):
    return sum(1 for _ in Path(f"/proc/{pid}/task").iterdir())


def layer_up(env, name):
    out = subprocess.run(["mmsg", "get", "all-layers"], env=env, capture_output=True, text=True).stdout
    return f'"name":"{name}"' in out


def view(env, tag):
    subprocess.run(["mmsg", "dispatch", f"view,{tag},0"], env=env, stdout=subprocess.DEVNULL)


def measure(name, cmd, env, idle=IDLE):
    """One pass: time to first frame, settled memory, idle cost, switching cost."""
    t0 = time.monotonic()
    proc = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    while not layer_up(env, name):
        if time.monotonic() - t0 > 15:
            proc.kill()
            sys.exit(f"{name} never mapped")
        time.sleep(0.005)
    startup = time.monotonic() - t0
    time.sleep(4)

    pid = proc.pid
    before = (cpu_ticks(pid), wakeups(pid))
    time.sleep(idle)
    idle_ticks, idle_wakes = cpu_ticks(pid) - before[0], wakeups(pid) - before[1]

    before = cpu_ticks(pid)
    for i in range(SWITCHES):
        view(env, 3 if i % 2 == 0 else 1)
        time.sleep(0.15)
    time.sleep(1)
    switch_ticks = cpu_ticks(pid) - before

    result = {
        "startup_ms": round(startup * 1000),
        "pss_mb": round(memory_kb(pid, "Pss") / 1024, 1),
        "rss_mb": round(memory_kb(pid, "Rss") / 1024, 1),
        "threads": threads(pid),
        "processes": len(family(pid)),
        "idle_cpu_pct": round(100 * idle_ticks / TICK / idle, 3),
        "idle_wakeups_per_s": round(idle_wakes / idle, 1),
        "switch_cpu_ms": round(1000 * switch_ticks / TICK / SWITCHES, 2),
    }
    proc.terminate()
    proc.wait(timeout=5)
    time.sleep(0.4)
    return result


def main():
    for tool in (ISLAND, MOCK, WAYBAR_CONFIG):
        if not Path(tool).exists():
            sys.exit(f"missing {tool}: run cargo build --release --bins --examples")
    with_mangobar = MANGOBAR.exists()

    env = session()
    run = Path(env["XDG_RUNTIME_DIR"])
    env.update(PULSE_SERVER=f"unix:{os.environ['XDG_RUNTIME_DIR']}/pulse/native",
               XDG_STATE_HOME=str(run))
    # Visualizers hear the test suite's fake band, never what this machine is playing.
    fake_audio = run / "fake-audio"
    fake_audio.mkdir(exist_ok=True)
    (fake_audio / "parec").symlink_to(H / "shim/parec")
    env["PATH"] = f"{fake_audio}:{env['PATH']}"
    helpers = []
    helpers.append(subprocess.Popen(["playerctld"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
    helpers.append(subprocess.Popen([MOCK], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))

    # A private copy, so the feeder's fullscreen hook can't reach the real wallpaper.
    if FEEDER.exists():
        feed = run / "feeder"
        feed.mkdir(exist_ok=True)
        shutil.copy(FEEDER, feed / "mangobar")
        (feed / "wall").write_text("#!/bin/sh\n")
        (feed / "wall").chmod(0o755)
        helpers.append(subprocess.Popen([feed / "mangobar"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
    time.sleep(0.5)
    # Idle is idle: the track is paused. The visualizers are measured playing, below.
    subprocess.run(["playerctl", "play-pause"], env=env)

    commands = {"waybar": ["waybar", "-c", WAYBAR_CONFIG, "-s", WAYBAR_STYLE], "island": [ISLAND]}
    order = ["waybar", "island"]
    if with_mangobar:
        commands["mangobar"] = [MANGOBAR]
        order.insert(1, "mangobar")
    runs = {name: [] for name in order}
    mb_env = dict(env, XDG_CONFIG_HOME=str(run / "nocfg"))
    if MANGOBAR_LIBS:
        mb_env["LD_LIBRARY_PATH"] = MANGOBAR_LIBS
    # The island as it installs, whatever this machine's config says.
    island_env = dict(env, XDG_CONFIG_HOME=str(run / "nocfg"))
    envs = {"mangobar": mb_env, "island": island_env}
    for i in range(RUNS):
        # Rotate the order so no bar always gets the warm page cache.
        at = i % len(order)
        for name in order[at:] + order[:at]:
            r = measure(name, commands[name], envs.get(name, env))
            runs[name].append(r)
            print(name, r, file=sys.stderr, flush=True)

    # Dunst sits in the background too, so it counts toward what the island replaces.
    # It's measured on its own, because only one daemon can own the name at a time.
    dunst = None
    if shutil.which("dunst"):
        proc = subprocess.Popen(["dunst"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(3)
        subprocess.run(["notify-send", "bench", "warm up"], env=env)
        time.sleep(2)
        dunst = {"pss_mb": round(memory_kb(proc.pid, "Pss") / 1024, 1), "rss_mb": round(memory_kb(proc.pid, "Rss") / 1024, 1)}
        proc.terminate()
        proc.wait(timeout=5)
        time.sleep(0.4)

    # Launcher: from the key's command to the search box being on screen.
    rofi_ms = []
    for _ in range(5 if shutil.which("rofi") else 0):
        t0 = time.monotonic()
        proc = subprocess.Popen(["rofi", "-show", "drun"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        while not layer_up(env, "rofi") and time.monotonic() - t0 < 5:
            time.sleep(0.002)
        rofi_ms.append(round((time.monotonic() - t0) * 1000))
        proc.terminate()
        proc.wait(timeout=5)
        time.sleep(0.4)

    styles = measure_styles(env, run)
    live = measure_live(island_env)
    proc = subprocess.Popen([ISLAND], env=island_env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(2.5)
    idle_pss = memory_kb(proc.pid, "Pss")
    island_ms = []
    for _ in range(5):
        t0 = time.monotonic()
        subprocess.run([ISLAND, "open", "apps"], env=env)
        while time.monotonic() - t0 < 5:
            out = subprocess.run(["mmsg", "get", "all-layers"], env=env, capture_output=True, text=True).stdout
            if '"layer":"overlay","name":"island"' in out:
                break
            time.sleep(0.002)
        island_ms.append(round((time.monotonic() - t0) * 1000))
        time.sleep(0.5)
        subprocess.run([ISLAND, "close"], env=env)
        time.sleep(0.7)
    subprocess.run([ISLAND, "open", "apps"], env=env)
    time.sleep(1)
    open_pss = memory_kb(proc.pid, "Pss")
    subprocess.run([ISLAND, "close"], env=env)
    time.sleep(1.5)
    closed_pss = memory_kb(proc.pid, "Pss")
    proc.terminate()
    proc.wait(timeout=5)
    time.sleep(0.4)

    # Morph cost: pin toggles run the full spring without any pointer.
    proc = subprocess.Popen([ISLAND], env=island_env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(2.5)
    before = cpu_ticks(proc.pid)
    for _ in range(10):
        subprocess.run([ISLAND, "pin"], env=env)
        time.sleep(0.8)
    morph_ms = 1000 * (cpu_ticks(proc.pid) - before) / TICK / 10
    proc.terminate()
    proc.wait(timeout=5)

    for h in reversed(helpers):
        h.terminate()
    subprocess.run(f'for f in {H}/run/*pid; do kill $(cat "$f") 2>/dev/null; done', shell=True)

    version = subprocess.run(["waybar", "--version"], capture_output=True, text=True).stdout.strip()
    median = {n: {k: statistics.median(r[k] for r in rs) for k in rs[0]} for n, rs in runs.items()}
    median["island"]["morph_cpu_ms"] = round(morph_ms, 1)
    if dunst:
        median["dunst"] = dunst
        median["stack_pss_mb"] = round(median["waybar"]["pss_mb"] + dunst["pss_mb"], 1)
        median["stack_rss_mb"] = round(median["waybar"]["rss_mb"] + dunst["rss_mb"], 1)
    median["launcher_ms"] = {"island": statistics.median(island_ms)}
    if rofi_ms:
        median["launcher_ms"]["rofi"] = statistics.median(rofi_ms)
    median["island_palette_pss_mb"] = {"idle": round(idle_pss / 1024, 1), "open": round(open_pss / 1024, 1), "after": round(closed_pss / 1024, 1)}
    median["island_styles"] = styles
    median["island_live"] = live
    report = {
        "method": f"{RUNS} runs, {IDLE}s idle, {SWITCHES} workspace switches, headless Mango, same windows and apps, music paused",
        "waybar_version": version,
        "binary_mb": {"waybar": round(Path(shutil.which("waybar")).stat().st_size / 1e6, 1),
                      "island": round(ISLAND.stat().st_size / 1e6, 1)},
        "median": median,
        "runs": runs,
    }
    if with_mangobar:
        report["mangobar_version"] = subprocess.run([MANGOBAR, "-v"], env=mb_env, capture_output=True, text=True).stdout.strip()
        report["binary_mb"]["mangobar"] = round(MANGOBAR.stat().st_size / 1e6, 1)
    print(json.dumps(report, indent=2))


def measure_styles(env, run):
    """What each style costs at rest and per switch, each started fresh in it."""
    out = {}
    for style in STYLES:
        home = run / f"style-{style}"
        (home / "island").mkdir(parents=True, exist_ok=True)
        (home / "island/config.toml").write_text(f'style = "{style}"\n')
        rs = [measure("island", [ISLAND], dict(env, XDG_CONFIG_HOME=str(home)), idle=10) for _ in range(3)]
        out[style] = {k: statistics.median(r[k] for r in rs) for k in ("pss_mb", "idle_cpu_pct", "idle_wakeups_per_s", "switch_cpu_ms")}
        print(style, out[style], file=sys.stderr, flush=True)
    return out


def own_cpu_pct(pid, seconds=15):
    """The island's own threads, to the nanosecond: the fake band's Python doesn't count."""
    def read():
        total = 0
        for f in Path(f"/proc/{pid}/task").glob("*/schedstat"):
            try:
                total += int(f.read_text().split()[0])
            except OSError:
                pass
        return total
    before = read()
    time.sleep(seconds)
    return round((read() - before) / 1e9 / seconds * 100, 2)


def measure_live(env):
    """Visualizers while music plays, where each one shows, and the sidebar's memory."""
    proc = subprocess.Popen([ISLAND], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(3)
    play = lambda: subprocess.run(["playerctl", "play-pause"], env=env)
    island = lambda cmd: subprocess.run([ISLAND, cmd], env=env)
    out = {}
    play()
    time.sleep(2)
    out["pill_bars_cpu_pct"] = own_cpu_pct(proc.pid)
    island("pin")
    time.sleep(2)
    out["panel_wave_ring_cpu_pct"] = own_cpu_pct(proc.pid)
    island("pin")
    time.sleep(1.5)
    island("sidebar")
    time.sleep(2)
    out["sidebar_wave_ring_cpu_pct"] = own_cpu_pct(proc.pid)
    play()
    time.sleep(2)
    out["sidebar_open_paused_cpu_pct"] = own_cpu_pct(proc.pid)
    out["sidebar_open_pss_mb"] = round(memory_kb(proc.pid, "Pss") / 1024, 1)
    island("sidebar")
    time.sleep(2.5)
    out["sidebar_closed_pss_mb"] = round(memory_kb(proc.pid, "Pss") / 1024, 1)
    out["paused_cpu_pct"] = own_cpu_pct(proc.pid)
    proc.terminate()
    proc.wait(timeout=5)
    time.sleep(0.4)
    return out


if __name__ == "__main__":
    main()
