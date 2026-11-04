//! Decides when sensor changes deserve a desktop notification.

/// Degrees below the warning the CPU must cool before warning again.
const REARM_BELOW_C: f32 = 5.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    /// Kind of alert; a newer one of the same kind replaces the older.
    pub tag: &'static str,
    pub summary: String,
    pub body: String,
}

/// What the user asked to be told about.
#[derive(Debug, Clone, Copy, Default)]
pub struct Wanted {
    pub power: bool,
    /// Warn above this CPU temperature (°C); 0 = never.
    pub hot_cpu_c: u32,
    pub fahrenheit: bool,
}

/// One sample's worth of the readings alerts depend on.
#[derive(Debug, Clone, Copy, Default)]
pub struct Reading {
    pub on_ac: Option<bool>,
    pub battery_percent: Option<u8>,
    pub cpu_c: Option<f32>,
}

#[derive(Debug, Default)]
pub struct Alerts {
    on_ac: Option<bool>,
    /// Already warned; waiting for the CPU to cool down.
    hot: bool,
}

fn degrees(c: f32, fahrenheit: bool) -> String {
    if fahrenheit {
        format!("{:.0} °F", c * 9.0 / 5.0 + 32.0)
    } else {
        format!("{c:.0} °C")
    }
}

impl Alerts {
    pub fn check(&mut self, now: Reading, wanted: Wanted) -> Vec<Alert> {
        let mut alerts = Vec::new();

        if let (Some(before), Some(after)) = (self.on_ac, now.on_ac)
            && before != after
            && wanted.power
        {
            let battery = now
                .battery_percent
                .map(|p| format!("Battery at {p}%"))
                .unwrap_or_default();
            alerts.push(Alert {
                tag: "power",
                summary: if after { "Plugged in" } else { "On battery" }.to_owned(),
                body: battery,
            });
        }
        if now.on_ac.is_some() {
            self.on_ac = now.on_ac;
        }

        #[allow(clippy::cast_precision_loss)] // small integer limit
        let limit = wanted.hot_cpu_c as f32;
        match now.cpu_c {
            Some(c) if wanted.hot_cpu_c > 0 && c > limit && !self.hot => {
                self.hot = true;
                alerts.push(Alert {
                    tag: "cpu-hot",
                    summary: "CPU is running hot".to_owned(),
                    body: format!(
                        "{}, above your {} warning",
                        degrees(c, wanted.fahrenheit),
                        degrees(limit, wanted.fahrenheit)
                    ),
                });
            }
            Some(c) if c < limit - REARM_BELOW_C => self.hot = false,
            _ => {}
        }
        if wanted.hot_cpu_c == 0 {
            self.hot = false;
        }
        alerts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: Wanted = Wanted {
        power: true,
        hot_cpu_c: 95,
        fahrenheit: false,
    };

    fn reading(on_ac: bool, cpu_c: f32) -> Reading {
        Reading {
            on_ac: Some(on_ac),
            battery_percent: Some(78),
            cpu_c: Some(cpu_c),
        }
    }

    #[test]
    fn power_changes_only() {
        let mut a = Alerts::default();
        assert!(a.check(reading(true, 50.0), ALL).is_empty(), "first sample");
        assert!(a.check(reading(true, 50.0), ALL).is_empty());
        let alerts = a.check(reading(false, 50.0), ALL);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].summary, "On battery");
        assert_eq!(alerts[0].body, "Battery at 78%");
        let off = Wanted {
            power: false,
            ..ALL
        };
        assert!(a.check(reading(true, 50.0), off).is_empty());
    }

    #[test]
    fn hot_warning_once_until_cooled() {
        let mut a = Alerts::default();
        a.check(reading(true, 50.0), ALL);
        let alerts = a.check(reading(true, 96.4), ALL);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].body, "96 °C, above your 95 °C warning");
        assert!(a.check(reading(true, 97.0), ALL).is_empty());
        assert!(
            a.check(reading(true, 92.0), ALL).is_empty(),
            "not cool enough"
        );
        assert!(a.check(reading(true, 96.0), ALL).is_empty());
        a.check(reading(true, 85.0), ALL);
        assert_eq!(a.check(reading(true, 96.0), ALL).len(), 1, "re-armed");
    }

    #[test]
    fn hot_warning_disabled_and_fahrenheit() {
        let mut a = Alerts::default();
        let never = Wanted {
            hot_cpu_c: 0,
            ..ALL
        };
        assert!(a.check(reading(true, 120.0), never).is_empty());
        let f = Wanted {
            fahrenheit: true,
            ..ALL
        };
        assert_eq!(
            a.check(reading(true, 100.0), f)[0].body,
            "212 °F, above your 203 °F warning"
        );
    }
}
