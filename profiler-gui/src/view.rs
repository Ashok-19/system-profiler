//! Turns sensor snapshots into display-ready text and bar percentages.
//!
//! Kept free of Slint types so the formatting can be unit-tested.

use sys_monitor::{ChargeStatus, GpuStats, PowerState, PowerStats, Snapshot};

use crate::history::{self, History, Sample};

/// Fans are drawn against this many RPM until a faster reading is seen.
const FAN_SCALE_FLOOR_RPM: u32 = 5000;
/// Package power is charted against this many watts until more is seen.
const POWER_SCALE_FLOOR_W: f32 = 60.0;
const CPU_TEMP_RANGE_C: (f32, f32) = (30.0, 100.0);
const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
const MIB: f64 = 1024.0 * 1024.0;
const MISSING: &str = "—";

#[derive(Debug, Clone, PartialEq)]
pub struct MetricView {
    pub label: String,
    pub value: String,
    /// 0-100.
    pub percent: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactView {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PanelView {
    pub title: String,
    pub bars: Vec<MetricView>,
    pub rows: Vec<FactView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BatteryView {
    /// Line under the percentage, e.g. "On battery · 3 h 20 min left".
    pub status: String,
    pub facts: Vec<FactView>,
}

/// One System Monitor chart. `line`/`area` are path commands on a
/// 300 x 100 viewbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartView {
    pub title: String,
    pub now: String,
    pub min: String,
    pub max: String,
    pub line: String,
    pub area: String,
}

/// One row of the "All Sensors" list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SensorView {
    pub name: String,
    /// Driver or device, e.g. `k10temp`.
    pub source: String,
    pub value: String,
}

/// Everything the monitor thread refreshes each second.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveView {
    /// CPU, GPU, Fan, Memory, in that order.
    pub panels: Vec<PanelView>,
    /// -1 when there is no battery.
    pub battery_percent: i32,
    pub on_ac: bool,
    pub battery: BatteryView,
    /// CPU temperature, CPU usage, package power, CPU fan.
    pub charts: Vec<ChartView>,
    pub sensors: Vec<SensorView>,
}

fn metric(label: &str, value: String, percent: f32) -> MetricView {
    MetricView {
        label: label.to_owned(),
        value,
        percent: percent.clamp(0.0, 100.0),
    }
}

fn fact(key: &str, value: String) -> FactView {
    FactView {
        key: key.to_owned(),
        value,
    }
}

/// `c` in °C, shown in °F when `fahrenheit`.
fn temp(c: Option<f32>, fahrenheit: bool) -> String {
    match c {
        None => MISSING.to_owned(),
        Some(v) if fahrenheit => format!("{:.0} °F", v * 9.0 / 5.0 + 32.0),
        Some(v) => format!("{v:.0} °C"),
    }
}

#[allow(clippy::cast_precision_loss)] // display only
fn ratio(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        0.0
    } else {
        part as f32 / whole as f32 * 100.0
    }
}

#[allow(clippy::cast_precision_loss)] // display only
fn gib(bytes: u64) -> f64 {
    bytes as f64 / GIB
}

#[allow(clippy::cast_precision_loss)] // display only
fn mib(bytes: u64) -> f64 {
    bytes as f64 / MIB
}

/// "3 h 20 min", or "45 min" under an hour.
fn duration(hours: f32) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped
    let minutes = (hours * 60.0).round().clamp(0.0, 6000.0) as u32;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

fn battery(power: &PowerStats) -> BatteryView {
    let Some(b) = &power.battery else {
        return BatteryView {
            status: "No battery found".to_owned(),
            facts: Vec::new(),
        };
    };
    // Estimates swing wildly at very low power; hide implausible ones.
    let left = b.hours_remaining().filter(|&h| h < 48.0).map(duration);
    let status = match b.status {
        ChargeStatus::Charging => left.map_or_else(
            || "Charging".to_owned(),
            |t| format!("Charging · full in {t}"),
        ),
        ChargeStatus::Discharging => left.map_or_else(
            || "On battery".to_owned(),
            |t| format!("On battery · {t} left"),
        ),
        ChargeStatus::Full => "Fully charged · on AC power".to_owned(),
        ChargeStatus::NotCharging => b.charge_limit_percent.filter(|&l| l < 100).map_or_else(
            || "Plugged in, not charging".to_owned(),
            |l| format!("Held at the {l}% charge limit · on AC power"),
        ),
        ChargeStatus::Unknown => if power.ac_online.unwrap_or(true) {
            "On AC power"
        } else {
            "On battery"
        }
        .to_owned(),
    };

    let mut facts = vec![
        fact(
            "Battery Health",
            match (
                b.health_percent(),
                b.energy_full_wh,
                b.energy_full_design_wh,
            ) {
                (Some(h), Some(full), Some(design)) => {
                    format!("{h:.0}% ({full:.1} of {design:.1} Wh)")
                }
                _ => MISSING.to_owned(),
            },
        ),
        fact(
            "Charge Limit",
            b.charge_limit_percent
                .map_or_else(|| "Not supported".to_owned(), |l| format!("{l}%")),
        ),
    ];
    let rate = match b.status {
        ChargeStatus::Charging => Some("Charge Rate"),
        ChargeStatus::Discharging => Some("Power Draw"),
        _ => None,
    };
    if let (Some(key), Some(w)) = (rate, b.power_w.filter(|&w| w > 0.1)) {
        facts.push(fact(key, format!("{w:.1} W")));
    }
    facts.push(fact(
        "Charge Cycles",
        // Many firmwares, including ASUS', always report 0.
        b.cycle_count
            .filter(|&c| c > 0)
            .map_or_else(|| "Not reported".to_owned(), |c| c.to_string()),
    ));
    if let Some(v) = b.voltage_v {
        facts.push(fact("Voltage", format!("{v:.2} V")));
    }
    let pack: Vec<&str> = [
        b.model.as_deref(),
        b.technology.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !pack.is_empty() {
        facts.push(fact("Battery Pack", pack.join(" · ")));
    }
    BatteryView { status, facts }
}

/// `cpu_fan` -> `CPU Fan`, `gpu_fan` -> `GPU Fan`, `fan3` -> `Fan 3`.
fn fan_label(raw: &str) -> String {
    raw.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            if matches!(w, "cpu" | "gpu" | "sys") {
                return w.to_uppercase();
            }
            let (letters, digits) =
                w.split_at(w.find(|c: char| c.is_ascii_digit()).unwrap_or(w.len()));
            let mut word = capitalize(letters);
            if !digits.is_empty() {
                word.push(' ');
                word.push_str(digits);
            }
            word
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The GPU driving the built-in screen (boot VGA), else the first non-NVIDIA.
fn integrated(gpus: &[GpuStats]) -> Option<&GpuStats> {
    gpus.iter()
        .find(|g| g.boot_vga)
        .or_else(|| gpus.iter().find(|g| g.driver.as_deref() != Some("nvidia")))
}

/// Human-readable state of the discrete GPU.
fn discrete_state(gpus: &[GpuStats]) -> String {
    let dgpu = gpus
        .iter()
        .find(|g| !g.boot_vga && g.driver.as_deref() == Some("nvidia"))
        .or_else(|| gpus.iter().find(|g| !g.boot_vga));
    match dgpu.map(|g| g.power_state) {
        Some(PowerState::Active) => "Active",
        Some(PowerState::Suspended) => "Sleeping",
        Some(PowerState::Unknown) => "Waking",
        None => "Not present",
    }
    .to_owned()
}

/// Package power of an AMD APU: the iGPU's PPT sensor covers the package.
fn package_power(s: &Snapshot) -> Option<f32> {
    integrated(&s.gpus)
        .filter(|g| g.driver.as_deref() == Some("amdgpu"))
        .and_then(|g| g.power_w)
}

/// The fan labelled `cpu_fan`, else the first one.
fn cpu_fan_rpm(s: &Snapshot) -> Option<u32> {
    s.fans
        .iter()
        .find(|f| f.label == "cpu_fan")
        .or_else(|| s.fans.first())
        .map(|f| f.rpm)
}

/// Friendly name for a temperature sensor; `drive` numbers `NVMe` drives.
fn sensor_name(chip: &str, label: &str, drive: usize) -> String {
    match (chip, label) {
        ("k10temp" | "zenpower", l) => format!("CPU ({l})"),
        ("coretemp", l) => format!("CPU {l}"),
        ("amdgpu", l) => format!("Radeon ({l})"),
        ("acpitz", _) => "ACPI thermal zone".to_owned(),
        ("nvme", "Composite") => format!("Drive {drive}"),
        ("nvme", l) => format!("Drive {drive} {}", l.to_lowercase()),
        (c, _) if c.starts_with("mt7") || c.starts_with("iwlwifi") || c.starts_with("ath") => {
            "Wi-Fi".to_owned()
        }
        (_, "temp1") => capitalize(chip),
        (_, l) => capitalize(l),
    }
}

fn sensors(s: &Snapshot, fahrenheit: bool) -> Vec<SensorView> {
    let mut drive = 0;
    let mut rows: Vec<SensorView> = s
        .temps
        .iter()
        .map(|t| {
            // Each NVMe drive reports "Composite" first.
            if t.chip == "nvme" && t.label == "Composite" {
                drive += 1;
            }
            SensorView {
                name: sensor_name(&t.chip, &t.label, drive),
                source: t.chip.clone(),
                value: temp(Some(t.temp_c), fahrenheit),
            }
        })
        .collect();
    rows.extend(s.fans.iter().map(|f| SensorView {
        name: fan_label(&f.label),
        source: f.chip.clone(),
        value: format!("{} RPM", f.rpm),
    }));
    if let Some(w) = package_power(s) {
        rows.push(SensorView {
            name: "Package power".to_owned(),
            source: "amdgpu".to_owned(),
            value: format!("{w:.1} W"),
        });
    }
    if let Some(b) = &s.power.battery {
        let volts = b
            .voltage_v
            .map_or_else(|| MISSING.to_owned(), |v| format!("{v:.2} V"));
        let watts = b.power_w.unwrap_or(0.0);
        rows.push(SensorView {
            name: "Battery".to_owned(),
            source: b.name.clone(),
            value: format!("{volts} · {watts:.1} W"),
        });
    }
    rows
}

fn chart(
    title: &str,
    values: &[f32],
    window: usize,
    (lo, hi): (f32, f32),
    fmt: impl Fn(f32) -> String,
    now: Option<f32>,
) -> ChartView {
    let (line, area) = history::chart_paths(values, window, lo, hi);
    ChartView {
        title: title.to_owned(),
        now: now.map_or_else(|| MISSING.to_owned(), &fmt),
        min: fmt(lo),
        max: fmt(hi),
        line,
        area,
    }
}

/// Builds the live views; remembers scales and history between samples.
#[derive(Debug)]
pub struct Formatter {
    fan_max_rpm: u32,
    power_max_w: f32,
    history: History,
    /// Chart window in minutes.
    pub window_minutes: u32,
    /// Seconds between samples, to size the chart window.
    pub interval_s: u32,
    pub fahrenheit: bool,
}

impl Default for Formatter {
    fn default() -> Self {
        Self {
            fan_max_rpm: FAN_SCALE_FLOOR_RPM,
            power_max_w: POWER_SCALE_FLOOR_W,
            history: History::default(),
            window_minutes: 1,
            interval_s: 1,
            fahrenheit: false,
        }
    }
}

impl Formatter {
    pub fn live(&mut self, s: &Snapshot) -> LiveView {
        let f = self.fahrenheit;
        let panels = vec![
            Self::cpu(s, f),
            Self::gpu(s, f),
            self.fans(s),
            Self::memory(s, f),
        ];
        LiveView {
            panels,
            charts: self.charts(s),
            sensors: sensors(s, f),
            battery_percent: s
                .power
                .battery
                .as_ref()
                .and_then(|b| b.capacity_percent)
                .map_or(-1, i32::from),
            on_ac: s.power.ac_online.unwrap_or(true),
            battery: battery(&s.power),
        }
    }

    fn charts(&mut self, s: &Snapshot) -> Vec<ChartView> {
        let package_w = package_power(s);
        let fan = cpu_fan_rpm(s);
        #[allow(clippy::cast_precision_loss)] // RPM values are small
        let fan_f = fan.map(|r| r as f32);
        self.history.push(Sample {
            cpu_temp_c: s.cpu.temp_c.unwrap_or(0.0),
            cpu_usage: s.cpu.usage_percent.unwrap_or(0.0),
            package_w: package_w.unwrap_or(0.0),
            fan_rpm: fan_f.unwrap_or(0.0),
        });
        if let Some(w) = package_w {
            // Grow the scale in steps of 10 W so it does not jitter.
            self.power_max_w = self.power_max_w.max((w / 10.0).ceil() * 10.0);
        }
        let samples = self.window_minutes.max(1) * 60 / self.interval_s.max(1);
        let window = usize::try_from(samples).unwrap_or(60);
        let (h, f) = (&self.history, self.fahrenheit);
        #[allow(clippy::cast_precision_loss)]
        let fan_max = self.fan_max_rpm as f32;
        vec![
            chart(
                "CPU Temperature",
                &h.series(window, |x| x.cpu_temp_c),
                window,
                CPU_TEMP_RANGE_C,
                |v| temp(Some(v), f),
                s.cpu.temp_c,
            ),
            chart(
                "CPU Usage",
                &h.series(window, |x| x.cpu_usage),
                window,
                (0.0, 100.0),
                |v| format!("{v:.0} %"),
                s.cpu.usage_percent,
            ),
            chart(
                "Package Power",
                &h.series(window, |x| x.package_w),
                window,
                (0.0, self.power_max_w),
                |v| format!("{v:.0} W"),
                package_w,
            ),
            chart(
                "CPU Fan",
                &h.series(window, |x| x.fan_rpm),
                window,
                (0.0, fan_max),
                |v| format!("{v:.0} RPM"),
                fan_f,
            ),
        ]
    }

    fn cpu(s: &Snapshot, f: bool) -> PanelView {
        let c = &s.cpu;
        let freq = c.avg_freq_mhz();
        let freq_pct = match (freq, c.rated_max_mhz) {
            (Some(f), Some(max)) => ratio(u64::from(f), u64::from(max)),
            _ => 0.0,
        };
        let mut rows = vec![fact(
            "Temperature",
            temp(c.temp_c, f),
        )];
        // On AMD APUs the iGPU's PPT sensor reports the whole package.
        if let Some(w) = package_power(s) {
            rows.push(fact("Package Power", format!("{w:.0} W")));
        }
        rows.push(fact("Threads", c.cores.len().to_string()));
        PanelView {
            title: "CPU".to_owned(),
            bars: vec![
                metric(
                    "Frequency",
                    freq.map_or_else(|| MISSING.to_owned(), |f| format!("{f} MHz")),
                    freq_pct,
                ),
                metric(
                    "Usage",
                    c.usage_percent
                        .map_or_else(|| MISSING.to_owned(), |u| format!("{u:.0} %")),
                    c.usage_percent.unwrap_or(0.0),
                ),
            ],
            rows,
        }
    }

    fn gpu(s: &Snapshot, f: bool) -> PanelView {
        let igpu = integrated(&s.gpus);
        let usage = igpu.and_then(|g| g.usage_percent);
        let vram = igpu.and_then(|g| Some((g.vram_used_bytes?, g.vram_total_bytes?)));
        PanelView {
            title: "GPU".to_owned(),
            bars: vec![
                metric(
                    "iGPU Usage",
                    usage.map_or_else(|| MISSING.to_owned(), |u| format!("{u:.0} %")),
                    usage.unwrap_or(0.0),
                ),
                metric(
                    "iGPU Memory",
                    vram.map_or_else(
                        || MISSING.to_owned(),
                        |(used, total)| format!("{:.0} / {:.0} MiB", mib(used), mib(total)),
                    ),
                    vram.map_or(0.0, |(used, total)| ratio(used, total)),
                ),
            ],
            rows: vec![
                fact(
                    "iGPU Frequency",
                    igpu.and_then(|g| g.clock_mhz)
                        .map_or_else(|| MISSING.to_owned(), |f| format!("{f} MHz")),
                ),
                fact("iGPU Temperature", temp(igpu.and_then(|g| g.temp_c), f)),
                fact("dGPU", discrete_state(&s.gpus)),
            ],
        }
    }

    fn fans(&mut self, s: &Snapshot) -> PanelView {
        let fastest = s.fans.iter().map(|f| f.rpm).max().unwrap_or(0);
        self.fan_max_rpm = self.fan_max_rpm.max(fastest);
        let scale = u64::from(self.fan_max_rpm);
        PanelView {
            title: "Fan".to_owned(),
            bars: s
                .fans
                .iter()
                .map(|f| {
                    metric(
                        &fan_label(&f.label),
                        format!("{} RPM", f.rpm),
                        ratio(u64::from(f.rpm), scale),
                    )
                })
                .collect(),
            rows: Vec::new(),
        }
    }

    fn memory(s: &Snapshot, f: bool) -> PanelView {
        let mut bars = Vec::new();
        if let Some(m) = &s.memory {
            bars.push(metric(
                "RAM",
                format!("{:.1} / {:.1} GiB", gib(m.used_bytes()), gib(m.total_bytes)),
                ratio(m.used_bytes(), m.total_bytes),
            ));
            if m.swap_total_bytes > 0 {
                bars.push(metric(
                    "Swap",
                    format!(
                        "{:.1} / {:.1} GiB",
                        gib(m.swap_used_bytes()),
                        gib(m.swap_total_bytes)
                    ),
                    ratio(m.swap_used_bytes(), m.swap_total_bytes),
                ));
            }
        }
        let rows = s
            .temps
            .iter()
            .filter(|t| t.chip == "nvme" && t.label == "Composite")
            .enumerate()
            .map(|(i, t)| fact(&format!("Drive {}", i + 1), temp(Some(t.temp_c), f)))
            .collect();
        PanelView {
            title: "Memory".to_owned(),
            bars,
            rows,
        }
    }
}

#[cfg(test)]
mod tests {
    use sys_monitor::{
        BatteryStats, ChargeStatus, CoreStats, CpuStats, FanStats, MemoryStats, PowerStats,
        TempSensor,
    };

    use super::*;

    fn gpu(card: &str, driver: &str, boot_vga: bool, state: PowerState) -> GpuStats {
        GpuStats {
            card: card.to_owned(),
            driver: Some(driver.to_owned()),
            pci_address: None,
            vendor_id: None,
            device_id: None,
            boot_vga,
            power_state: state,
            usage_percent: None,
            clock_mhz: None,
            temp_c: None,
            power_w: None,
            vram_used_bytes: None,
            vram_total_bytes: None,
        }
    }

    fn snapshot() -> Snapshot {
        let mut igpu = gpu("card2", "amdgpu", true, PowerState::Active);
        igpu.usage_percent = Some(21.0);
        igpu.clock_mhz = Some(1600);
        igpu.temp_c = Some(70.0);
        igpu.power_w = Some(54.0);
        igpu.vram_used_bytes = Some(256 * 1024 * 1024);
        igpu.vram_total_bytes = Some(512 * 1024 * 1024);
        Snapshot {
            cpu: CpuStats {
                model: Some("AMD Ryzen 7 4800H with Radeon Graphics".to_owned()),
                usage_percent: Some(52.4),
                temp_c: Some(91.9),
                rated_max_mhz: Some(4300),
                cores: vec![
                    CoreStats {
                        id: 0,
                        usage_percent: None,
                        freq_mhz: Some(4300),
                    },
                    CoreStats {
                        id: 1,
                        usage_percent: None,
                        freq_mhz: Some(2150),
                    },
                ],
            },
            gpus: vec![
                gpu("card1", "nvidia", false, PowerState::Suspended),
                igpu,
            ],
            memory: Some(MemoryStats {
                total_bytes: 16 * 1024 * 1024 * 1024,
                available_bytes: 4 * 1024 * 1024 * 1024,
                swap_total_bytes: 0,
                swap_free_bytes: 0,
            }),
            power: PowerStats {
                ac_online: Some(false),
                battery: Some(BatteryStats {
                    name: "BAT1".to_owned(),
                    status: ChargeStatus::Discharging,
                    capacity_percent: Some(80),
                    power_w: None,
                    energy_now_wh: None,
                    energy_full_wh: None,
                    energy_full_design_wh: None,
                    cycle_count: None,
                    charge_limit_percent: None,
                    voltage_v: None,
                    model: None,
                    technology: None,
                }),
            },
            fans: vec![
                FanStats {
                    chip: "asus".to_owned(),
                    label: "cpu_fan".to_owned(),
                    rpm: 2500,
                },
                FanStats {
                    chip: "asus".to_owned(),
                    label: "gpu_fan".to_owned(),
                    rpm: 6000,
                },
            ],
            temps: vec![
                TempSensor {
                    chip: "nvme".to_owned(),
                    label: "Composite".to_owned(),
                    temp_c: 46.85,
                },
                TempSensor {
                    chip: "nvme".to_owned(),
                    label: "Sensor 1".to_owned(),
                    temp_c: 54.0,
                },
            ],
        }
    }

    fn row<'a>(p: &'a PanelView, key: &str) -> &'a str {
        &p.rows.iter().find(|r| r.key == key).unwrap().value
    }

    #[test]
    fn home_view() {
        let v = Formatter::default().live(&snapshot());
        assert_eq!(v.battery_percent, 80);
        assert!(!v.on_ac);
        let titles: Vec<&str> = v.panels.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "CPU", "GPU", "Fan", "Memory"
            ]
        );

        let cpu = &v.panels[0];
        assert_eq!(cpu.bars[0].value, "3225 MHz");
        assert!((cpu.bars[0].percent - 75.0).abs() < 0.1);
        assert_eq!(cpu.bars[1].value, "52 %");
        assert_eq!(row(cpu, "Temperature"), "92 °C");
        assert_eq!(row(cpu, "Package Power"), "54 W");
        assert_eq!(row(cpu, "Threads"), "2");

        let gpu = &v.panels[1];
        assert_eq!(gpu.bars[1].value, "256 / 512 MiB");
        assert!((gpu.bars[1].percent - 50.0).abs() < 0.1);
        assert_eq!(row(gpu, "dGPU"), "Sleeping");

        let fans = &v.panels[2];
        assert_eq!(fans.bars[0].label, "CPU Fan");
        // The 6000 RPM reading raises the scale above the 5000 floor.
        assert!((fans.bars[1].percent - 100.0).abs() < 0.1);

        let mem = &v.panels[3];
        assert_eq!(mem.bars.len(), 1, "no swap bar without swap");
        assert_eq!(mem.bars[0].value, "12.0 / 16.0 GiB");
        assert_eq!(row(mem, "Drive 1"), "47 °C");
    }

    #[test]
    fn missing_sensors() {
        let mut s = snapshot();
        s.cpu = CpuStats::default();
        s.gpus.clear();
        s.power = PowerStats::default();
        let v = Formatter::default().live(&s);
        assert_eq!(v.battery_percent, -1);
        assert!(v.on_ac);
        assert_eq!(v.panels[0].bars[0].value, "—");
        assert_eq!(row(&v.panels[1], "dGPU"), "Not present");
    }

    #[test]
    fn battery_discharging() {
        let mut s = snapshot();
        let b = s.power.battery.as_mut().unwrap();
        b.power_w = Some(15.0);
        b.energy_now_wh = Some(50.0);
        b.energy_full_wh = Some(64.5);
        b.energy_full_design_wh = Some(90.0);
        b.cycle_count = Some(0);
        b.charge_limit_percent = Some(80);
        b.voltage_v = Some(16.441);
        b.model = Some("A32-K55".to_owned());
        b.technology = Some("Li-ion".to_owned());
        let v = Formatter::default().live(&s).battery;
        assert_eq!(v.status, "On battery · 3 h 20 min left");
        let keys: Vec<(&str, &str)> = v
            .facts
            .iter()
            .map(|f| (f.key.as_str(), f.value.as_str()))
            .collect();
        assert_eq!(
            keys,
            [
                ("Battery Health", "72% (64.5 of 90.0 Wh)"),
                ("Charge Limit", "80%"),
                ("Power Draw", "15.0 W"),
                ("Charge Cycles", "Not reported"),
                ("Voltage", "16.44 V"),
                ("Battery Pack", "A32-K55 · Li-ion"),
            ]
        );
    }

    #[test]
    fn battery_status_lines() {
        let mut s = snapshot();
        let b = s.power.battery.as_mut().unwrap();
        b.status = ChargeStatus::NotCharging;
        b.charge_limit_percent = Some(80);
        assert_eq!(
            Formatter::default().live(&s).battery.status,
            "Held at the 80% charge limit · on AC power"
        );
        let b = s.power.battery.as_mut().unwrap();
        b.status = ChargeStatus::Charging;
        assert_eq!(Formatter::default().live(&s).battery.status, "Charging");
        s.power.battery = None;
        let v = Formatter::default().live(&s).battery;
        assert_eq!(v.status, "No battery found");
        assert!(v.facts.is_empty());
    }

    #[test]
    fn durations() {
        assert_eq!(duration(0.75), "45 min");
        assert_eq!(duration(2.0), "2 h");
        assert_eq!(duration(3.0 + 1.0 / 3.0), "3 h 20 min");
    }

    #[test]
    fn charts_and_sensors() {
        let mut f = Formatter::default();
        f.live(&snapshot());
        let v = f.live(&snapshot());
        let titles: Vec<&str> = v.charts.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "CPU Temperature", "CPU Usage", "Package Power", "CPU Fan"
            ]
        );
        assert_eq!(v.charts[0].now, "92 °C");
        assert_eq!(v.charts[0].min, "30 °C");
        assert_eq!(v.charts[3].max, "6000 RPM", "scale follows the fastest fan");
        assert!(v.charts[1].line.starts_with('M'));

        let names: Vec<(&str, &str)> = v
            .sensors
            .iter()
            .map(|s| (s.name.as_str(), s.value.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("Drive 1", "47 °C"),
                ("Drive 1 sensor 1", "54 °C"),
                ("CPU Fan", "2500 RPM"),
                ("GPU Fan", "6000 RPM"),
                ("Package power", "54.0 W"),
                ("Battery", "— · 0.0 W"),
            ]
        );
    }

    #[test]
    fn fahrenheit() {
        assert_eq!(temp(Some(100.0), true), "212 °F");
        assert_eq!(temp(Some(30.0), false), "30 °C");
        let mut f = Formatter {
            fahrenheit: true,
            ..Formatter::default()
        };
        let v = f.live(&snapshot());
        assert_eq!(v.charts[0].min, "86 °F");
        assert_eq!(row(&v.panels[0], "Temperature"), "197 °F");
    }

    #[test]
    fn sensor_names() {
        assert_eq!(sensor_name("k10temp", "Tctl", 0), "CPU (Tctl)");
        assert_eq!(sensor_name("amdgpu", "edge", 0), "Radeon (edge)");
        assert_eq!(sensor_name("mt7921_phy0", "temp1", 0), "Wi-Fi");
        assert_eq!(sensor_name("nvme", "Composite", 2), "Drive 2");
        assert_eq!(sensor_name("acpitz", "temp1", 0), "ACPI thermal zone");
        assert_eq!(sensor_name("thinkpad", "temp1", 0), "Thinkpad");
    }

    #[test]
    fn fan_labels() {
        assert_eq!(fan_label("cpu_fan"), "CPU Fan");
        assert_eq!(fan_label("gpu_fan"), "GPU Fan");
        assert_eq!(fan_label("fan3"), "Fan 3");
        assert_eq!(fan_label("mid_fan"), "Mid Fan");
    }
}
