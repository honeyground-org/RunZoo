//! 시스템 측정. 다섯 가지 소스를 같은 모양(0~100%)으로 정규화해서
//! 어느 것이든 동물 속도의 기준으로 쓸 수 있게 한다.
use std::collections::VecDeque;
use std::path::Path;
use std::time::Instant;

use sysinfo::{Disks, Networks, ProcessesToUpdate, System};

/// 대시보드에 그릴 최근 표본 수 (1초 간격이므로 60초)
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
            Source::Memory => "메모리",
            Source::Disk => "디스크",
            Source::Network => "네트워크",
            Source::Battery => "배터리",
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
    /// 소스별 최근 값 (0~100)
    pub hist: Vec<VecDeque<f32>>,
    /// 소스별 사람이 읽을 설명. "34.2%" / "12.3 MB/s" 같은 것
    pub detail: Vec<String>,
    /// 소스를 실제로 읽을 수 있는지. 배터리 없는 맥이 있다.
    pub available: [bool; 5],
    pub top: Vec<Proc>,
    /// 처리량 계열은 절대 상한이 없어서, 최근에 본 최댓값 대비로 환산한다.
    net_peak: f64,
    disk_peak: f64,
    last: Instant,
    /// 첫 두 번은 버린다. refresh_processes 의 첫 호출은 그 초의 증분이 아니라
    /// 프로세스마다 살아온 내내의 누적 바이트를 돌려주기 때문이다.
    warmup: u8,
    /// 프로세스 열거는 이 앱에서 제일 비싼 일이다. 2초에 한 번만 한다.
    proc_tick: u8,
    last_proc: Instant,
    disk_pct: f32,
    disk_rate: f64,
    /// 용량은 초 단위로 변할 리 없다. 열 번에 한 번만 다시 잰다.
    capacity: f32,
    cap_tick: u8,
}

/// 누적값을 증분으로 착각했을 때만 걸리라고 둔 뒷문. 예열 건너뛰기가 본 방어이고
/// 이건 보조다. 이 맥에서 실측한 지속 쓰기가 8.0 GB/s 였으므로 (iostat 과 대조함)
/// 진짜 값이 기각되지 않도록 넉넉히 잡는다.
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

    /// 처리량(초당 바이트)을 0~100 으로 바꾼다. 절대 상한이 없는 값이라
    /// 최근에 본 최댓값 대비로 환산하고, 그 최댓값은 서서히 잊는다.
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

        // --- 메모리
        self.sys.refresh_memory();
        let (used, total) = (self.sys.used_memory(), self.sys.total_memory());
        let mem = if total > 0 { used as f32 / total as f32 * 100.0 } else { 0.0 };
        self.push(Source::Memory, mem);
        self.detail[Source::Memory.idx()] =
            format!("{mem:.0}% · {} / {}", human_bytes(used), human_bytes(total));

        // --- 프로세스: 범인 목록과 디스크 처리량을 한 번에 얻는다.
        // 한계: 두 번의 갱신 사이에 태어나 죽은 프로세스의 입출력은 사라진다.
        // 오래 사는 프로세스에 대해서는 iostat 과 일치하는 것을 확인했다.
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
        self.detail[Source::Disk.idx()] = format!("{} · 사용 {cap:.0}%", human_rate(disk_rate));

        // --- 네트워크
        self.nets.refresh(true);
        let mut bytes = 0u64;
        for (_, n) in self.nets.iter() {
            bytes += n.received() + n.transmitted();
        }
        let net_rate = if warming { 0.0 } else { bytes as f64 / dt };
        let pct = self.throughput(net_rate, |m| &mut m.net_peak);
        self.push(Source::Network, pct);
        self.detail[Source::Network.idx()] = human_rate(net_rate);

        // --- 배터리: 남은 양이 적을수록 부하가 높다고 본다 (다급하게 뛴다)
        if let Some(m) = &self.battery {
            if let Ok(mut it) = m.batteries() {
                if let Some(Ok(b)) = it.next() {
                    let soc = b.state_of_charge().value * 100.0;
                    self.available[Source::Battery.idx()] = true;
                    self.push(Source::Battery, 100.0 - soc);
                    self.detail[Source::Battery.idx()] =
                        format!("{soc:.0}% 남음 · {:?}", b.state());
                }
            }
        }
    }

    /// 부팅 볼륨 하나만 본다. APFS 는 여러 볼륨이 한 컨테이너를 나눠 쓰기 때문에
    /// 전부 더하면 같은 저장 공간을 두 번 세고, 꽂아둔 DMG 까지 섞인다.
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
