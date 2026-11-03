//! CPU usage, clocks and package temperature.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::sysfs::{self, Hwmon};

/// Per logical CPU (hardware thread) readings.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreStats {
    /// Logical CPU number (`cpuN`).
    pub id: u32,
    /// Busy time since the previous sample, 0-100. `None` on the first sample.
    pub usage_percent: Option<f32>,
    /// Current clock in MHz, from cpufreq.
    pub freq_mhz: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CpuStats {
    /// Marketing name, e.g. "AMD Ryzen 7 4800H with Radeon Graphics".
    pub model: Option<String>,
    /// Overall busy time since the previous sample, 0-100.
    pub usage_percent: Option<f32>,
    /// Package/die temperature in °C.
    pub temp_c: Option<f32>,
    /// Highest clock the CPU is rated for (`cpuinfo_max_freq`), in MHz.
    pub rated_max_mhz: Option<u32>,
    pub cores: Vec<CoreStats>,
}

impl CpuStats {
    /// Mean of the known per-core clocks.
    #[must_use]
    pub fn avg_freq_mhz(&self) -> Option<u32> {
        let freqs: Vec<u64> = self
            .cores
            .iter()
            .filter_map(|c| c.freq_mhz.map(u64::from))
            .collect();
        let count = u64::try_from(freqs.len()).ok().filter(|&n| n > 0)?;
        u32::try_from(freqs.iter().sum::<u64>() / count).ok()
    }

    /// Highest per-core clock.
    #[must_use]
    pub fn max_freq_mhz(&self) -> Option<u32> {
        self.cores.iter().filter_map(|c| c.freq_mhz).max()
    }
}

/// Cumulative jiffies for one `/proc/stat` cpu line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Times {
    busy: u64,
    total: u64,
}

impl Times {
    /// Parse the numeric fields after the `cpu`/`cpuN` label.
    fn parse(fields: &[&str]) -> Option<Self> {
        // user nice system idle iowait irq softirq steal [guest guest_nice]
        // guest time is already counted in user/nice, so only the first 8.
        let vals: Vec<u64> = fields
            .iter()
            .take(8)
            .map(|f| f.parse().ok())
            .collect::<Option<_>>()?;
        if vals.len() < 4 {
            return None;
        }
        let total: u64 = vals.iter().sum();
        let idle = vals[3] + vals.get(4).copied().unwrap_or(0);
        Some(Self {
            busy: total.saturating_sub(idle),
            total,
        })
    }

    fn usage_since(self, prev: Self) -> Option<f32> {
        let total = self.total.checked_sub(prev.total)?;
        let busy = self.busy.checked_sub(prev.busy)?;
        if total == 0 {
            return None;
        }
        #[allow(clippy::cast_precision_loss)] // jiffy deltas are small
        let pct = busy as f32 / total as f32 * 100.0;
        Some(pct.clamp(0.0, 100.0))
    }
}

/// `None` key is the aggregate `cpu` line.
fn parse_proc_stat(text: &str) -> HashMap<Option<u32>, Times> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let label = fields.next()?;
            let rest = label.strip_prefix("cpu")?;
            let id = if rest.is_empty() {
                None
            } else {
                Some(rest.parse().ok()?)
            };
            let values: Vec<&str> = fields.collect();
            Some((id, Times::parse(&values)?))
        })
        .collect()
}

fn read_model(root: &Path) -> Option<String> {
    let text = sysfs::read_trimmed(&root.join("proc/cpuinfo"))?;
    text.lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.trim().to_owned())
}

/// CPU package temperature from the first matching driver.
fn read_temp(chips: &[Hwmon]) -> Option<f32> {
    // (hwmon name, preferred labels). Tdie is the real die temperature on
    // AMD; Tctl may carry a fan-control offset on some parts.
    const AMD: &[&str] = &[
        "Tdie", "Tctl",
    ];
    const INTEL: &[&str] = &["Package id 0"];
    const SOURCES: &[(&str, &[&str])] = &[
        ("k10temp", AMD),
        ("zenpower", AMD),
        ("coretemp", INTEL),
    ];
    for (name, labels) in SOURCES {
        let Some(chip) = sysfs::hwmon_named(chips, name) else {
            continue;
        };
        let indices = chip.input_indices("temp");
        let by_label = labels.iter().find_map(|want| {
            indices
                .iter()
                .find(|i| chip.read_label("temp", i).as_deref() == Some(want))
        });
        let index = by_label.or_else(|| indices.first())?;
        return chip
            .read_i64(&format!("temp{index}_input"))
            .map(millidegrees_to_c);
    }
    None
}

#[allow(clippy::cast_precision_loss)] // sensor values are far below 2^24
pub(crate) fn millidegrees_to_c(v: i64) -> f32 {
    v as f32 / 1000.0
}

/// Keeps the previous `/proc/stat` sample to compute usage deltas.
#[derive(Debug)]
pub(crate) struct CpuReader {
    root: PathBuf,
    model: Option<String>,
    rated_max_mhz: Option<u32>,
    prev: HashMap<Option<u32>, Times>,
}

impl CpuReader {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
            model: read_model(root),
            rated_max_mhz: sysfs::read_u64(
                &root.join("sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq"),
            )
            .and_then(|khz| u32::try_from(khz / 1000).ok()),
            prev: HashMap::new(),
        }
    }

    pub fn sample(&mut self, chips: &[Hwmon]) -> CpuStats {
        let now = sysfs::read_trimmed(&self.root.join("proc/stat"))
            .map(|t| parse_proc_stat(&t))
            .unwrap_or_default();
        let usage =
            |id: Option<u32>| -> Option<f32> { now.get(&id)?.usage_since(*self.prev.get(&id)?) };

        let mut ids: Vec<u32> = now.keys().flatten().copied().collect();
        ids.sort_unstable();
        let cores = ids
            .into_iter()
            .map(|id| CoreStats {
                id,
                usage_percent: usage(Some(id)),
                freq_mhz: sysfs::read_u64(&self.root.join(format!(
                    "sys/devices/system/cpu/cpu{id}/cpufreq/scaling_cur_freq"
                )))
                .and_then(|khz| u32::try_from(khz / 1000).ok()),
            })
            .collect();

        let stats = CpuStats {
            model: self.model.clone(),
            rated_max_mhz: self.rated_max_mhz,
            usage_percent: usage(None),
            temp_c: read_temp(chips),
            cores,
        };
        self.prev = now;
        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::Fixture;

    const STAT_1: &str = "cpu  100 0 100 800 0 0 0 0 0 0\n\
                          cpu0 50 0 50 400 0 0 0 0 0 0\n\
                          cpu1 50 0 50 400 0 0 0 0 0 0\n\
                          intr 12345\n";
    // cpu0 fully busy for 100 jiffies, cpu1 idle for 100 jiffies.
    const STAT_2: &str = "cpu  200 0 100 900 0 0 0 0 0 0\n\
                          cpu0 150 0 50 400 0 0 0 0 0 0\n\
                          cpu1 50 0 50 500 0 0 0 0 0 0\n";

    fn fixture() -> Fixture {
        let fx = Fixture::new();
        fx.write(
            "proc/cpuinfo",
            "processor\t: 0\nmodel name\t: AMD Ryzen 7 4800H with Radeon Graphics\n",
        );
        fx.write("proc/stat", STAT_1);
        fx.write(
            "sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq",
            "2969022\n",
        );
        fx.write(
            "sys/devices/system/cpu/cpu1/cpufreq/scaling_cur_freq",
            "1400000\n",
        );
        fx.write(
            "sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq",
            "4300000\n",
        );
        fx.write("sys/class/hwmon/hwmon5/name", "k10temp\n");
        fx.write("sys/class/hwmon/hwmon5/temp1_input", "92000\n");
        fx.write("sys/class/hwmon/hwmon5/temp1_label", "Tctl\n");
        fx
    }

    #[test]
    fn first_sample_has_no_usage() {
        let fx = fixture();
        let mut reader = CpuReader::new(fx.root());
        let s = reader.sample(&sysfs::hwmons(fx.root()));
        assert_eq!(
            s.model.as_deref(),
            Some("AMD Ryzen 7 4800H with Radeon Graphics")
        );
        assert_eq!(s.usage_percent, None);
        assert_eq!(s.cores.len(), 2);
        assert_eq!(s.cores[0].freq_mhz, Some(2969));
        assert_eq!(s.rated_max_mhz, Some(4300));
        assert_eq!(s.temp_c, Some(92.0));
    }

    #[test]
    fn usage_from_deltas() {
        let fx = fixture();
        let mut reader = CpuReader::new(fx.root());
        let chips = sysfs::hwmons(fx.root());
        reader.sample(&chips);
        fx.write("proc/stat", STAT_2);
        let s = reader.sample(&chips);
        assert_eq!(s.usage_percent, Some(50.0));
        assert_eq!(s.cores[0].usage_percent, Some(100.0));
        assert_eq!(s.cores[1].usage_percent, Some(0.0));
        assert_eq!(s.avg_freq_mhz(), Some(2184));
        assert_eq!(s.max_freq_mhz(), Some(2969));
    }

    #[test]
    fn counter_reset_yields_none() {
        let a = Times {
            busy: 10,
            total: 100,
        };
        let b = Times { busy: 5, total: 50 };
        assert_eq!(b.usage_since(a), None);
    }

    #[test]
    fn prefers_tdie_over_tctl() {
        let fx = fixture();
        fx.write("sys/class/hwmon/hwmon5/temp2_input", "82000\n");
        fx.write("sys/class/hwmon/hwmon5/temp2_label", "Tdie\n");
        assert_eq!(read_temp(&sysfs::hwmons(fx.root())), Some(82.0));
    }

    #[test]
    fn intel_coretemp_package() {
        let fx = Fixture::new();
        fx.write("sys/class/hwmon/hwmon2/name", "coretemp\n");
        fx.write("sys/class/hwmon/hwmon2/temp2_input", "51000\n");
        fx.write("sys/class/hwmon/hwmon2/temp2_label", "Core 0\n");
        fx.write("sys/class/hwmon/hwmon2/temp1_input", "55000\n");
        fx.write("sys/class/hwmon/hwmon2/temp1_label", "Package id 0\n");
        assert_eq!(read_temp(&sysfs::hwmons(fx.root())), Some(55.0));
    }
}
