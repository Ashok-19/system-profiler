//! Built-in panel refresh rate (via `xrandr`) and touchpad on/off (via
//! `xinput`). X11 only. Commands run only when the Display page asks or
//! the power source changes, never on a timer.

use std::process::Command;

/// Rates below this are not offered; they only make motion worse.
const MIN_RATE_HZ: f32 = 59.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    /// xrandr output name, e.g. `eDP`.
    pub output: String,
    pub width: u32,
    pub height: u32,
    /// Distinct rates for the current resolution, fastest first.
    pub rates: Vec<f32>,
    /// Index into `rates` of the active rate.
    pub current: Option<usize>,
    /// Diagonal in inches, from the reported physical size.
    pub inches: Option<f32>,
}

impl Panel {
    pub fn mode(&self) -> String {
        format!("{}x{}", self.width, self.height)
    }
}

fn is_builtin(output: &str) -> bool {
    output.starts_with("eDP") || output.starts_with("LVDS") || output.starts_with("DSI")
}

/// Marketed laptop panel sizes; EDID sizes land a little off them.
const COMMON_INCHES: [f32; 8] = [
    11.6, 13.3, 14.0, 15.6, 16.0, 17.3, 18.0, 13.0,
];

/// "344mm x 193mm" -> diagonal inches, snapped to a marketed size when
/// within 0.2″ (344 x 193 mm is 15.53″, sold as 15.6″).
fn diagonal_inches(line: &str) -> Option<f32> {
    let mut mm = line
        .split_whitespace()
        .filter_map(|w| w.strip_suffix("mm")?.parse::<f32>().ok());
    let (w, h) = (mm.next()?, mm.next()?);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let measured = w.hypot(h) / 25.4;
    Some(
        COMMON_INCHES
            .into_iter()
            .find(|c| (c - measured).abs() <= 0.2)
            .unwrap_or(measured),
    )
}

/// One resolution's line: width, height and its (rate, active) pairs.
type ModeLine = (u32, u32, Vec<(f32, bool)>);

/// Parse `xrandr --query` for the built-in panel (else the first
/// connected output).
pub fn parse_xrandr(text: &str) -> Option<Panel> {
    let lines: Vec<&str> = text.lines().collect();
    let headers: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| !l.starts_with(char::is_whitespace) && l.contains(" connected"))
        .map(|(i, _)| i)
        .collect();
    let header = headers
        .iter()
        .copied()
        .find(|&i| is_builtin(lines[i]))
        .or_else(|| headers.first().copied())?;
    let output = lines[header].split_whitespace().next()?.to_owned();
    let inches = diagonal_inches(lines[header]);

    // Mode lines follow the header; the active one has a '*'.
    let modes = lines[header + 1..]
        .iter()
        .take_while(|l| l.starts_with(char::is_whitespace));
    let mut chosen: Option<ModeLine> = None;
    for line in modes {
        let mut words = line.split_whitespace();
        let Some((w, h)) = words.next().and_then(|m| m.split_once('x')) else {
            continue;
        };
        let (Ok(w), Ok(h)) = (w.parse(), h.trim_end_matches('i').parse()) else {
            continue;
        };
        let rates: Vec<(f32, bool)> = words
            .filter_map(|r| {
                let active = r.contains('*');
                let hz = r.trim_end_matches(['*', '+']).parse().ok()?;
                Some((hz, active))
            })
            .collect();
        let active = rates.iter().any(|&(_, a)| a);
        // The active resolution, else the first (preferred) one.
        if active || chosen.is_none() {
            chosen = Some((w, h, rates));
            if active {
                break;
            }
        }
    }
    let (width, height, mut rates) = chosen?;

    rates.retain(|&(hz, _)| hz >= MIN_RATE_HZ);
    rates.sort_by(|a, b| b.0.total_cmp(&a.0));
    // 60.02 and 60.00 Hz are the same choice for a person; keep one.
    let mut distinct: Vec<(f32, bool)> = Vec::new();
    for (hz, active) in rates {
        match distinct.last_mut() {
            Some(last) if (last.0 - hz).abs() < 1.0 => last.1 |= active,
            _ => distinct.push((hz, active)),
        }
    }
    Some(Panel {
        output,
        width,
        height,
        current: distinct.iter().position(|&(_, a)| a),
        rates: distinct.into_iter().map(|(hz, _)| hz).collect(),
        inches,
    })
}

fn run(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output();
    match out {
        Ok(out) if out.status.success() => Some(String::from_utf8_lossy(&out.stdout).into_owned()),
        Ok(out) => {
            log::warn!(
                "{program} {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
            None
        }
        Err(e) => {
            log::warn!("Cannot run {program}: {e}");
            None
        }
    }
}

pub fn query_panel() -> Option<Panel> {
    parse_xrandr(&run("xrandr", &["--query"])?)
}

pub fn set_rate(panel: &Panel, hz: f32) -> bool {
    let rate = format!("{hz:.2}");
    run(
        "xrandr",
        &[
            "--output",
            &panel.output,
            "--mode",
            &panel.mode(),
            "--rate",
            &rate,
        ],
    )
    .is_some()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Touchpad {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
}

/// Find the touchpad in `xinput list` output: (id, name).
pub fn parse_touchpad(list: &str) -> Option<(u32, String)> {
    list.lines().find_map(|line| {
        let (name, rest) = line.split_once("id=")?;
        let name = name
            .trim_start_matches(|c: char| !c.is_alphanumeric())
            .trim();
        if !name.to_lowercase().contains("touchpad") {
            return None;
        }
        let id = rest.split_whitespace().next()?.parse().ok()?;
        Some((id, name.to_owned()))
    })
}

/// The `Device Enabled (185): 1` line of `xinput list-props` output.
pub fn parse_enabled(props: &str) -> Option<bool> {
    props
        .lines()
        .find(|l| l.trim_start().starts_with("Device Enabled"))
        .and_then(|l| l.rsplit(':').next())
        .map(|v| v.trim() == "1")
}

pub fn query_touchpad() -> Option<Touchpad> {
    let (id, name) = parse_touchpad(&run("xinput", &["list"])?)?;
    let enabled = parse_enabled(&run(
        "xinput",
        &[
            "list-props",
            &id.to_string(),
        ],
    )?)?;
    Some(Touchpad { id, name, enabled })
}

pub fn set_touchpad(id: u32, enabled: bool) -> bool {
    let action = if enabled { "enable" } else { "disable" };
    run(
        "xinput",
        &[
            action,
            &id.to_string(),
        ],
    )
    .is_some()
}

/// Rate to use for a power source when "60 Hz on battery" is on: the
/// slowest offered rate on battery, the fastest on AC.
pub fn rate_for_power(panel: &Panel, on_ac: bool) -> Option<f32> {
    if on_ac {
        panel.rates.first().copied()
    } else {
        panel.rates.last().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FA506: &str = "\
Screen 0: minimum 320 x 200, current 1920 x 1080, maximum 16384 x 16384
eDP connected 1920x1080+0+0 (normal left inverted right x axis y axis) 344mm x 193mm
   1920x1080    144.00*+ 120.04    96.00    72.00    60.02    50.00    48.00    60.00
   1680x1050    144.00
   1280x720     144.00
HDMI-A-0 disconnected (normal left inverted right x axis y axis)
DP-1-0 disconnected (normal left inverted right x axis y axis)
";

    #[test]
    fn fa506_panel() {
        let p = parse_xrandr(FA506).unwrap();
        assert_eq!(p.output, "eDP");
        assert_eq!((p.width, p.height), (1920, 1080));
        assert_eq!(
            p.rates,
            [
                144.0, 120.04, 96.0, 72.0, 60.02
            ]
        );
        assert_eq!(p.current, Some(0));
        assert_eq!(p.inches, Some(15.6));
        assert_eq!(p.mode(), "1920x1080");
        assert_eq!(rate_for_power(&p, false), Some(60.02));
        assert_eq!(rate_for_power(&p, true), Some(144.0));
    }

    #[test]
    fn active_rate_after_duplicate_merge() {
        let text = FA506
            .replace("144.00*+", "144.00 +")
            .replace("    60.00\n", "    60.00*\n");
        let p = parse_xrandr(&text).unwrap();
        assert_eq!(p.current, Some(4), "60.00 is merged into 60.02");
    }

    #[test]
    fn prefers_builtin_over_external() {
        let text = "\
HDMI-1 connected 2560x1440+0+0 (normal) 600mm x 340mm
   2560x1440     59.95*+
LVDS-1 connected primary 1366x768+2560+0 (normal) 344mm x 194mm
   1366x768      60.00*+
";
        let p = parse_xrandr(text).unwrap();
        assert_eq!(p.output, "LVDS-1");
        assert_eq!(p.inches, Some(15.6));
        assert_eq!(p.rates, [60.0]);
    }

    #[test]
    fn nothing_connected() {
        assert_eq!(parse_xrandr("Screen 0: minimum 8 x 8\n"), None);
    }

    #[test]
    fn touchpad_parsing() {
        let list = "\
⎡ Virtual core pointer                    \tid=2\t[master pointer  (3)]
⎜   ↳ ELAN1203:00 04F3:307A Mouse             \tid=17\t[slave  pointer  (2)]
⎜   ↳ ELAN1203:00 04F3:307A Touchpad          \tid=18\t[slave  pointer  (2)]
";
        assert_eq!(
            parse_touchpad(list),
            Some((18, "ELAN1203:00 04F3:307A Touchpad".to_owned()))
        );
        assert_eq!(parse_touchpad("⎜   ↳ Mouse\tid=3\t[slave]"), None);
        assert_eq!(
            parse_enabled("Device 'x':\n\tDevice Enabled (185):\t1\n"),
            Some(true)
        );
        assert_eq!(parse_enabled("\tDevice Enabled (185):\t0\n"), Some(false));
    }
}
