//! Unprivileged live hardware monitoring for Linux laptops.
//!
//! Everything is read from procfs/sysfs, so no root access or daemon is
//! required. A [`Monitor`] keeps the state needed for rate-based values such
//! as CPU usage; call [`Monitor::sample`] periodically (about once a second).
//!
//! GPUs that are runtime-suspended are never touched beyond reading their
//! power state, so monitoring does not wake a sleeping dGPU.
//!
//! ```no_run
//! let mut monitor = sys_monitor::Monitor::new();
//! let snapshot = monitor.sample();
//! println!("CPU {:?} °C", snapshot.cpu.temp_c);
//! ```

mod cpu;
mod gpu;
mod memory;
mod power;
mod sensors;
mod sysfs;
#[cfg(test)]
mod test_util;

use std::path::{Path, PathBuf};

pub use cpu::{CoreStats, CpuStats};
pub use gpu::{GpuStats, PowerState};
pub use memory::MemoryStats;
pub use power::{BatteryStats, ChargeStatus, PowerStats};
pub use sensors::{FanStats, TempSensor};

/// One point-in-time reading of all sensors.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub cpu: CpuStats,
    pub gpus: Vec<GpuStats>,
    pub memory: Option<MemoryStats>,
    pub power: PowerStats,
    pub fans: Vec<FanStats>,
    /// Every hwmon temperature channel (CPU, GPU, `NVMe`, Wi-Fi, ACPI, ...).
    pub temps: Vec<TempSensor>,
}

/// Samples hardware state; keeps history needed for usage percentages.
#[derive(Debug)]
pub struct Monitor {
    root: PathBuf,
    cpu: cpu::CpuReader,
}

impl Default for Monitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Monitor {
    /// Monitor the running system.
    #[must_use]
    pub fn new() -> Self {
        Self::with_root(Path::new("/"))
    }

    /// Monitor a filesystem tree rooted at `root` instead of `/` (for tests
    /// and captured sysfs dumps).
    #[must_use]
    pub fn with_root(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
            cpu: cpu::CpuReader::new(root),
        }
    }

    /// Read every sensor. Usage percentages are relative to the previous
    /// call and are `None` on the first one.
    pub fn sample(&mut self) -> Snapshot {
        let chips = sysfs::hwmons(&self.root);
        Snapshot {
            cpu: self.cpu.sample(&chips),
            gpus: gpu::sample(&self.root),
            memory: memory::sample(&self.root),
            power: power::sample(&self.root),
            fans: sensors::fans(&chips),
            temps: sensors::temps(&chips),
        }
    }
}
