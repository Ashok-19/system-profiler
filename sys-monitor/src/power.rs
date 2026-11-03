//! Battery and AC adapter state from `/sys/class/power_supply`.
//!
//! Batteries report either energy (`energy_*`, µWh, `power_now`, µW) or
//! charge (`charge_*`, µAh, `current_now`, µA). Both are handled; watts and
//! watt-hours are derived from voltage when only charge is available.

use std::path::{Path, PathBuf};

use crate::sysfs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChargeStatus {
    Charging,
    Discharging,
    Full,
    /// Plugged in but held, e.g. by a charge limit.
    NotCharging,
    Unknown,
}

impl ChargeStatus {
    fn parse(s: &str) -> Self {
        match s {
            "Charging" => Self::Charging,
            "Discharging" => Self::Discharging,
            "Full" => Self::Full,
            "Not charging" => Self::NotCharging,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BatteryStats {
    /// `power_supply` name, e.g. `BAT1`.
    pub name: String,
    pub status: ChargeStatus,
    pub capacity_percent: Option<u8>,
    /// Charge/discharge rate in watts (always positive).
    pub power_w: Option<f32>,
    pub energy_now_wh: Option<f32>,
    pub energy_full_wh: Option<f32>,
    pub energy_full_design_wh: Option<f32>,
    /// Some firmwares always report 0.
    pub cycle_count: Option<u32>,
    /// `charge_control_end_threshold`, the charge limit.
    pub charge_limit_percent: Option<u8>,
    pub voltage_v: Option<f32>,
}

impl BatteryStats {
    /// Full capacity relative to design capacity, 0-100+.
    #[must_use]
    pub fn health_percent(&self) -> Option<f32> {
        let design = self.energy_full_design_wh.filter(|&d| d > 0.0)?;
        Some(self.energy_full_wh? / design * 100.0)
    }

    /// Estimated hours until empty (discharging) or full (charging).
    #[must_use]
    pub fn hours_remaining(&self) -> Option<f32> {
        let rate = self.power_w.filter(|&p| p > 0.1)?;
        let now = self.energy_now_wh?;
        match self.status {
            ChargeStatus::Discharging => Some(now / rate),
            ChargeStatus::Charging => Some((self.energy_full_wh? - now).max(0.0) / rate),
            ChargeStatus::Full | ChargeStatus::NotCharging | ChargeStatus::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PowerStats {
    /// `None` if the machine has no AC adapter entry.
    pub ac_online: Option<bool>,
    pub battery: Option<BatteryStats>,
}

#[allow(clippy::cast_precision_loss)] // sensor values are far below 2^24
fn micro(v: u64) -> f32 {
    v as f32 / 1_000_000.0
}

fn read_battery(dir: &Path) -> Option<BatteryStats> {
    let read = |f: &str| sysfs::read_u64(&dir.join(f));
    let voltage_v = read("voltage_now").map(micro);
    // Convert charge (µAh) to energy using the design voltage (stable) or the
    // present voltage.
    let charge_volts = read("voltage_min_design").map(micro).or(voltage_v);
    let energy = |name: &str| -> Option<f32> {
        read(&format!("energy_{name}"))
            .map(micro)
            .or_else(|| Some(micro(read(&format!("charge_{name}"))?) * charge_volts?))
    };
    let power_w = read("power_now").map(micro).or_else(|| {
        // Some drivers report current_now as negative while discharging.
        let amps = sysfs::read_i64(&dir.join("current_now"))?.unsigned_abs();
        Some(micro(amps) * voltage_v?)
    });

    Some(BatteryStats {
        name: dir.file_name()?.to_str()?.to_owned(),
        status: sysfs::read_trimmed(&dir.join("status"))
            .map_or(ChargeStatus::Unknown, |s| ChargeStatus::parse(&s)),
        capacity_percent: read("capacity").and_then(|v| u8::try_from(v).ok()),
        power_w,
        energy_now_wh: energy("now"),
        energy_full_wh: energy("full"),
        energy_full_design_wh: energy("full_design"),
        cycle_count: read("cycle_count").and_then(|v| u32::try_from(v).ok()),
        charge_limit_percent: read("charge_control_end_threshold")
            .and_then(|v| u8::try_from(v).ok()),
        voltage_v,
    })
}

pub(crate) fn sample(root: &Path) -> PowerStats {
    let supplies: Vec<PathBuf> = sysfs::dir_entries(&root.join("sys/class/power_supply"), |_| true);
    let kind = |p: &Path| sysfs::read_trimmed(&p.join("type"));

    let ac_online = supplies
        .iter()
        .find(|p| kind(p).as_deref() == Some("Mains"))
        .and_then(|p| sysfs::read_u64(&p.join("online")))
        .map(|v| v == 1);
    // Skip peripheral batteries (mice, headsets), which have scope=Device.
    let battery = supplies
        .iter()
        .filter(|p| kind(p).as_deref() == Some("Battery"))
        .find(|p| sysfs::read_trimmed(&p.join("scope")).as_deref() != Some("Device"))
        .and_then(|p| read_battery(p));

    PowerStats { ac_online, battery }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::Fixture;

    fn close(a: Option<f32>, b: f32) -> bool {
        a.is_some_and(|a| (a - b).abs() < 0.01)
    }

    #[test]
    fn charge_based_battery() {
        // Values from the FA506IC (A32-K55 pack).
        let fx = Fixture::new();
        let b = "sys/class/power_supply/BAT1";
        fx.write(&format!("{b}/type"), "Battery\n");
        fx.write(&format!("{b}/status"), "Discharging\n");
        fx.write(&format!("{b}/capacity"), "80\n");
        fx.write(&format!("{b}/voltage_now"), "15000000\n");
        fx.write(&format!("{b}/voltage_min_design"), "15860000\n");
        fx.write(&format!("{b}/current_now"), "1000000\n");
        fx.write(&format!("{b}/charge_now"), "3000000\n");
        fx.write(&format!("{b}/charge_full"), "4066000\n");
        fx.write(&format!("{b}/charge_full_design"), "5675000\n");
        fx.write(&format!("{b}/cycle_count"), "0\n");
        fx.write(&format!("{b}/charge_control_end_threshold"), "80\n");
        fx.write("sys/class/power_supply/ACAD/type", "Mains\n");
        fx.write("sys/class/power_supply/ACAD/online", "0\n");

        let p = sample(fx.root());
        assert_eq!(p.ac_online, Some(false));
        let bat = p.battery.unwrap();
        assert_eq!(bat.name, "BAT1");
        assert_eq!(bat.status, ChargeStatus::Discharging);
        assert_eq!(bat.capacity_percent, Some(80));
        assert_eq!(bat.charge_limit_percent, Some(80));
        assert!(close(bat.power_w, 15.0));
        assert!(close(bat.energy_now_wh, 3.0 * 15.86));
        assert!(close(bat.health_percent(), 4066.0 / 5675.0 * 100.0));
        assert!(close(bat.hours_remaining(), 3.0 * 15.86 / 15.0));
    }

    #[test]
    fn energy_based_battery_with_peripheral() {
        let fx = Fixture::new();
        fx.write("sys/class/power_supply/hid-mouse/type", "Battery\n");
        fx.write("sys/class/power_supply/hid-mouse/scope", "Device\n");
        let b = "sys/class/power_supply/BAT0";
        fx.write(&format!("{b}/type"), "Battery\n");
        fx.write(&format!("{b}/status"), "Charging\n");
        fx.write(&format!("{b}/power_now"), "20000000\n");
        fx.write(&format!("{b}/energy_now"), "40000000\n");
        fx.write(&format!("{b}/energy_full"), "60000000\n");

        let bat = sample(fx.root()).battery.unwrap();
        assert_eq!(bat.name, "BAT0");
        assert!(close(bat.power_w, 20.0));
        assert!(close(bat.hours_remaining(), 1.0));
        assert_eq!(bat.health_percent(), None);
    }

    #[test]
    fn negative_current() {
        let fx = Fixture::new();
        let b = "sys/class/power_supply/BAT0";
        fx.write(&format!("{b}/type"), "Battery\n");
        fx.write(&format!("{b}/voltage_now"), "10000000\n");
        fx.write(&format!("{b}/current_now"), "-2000000\n");
        let bat = sample(fx.root()).battery.unwrap();
        assert!(close(bat.power_w, 20.0));
    }

    #[test]
    fn desktop_without_battery() {
        let fx = Fixture::new();
        assert_eq!(sample(fx.root()), PowerStats::default());
    }
}
