//! Hardware stats, read only while a panel is open. Anything this machine
//! doesn't have comes back None and its tile disappears.
use libloading::{Library, Symbol};
use std::{ffi::{c_int, c_uint, c_void}, fs, path::PathBuf, time::Instant};

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

/// NVML, opened on first use. nvidia-smi would cost a process per sample.
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

pub struct Sampler {
    temp: Option<PathBuf>,
    nvml: Option<Nvml>,
    tried_nvml: bool,
    cpu: Option<(u64, u64)>,
    net: Option<((u64, u64), Instant)>,
}

impl Sampler {
    pub fn new() -> Self {
        Sampler { temp: cpu_temp_path(), nvml: None, tried_nvml: false, cpu: None, net: None }
    }

    /// Take the baseline for rates, so the first panel shows a real number
    /// (the average since startup) instead of a tile that pops in a tick later.
    pub fn prime(&mut self) {
        self.cpu = fs::read_to_string("/proc/stat").ok().as_deref().and_then(parse_cpu);
        self.net = Some((parse_net(&fs::read_to_string("/proc/net/dev").unwrap_or_default()), Instant::now()));
    }

    pub fn sample(&mut self, last: &Snapshot) -> Snapshot {
        if !self.tried_nvml {
            self.tried_nvml = true;
            self.nvml = Nvml::open();
        }
        let gpu = self.nvml.as_ref().and_then(Nvml::read);

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
