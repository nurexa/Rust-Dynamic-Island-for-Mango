//! Hardware stats, read only while a panel is open, on a thread of their own so
//! a slow driver call can't stall an animation. Anything this machine doesn't
//! have comes back None and its tile disappears.
use crate::Msg;
use libloading::{Library, Symbol};
use std::{
    ffi::{c_int, c_uint, c_void},
    fs,
    io::{self, BufRead, BufReader, Write},
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::mpsc,
    thread,
    time::Instant,
};

#[derive(Clone, Default, Debug, PartialEq)]
pub struct Snapshot {
    pub cpu: Option<f64>,
    pub cpu_temp: Option<f64>,
    pub gpu: Option<f64>,
    pub gpu_temp: Option<f64>,
    pub mem: Option<(f64, f64)>,
    pub rx: f64,
    pub tx: f64,
    /// Percent, charging.
    pub battery: Option<(f64, bool)>,
}

#[repr(C)]
#[derive(Default)]
struct Utilization {
    gpu: c_uint,
    memory: c_uint,
}

/// NVML costs tens of MB resident, and every init/shutdown pair leaks an
/// eventfd inside the driver library, while libcuda can't be unloaded at all.
/// So it never enters this process: a copy of this binary runs `gpu_sampler`
/// only while a panel is open, and exiting cleans up what shutdown can't.
struct Nvml {
    lib: Library,
    dev: *mut c_void,
}

impl Nvml {
    fn open() -> Option<Self> {
        unsafe {
            let lib = Library::new("libnvidia-ml.so.1").ok()?;
            let init: Symbol<unsafe extern "C" fn() -> c_int> = lib.get(b"nvmlInit_v2").ok()?;
            if init() != 0 {
                return None;
            }
            let by_index: Symbol<unsafe extern "C" fn(c_uint, *mut *mut c_void) -> c_int> =
                lib.get(b"nvmlDeviceGetHandleByIndex_v2").ok()?;
            let mut dev = std::ptr::null_mut();
            if by_index(0, &mut dev) != 0 {
                return None;
            }
            drop(by_index);
            drop(init);
            Some(Nvml { lib, dev })
        }
    }

    fn read(&self) -> Option<(f64, f64)> {
        unsafe {
            let util: Symbol<unsafe extern "C" fn(*mut c_void, *mut Utilization) -> c_int> =
                self.lib.get(b"nvmlDeviceGetUtilizationRates").ok()?;
            let temp: Symbol<unsafe extern "C" fn(*mut c_void, c_int, *mut c_uint) -> c_int> =
                self.lib.get(b"nvmlDeviceGetTemperature").ok()?;
            let (mut u, mut t) = (Utilization::default(), 0);
            if util(self.dev, &mut u) != 0 || temp(self.dev, 0, &mut t) != 0 {
                return None;
            }
            Some((u.gpu as f64, t as f64))
        }
    }
}

/// Package temperature: coretemp's first sensor, or AMD's Tctl.
fn cpu_temp_path() -> Option<PathBuf> {
    fs::read_dir("/sys/class/hwmon").ok()?.flatten().map(|d| d.path()).find_map(|p| {
        let name = fs::read_to_string(p.join("name")).ok()?;
        matches!(name.trim(), "coretemp" | "k10temp" | "zenpower").then(|| p.join("temp1_input"))
    })
}

fn parse_cpu(stat: &str) -> Option<(u64, u64)> {
    let f: Vec<u64> = stat.lines().next()?.split_whitespace().skip(1).filter_map(|v| v.parse().ok()).collect();
    let idle = f.get(3)? + f.get(4).unwrap_or(&0);
    Some((f.iter().take(8).sum(), idle))
}

fn parse_mem(src: &str) -> Option<(f64, f64)> {
    let field = |key: &str| -> Option<f64> {
        src.lines().find(|l| l.starts_with(key))?.split_whitespace().nth(1)?.parse().ok()
    };
    let (total, avail) = (field("MemTotal:")?, field("MemAvailable:")?);
    Some(((total - avail) * 1024.0, total * 1024.0))
}

fn parse_net(src: &str) -> (u64, u64) {
    let mut sum = (0, 0);
    for line in src.lines().skip(2) {
        let Some((name, rest)) = line.split_once(':') else { continue };
        if name.trim() == "lo" {
            continue;
        }
        let f: Vec<u64> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
        if f.len() > 8 {
            sum = (sum.0 + f[0], sum.1 + f[8]);
        }
    }
    sum
}

fn battery() -> Option<(f64, bool)> {
    let dir = fs::read_dir("/sys/class/power_supply").ok()?.flatten().map(|d| d.path())
        .find(|p| fs::read_to_string(p.join("type")).is_ok_and(|t| t.trim() == "Battery"))?;
    let pct = fs::read_to_string(dir.join("capacity")).ok()?.trim().parse().ok()?;
    let status = fs::read_to_string(dir.join("status")).unwrap_or_default();
    Some((pct, status.trim() == "Charging"))
}

/// `island --gpu-sampler`: one line of "util temp" per line of stdin.
pub fn gpu_sampler() {
    let Some(nvml) = Nvml::open() else { return };
    let mut out = io::stdout().lock();
    for _ in io::stdin().lock().lines() {
        let Some((util, temp)) = nvml.read() else { return };
        if writeln!(out, "{util} {temp}").and_then(|_| out.flush()).is_err() {
            return;
        }
    }
}

struct Gpu {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Gpu {
    fn open() -> Option<Self> {
        let mut cmd = Command::new("/proc/self/exe");
        cmd.arg("--gpu-sampler").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        let mut child = cmd.spawn().ok()?;
        let (input, output) = (child.stdin.take()?, child.stdout.take()?);
        Some(Gpu { child, input, output: BufReader::new(output) })
    }

    fn read(&mut self) -> Option<(f64, f64)> {
        writeln!(self.input).ok()?;
        let mut line = String::new();
        self.output.read_line(&mut line).ok()?;
        let (util, temp) = line.trim().split_once(' ')?;
        Some((util.parse().ok()?, temp.parse().ok()?))
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub enum Cmd {
    Sample,
    /// No panel is open: end the GPU sampler and return freed memory.
    Release,
}

/// Parked on the channel while every panel is closed.
pub fn spawn(tx: crate::Tx) -> mpsc::Sender<Cmd> {
    let (cmd, cmds) = mpsc::channel();
    thread::spawn(move || {
        let mut s = Sampler::new();
        s.prime();
        let mut last = Snapshot::default();
        for c in cmds {
            match c {
                Cmd::Sample => {
                    last = s.sample(&last);
                    if tx.send(Msg::Sys(last.clone())).is_err() {
                        return;
                    }
                }
                Cmd::Release => {
                    s.gpu = None;
                    unsafe { libc::malloc_trim(0) };
                }
            }
        }
    });
    cmd
}

struct Sampler {
    temp: Option<PathBuf>,
    gpu: Option<Gpu>,
    /// Set once the sampler fails: no NVIDIA GPU here, so stop trying.
    no_gpu: bool,
    cpu: Option<(u64, u64)>,
    net: Option<((u64, u64), Instant)>,
}

impl Sampler {
    fn new() -> Self {
        Sampler { temp: cpu_temp_path(), gpu: None, no_gpu: false, cpu: None, net: None }
    }

    /// Take the baseline for rates, so the first panel shows a real number
    /// (the average since startup) instead of a tile that pops in a tick later.
    fn prime(&mut self) {
        self.cpu = fs::read_to_string("/proc/stat").ok().as_deref().and_then(parse_cpu);
        self.net = Some((parse_net(&fs::read_to_string("/proc/net/dev").unwrap_or_default()), Instant::now()));
    }

    fn sample(&mut self, last: &Snapshot) -> Snapshot {
        if self.gpu.is_none() && !self.no_gpu {
            self.gpu = Gpu::open();
        }
        let gpu = self.gpu.as_mut().and_then(Gpu::read);
        if gpu.is_none() {
            self.gpu = None;
            self.no_gpu = true;
        }

        let cpu_now = fs::read_to_string("/proc/stat").ok().as_deref().and_then(parse_cpu);
        let cpu = match (self.cpu, cpu_now) {
            (Some((t0, i0)), Some((t1, i1))) if t1 > t0 => {
                Some(100.0 * (1.0 - (i1 - i0) as f64 / (t1 - t0) as f64))
            }
            _ => last.cpu,
        };
        self.cpu = cpu_now;

        let net_now = parse_net(&fs::read_to_string("/proc/net/dev").unwrap_or_default());
        let now = Instant::now();
        let (rx, tx) = self.net.map_or((last.rx, last.tx), |((r0, t0), at)| {
            let dt = now.duration_since(at).as_secs_f64().max(0.001);
            (net_now.0.saturating_sub(r0) as f64 / dt, net_now.1.saturating_sub(t0) as f64 / dt)
        });
        self.net = Some((net_now, now));

        Snapshot {
            cpu,
            cpu_temp: self.temp.as_ref().and_then(|p| fs::read_to_string(p).ok())
                .and_then(|s| s.trim().parse::<f64>().ok()).map(|t| t / 1000.0),
            gpu: gpu.map(|g| g.0),
            gpu_temp: gpu.map(|g| g.1),
            mem: fs::read_to_string("/proc/meminfo").ok().as_deref().and_then(parse_mem),
            rx,
            tx,
            battery: battery(),
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsers_read_real_proc_formats() {
        assert_eq!(parse_cpu("cpu  10 0 10 70 10 0 0 0 0 0\ncpu0 1"), Some((100, 80)));
        assert_eq!(parse_mem("MemTotal: 1000 kB\nMemFree: 1 kB\nMemAvailable: 400 kB\n"),
                   Some((600.0 * 1024.0, 1000.0 * 1024.0)));
        let net = "Inter|\n face |\n    lo: 999 0 0 0 0 0 0 0 999 0\n wlan0: 100 1 0 0 0 0 0 0 50 1 0 0 0 0 0 0\n";
        assert_eq!(parse_net(net), (100, 50));
    }
}
