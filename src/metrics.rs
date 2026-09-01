//! System measurement. Five sources normalised to the same shape (0..100%) so
//! that any of them can drive the animal's speed - and, since the colour ramp
//! reads the same number, its severity colour too.
use std::collections::VecDeque;
use std::path::Path;
use std::time::Instant;

use sysinfo::{Disks, Networks, ProcessesToUpdate, System};

/// How many recent samples the dashboard draws (one per second, so 60s)
pub const HISTORY: usize = 60;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Source {
    Cpu,
    Memory,
    Disk,
    Network,
    Battery,
}

impl Source {
    pub const ALL: [Source; 5] = [
        Source::Cpu,
        Source::Memory,
        Source::Disk,
        Source::Network,
        Source::Battery,
    ];

    pub fn idx(self) -> usize {
        match self {
            Source::Cpu => 0,
            Source::Memory => 1,
            Source::Disk => 2,
            Source::Network => 3,
            Source::Battery => 4,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Source::Cpu => "cpu",
            Source::Memory => "memory",
            Source::Disk => "disk",
            Source::Network => "network",
            Source::Battery => "battery",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Source::Cpu => "CPU",
            Source::Memory => "Memory",
            Source::Disk => "Disk",
            Source::Network => "Network",
            Source::Battery => "Battery",
        }
    }

    pub fn from_key(k: &str) -> Option<Source> {
        Source::ALL.into_iter().find(|s| s.key() == k)
    }
}

pub struct Proc {
    pub name: String,
    pub cpu: f32,
    pub mem: u64,
}

pub struct Metrics {
    sys: System,
    disks: Disks,
    nets: Networks,
    battery: Option<starship_battery::Manager>,
    /// Recent values per source (0..100)
    pub hist: Vec<VecDeque<f32>>,
    /// A human-readable detail per source: "34.2%", "12.3 MB/s" and so on
    pub detail: Vec<String>,
    /// Whether the source can actually be read. Some Macs have no battery.
    pub available: [bool; 5],
    pub top: Vec<Proc>,
    /// Throughput has no absolute ceiling, so we scale it against the highest
    /// rate seen recently.
    net_peak: f64,
    disk_peak: f64,
    last: Instant,
    /// Throw the first two rounds away: the first refresh_processes call
    /// reports each process's lifetime byte total, not that second's delta.
    warmup: u8,
    /// Enumerating processes is the most expensive thing this app does.
    /// Once every two seconds is enough.
    proc_tick: u8,
    last_proc: Instant,
    disk_pct: f32,
    disk_rate: f64,
    /// Disk capacity does not move by the second. Re-measure every tenth tick.
    capacity: f32,
    cap_tick: u8,
}

/// A back stop that should only ever catch a lifetime total mistaken for a
/// delta. Skipping the warm-up rounds is the real defence; this is the spare.
/// Sustained writes measured 8.0 GB/s on this Mac (checked against iostat), so
/// the ceiling is set well clear of any real reading.
const RATE_SANITY_CEILING: f64 = 64.0 * 1024.0 * 1024.0 * 1024.0;

fn human_rate(bytes_per_sec: f64) -> String {
    const U: [&str; 4] = ["B/s", "KB/s", "MB/s", "GB/s"];
    let mut v = bytes_per_sec;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.1} {}", U[i])
}

fn human_bytes(bytes: u64) -> String {
    const U: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.1} {}", U[i])
}

impl Metrics {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        Metrics {
            sys,
            disks: Disks::new_with_refreshed_list(),
            nets: Networks::new_with_refreshed_list(),
            battery: starship_battery::Manager::new().ok(),
            hist: (0..5).map(|_| VecDeque::with_capacity(HISTORY)).collect(),
            detail: vec![String::from("—"); 5],
            available: [true, true, true, true, false],
            top: Vec::new(),
            net_peak: 1024.0 * 1024.0,
            disk_peak: 1024.0 * 1024.0,
            last: Instant::now(),
            warmup: 2,
            proc_tick: 0,
            last_proc: Instant::now(),
            disk_pct: 0.0,
            disk_rate: 0.0,
            capacity: 0.0,
            cap_tick: 0,
        }
    }

    fn push(&mut self, s: Source, v: f32) {
        let h = &mut self.hist[s.idx()];
        if h.len() == HISTORY {
            h.pop_front();
        }
        h.push_back(v.clamp(0.0, 100.0));
    }

    pub fn latest(&self, s: Source) -> f32 {
        self.hist[s.idx()].back().copied().unwrap_or(0.0)
    }

    /// Turn a throughput (bytes per second) into 0..100. There is no absolute
    /// ceiling for it, so scale against the highest rate seen recently and let
    /// that peak decay away.
    fn throughput(&mut self, rate: f64, peak: fn(&mut Metrics) -> &mut f64) -> f32 {
        let p = peak(self);
        *p *= 0.98;
        if *p < 1024.0 * 1024.0 {
            *p = 1024.0 * 1024.0;
        }
        if rate < RATE_SANITY_CEILING && *p < rate {
            *p = rate;
        }
        let p = *p;
        (rate / p * 100.0) as f32
    }

    pub fn refresh(&mut self) {
        let dt = self.last.elapsed().as_secs_f64().max(0.05);
        self.last = Instant::now();
        let warming = self.warmup > 0;
        self.warmup = self.warmup.saturating_sub(1);

        // --- CPU
        self.sys.refresh_cpu_usage();
        let cpu = self.sys.global_cpu_usage();
        self.push(Source::Cpu, cpu);
        self.detail[Source::Cpu.idx()] = format!("{cpu:.1}%");

        // --- Memory
        self.sys.refresh_memory();
        let (used, total) = (self.sys.used_memory(), self.sys.total_memory());
        let mem = if total > 0 { used as f32 / total as f32 * 100.0 } else { 0.0 };
        self.push(Source::Memory, mem);
        self.detail[Source::Memory.idx()] =
            format!("{mem:.0}% · {} / {}", human_bytes(used), human_bytes(total));

        // --- Processes: the culprit list and disk throughput in one pass.
        // Limit: I/O from a process born and dead between two refreshes is lost.
        // For long-lived processes this was verified to agree with iostat.
        if self.proc_tick == 0 {
            let pdt = self.last_proc.elapsed().as_secs_f64().max(0.05);
            self.last_proc = Instant::now();
            self.sys.refresh_processes(ProcessesToUpdate::All, true);
            let mut read = 0u64;
            let mut written = 0u64;
            let mut procs: Vec<Proc> = Vec::new();
            for p in self.sys.processes().values() {
                let d = p.disk_usage();
                read += d.read_bytes;
                written += d.written_bytes;
                procs.push(Proc {
                    name: p.name().to_string_lossy().into_owned(),
                    cpu: p.cpu_usage(),
                    mem: p.memory(),
                });
            }
            procs.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
            procs.truncate(5);
            self.top = procs;
            self.disk_rate = if warming { 0.0 } else { (read + written) as f64 / pdt };
            self.disk_pct = self.throughput(self.disk_rate, |m| &mut m.disk_peak);
        }
        self.proc_tick = (self.proc_tick + 1) % 2;
        let disk_rate = self.disk_rate;
        let pct = self.disk_pct;
        self.push(Source::Disk, pct);
        let cap = self.disk_capacity();
        self.detail[Source::Disk.idx()] = format!("{} · {cap:.0}% used", human_rate(disk_rate));

        // --- Network
        self.nets.refresh(true);
        let mut bytes = 0u64;
        for n in self.nets.values() {
            bytes += n.received() + n.transmitted();
        }
        let net_rate = if warming { 0.0 } else { bytes as f64 / dt };
        let pct = self.throughput(net_rate, |m| &mut m.net_peak);
        self.push(Source::Network, pct);
        self.detail[Source::Network.idx()] = human_rate(net_rate);

        // --- Battery: the less is left, the higher the "load", so the animal
        // runs as if in a hurry - and the severity colour rises with it.
        if let Some(m) = &self.battery {
            if let Ok(mut it) = m.batteries() {
                if let Some(Ok(b)) = it.next() {
                    let soc = b.state_of_charge().value * 100.0;
                    self.available[Source::Battery.idx()] = true;
                    self.push(Source::Battery, 100.0 - soc);
                    self.detail[Source::Battery.idx()] =
                        format!("{soc:.0}% left · {:?}", b.state());
                }
            }
        }
    }

    /// Look at the boot volume only. Under APFS several volumes share one
    /// container, so adding them all up counts the same storage twice - and
    /// drags in any mounted DMG as well.
    fn disk_capacity(&mut self) -> f32 {
        if self.cap_tick == 0 {
            self.disks.refresh(false);
            let pick = self
                .disks
                .list()
                .iter()
                .find(|d| d.mount_point() == Path::new("/System/Volumes/Data"))
                .or_else(|| self.disks.list().iter().find(|d| d.mount_point() == Path::new("/")));
            self.capacity = match pick {
                Some(d) if d.total_space() > 0 => {
                    (d.total_space() - d.available_space()) as f32 / d.total_space() as f32 * 100.0
                }
                _ => 0.0,
            };
        }
        self.cap_tick = (self.cap_tick + 1) % 10;
        self.capacity
    }
}
