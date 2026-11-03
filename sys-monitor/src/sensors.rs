//! Generic hwmon fans and temperature sensors.

use crate::cpu::millidegrees_to_c;
use crate::sysfs::Hwmon;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanStats {
    /// hwmon chip name, e.g. `asus`.
    pub chip: String,
    /// Channel label, e.g. `cpu_fan`, or `fanN` when the driver has none.
    pub label: String,
    pub rpm: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TempSensor {
    /// hwmon chip name, e.g. `nvme`, `k10temp`, `mt7921_phy0`.
    pub chip: String,
    /// Channel label, e.g. `Composite`, or `tempN` when the driver has none.
    pub label: String,
    pub temp_c: f32,
}

/// `(chip, label, raw value)` for every `<prefix>N_input` channel.
fn channels<'a>(
    chips: &'a [Hwmon],
    prefix: &'a str,
) -> impl Iterator<Item = (&'a Hwmon, String, i64)> + 'a {
    chips.iter().flat_map(move |chip| {
        chip.input_indices(prefix).into_iter().filter_map(move |i| {
            let value = chip.read_i64(&format!("{prefix}{i}_input"))?;
            let label = chip
                .read_label(prefix, &i)
                .unwrap_or_else(|| format!("{prefix}{i}"));
            Some((chip, label, value))
        })
    })
}

/// Every fan tachometer reported by any hwmon chip.
pub(crate) fn fans(chips: &[Hwmon]) -> Vec<FanStats> {
    channels(chips, "fan")
        .filter_map(|(chip, label, value)| {
            Some(FanStats {
                chip: chip.name.clone(),
                label,
                rpm: u32::try_from(value).ok()?,
            })
        })
        .collect()
}

/// Every temperature channel reported by any hwmon chip.
pub(crate) fn temps(chips: &[Hwmon]) -> Vec<TempSensor> {
    channels(chips, "temp")
        .map(|(chip, label, value)| TempSensor {
            chip: chip.name.clone(),
            label,
            temp_c: millidegrees_to_c(value),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysfs::hwmons;
    use crate::test_util::Fixture;

    #[test]
    fn fans_and_temps() {
        let fx = Fixture::new();
        fx.write("sys/class/hwmon/hwmon6/name", "asus\n");
        fx.write("sys/class/hwmon/hwmon6/fan1_input", "2700\n");
        fx.write("sys/class/hwmon/hwmon6/fan1_label", "cpu_fan\n");
        fx.write("sys/class/hwmon/hwmon6/fan2_input", "2800\n");
        fx.write("sys/class/hwmon/hwmon6/fan2_label", "gpu_fan\n");
        fx.write("sys/class/hwmon/hwmon3/name", "nvme\n");
        fx.write("sys/class/hwmon/hwmon3/temp1_input", "46850\n");
        fx.write("sys/class/hwmon/hwmon3/temp1_label", "Composite\n");
        fx.write("sys/class/hwmon/hwmon7/name", "mt7921_phy0\n");
        fx.write("sys/class/hwmon/hwmon7/temp1_input", "48000\n");
        let chips = hwmons(fx.root());

        assert_eq!(
            fans(&chips),
            [
                FanStats {
                    chip: "asus".to_owned(),
                    label: "cpu_fan".to_owned(),
                    rpm: 2700
                },
                FanStats {
                    chip: "asus".to_owned(),
                    label: "gpu_fan".to_owned(),
                    rpm: 2800
                },
            ]
        );
        let t = temps(&chips);
        assert_eq!(t.len(), 2);
        assert_eq!(
            (t[0].chip.as_str(), t[0].label.as_str()),
            ("nvme", "Composite")
        );
        assert!((t[0].temp_c - 46.85).abs() < 1e-3);
        assert_eq!(
            (t[1].chip.as_str(), t[1].label.as_str()),
            ("mt7921_phy0", "temp1")
        );
    }
}
