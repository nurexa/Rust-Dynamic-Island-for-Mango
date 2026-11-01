#!/usr/bin/env python3
"""Waybar vs the island, under identical conditions.

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
TICK = os.sysconf("SC_CLK_TCK")
HOME = Path.home()
WAYBAR_CONFIG = Path(os.environ.get("WAYBAR_CONFIG", HOME / ".config/waybar/config.jsonc"))
WAYBAR_STYLE = WAYBAR_CONFIG.with_name("style.css")
FEEDER = HOME / ".config/mango/scripts/mangobar"
ISLAND = ROOT / "target/release/island"
MOCK = ROOT / "target/release/examples/mock"


def session():
    subprocess.run([H / "fresh.sh", "1"], stdout=subprocess.DEVNULL, check=True)
    env = dict(os.environ)
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


def measure(name, cmd, env):
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
    time.sleep(IDLE)
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
        "idle_cpu_pct": round(100 * idle_ticks / TICK / IDLE, 3),
        "idle_wakeups_per_s": round(idle_wakes / IDLE, 1),
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

    env = session()
    run = Path(env["XDG_RUNTIME_DIR"])
    env.update(DBUS_SESSION_BUS_ADDRESS=f"unix:path={run}/bus", PULSE_SERVER=f"unix:{os.environ['XDG_RUNTIME_DIR']}/pulse/native",
               XDG_STATE_HOME=str(run))
    helpers = [
        subprocess.Popen(["dbus-daemon", "--session", "--nofork", f"--address={env['DBUS_SESSION_BUS_ADDRESS']}"],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)]
    time.sleep(0.3)
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

    runs = {"waybar": [], "island": []}
    commands = {"waybar": ["waybar", "-c", WAYBAR_CONFIG, "-s", WAYBAR_STYLE], "island": [ISLAND]}
    for i in range(RUNS):
        # Alternate the order so neither bar always gets the warm page cache.
        for name in (("waybar", "island") if i % 2 == 0 else ("island", "waybar")):
            r = measure(name, commands[name], env)
            runs[name].append(r)
            print(name, r, file=sys.stderr, flush=True)

    # Morph cost: pin toggles run the full spring without any pointer.
    proc = subprocess.Popen([ISLAND], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
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
    print(json.dumps({
        "method": f"{RUNS} runs, {IDLE}s idle, {SWITCHES} workspace switches, headless Mango, same windows and apps",
        "waybar_version": version,
        "binary_mb": {"waybar": round(Path(shutil.which("waybar")).stat().st_size / 1e6, 1),
                      "island": round(ISLAND.stat().st_size / 1e6, 1)},
        "median": median,
        "runs": runs,
    }, indent=2))


if __name__ == "__main__":
    main()
