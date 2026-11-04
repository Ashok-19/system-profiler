//! Optional integration with LACT's daemon (`lactd`,
//! <https://github.com/ilya-zlobintsev/LACT>) for tuning the integrated
//! Radeon GPU: performance level and power profile.
//!
//! lactd speaks newline-delimited JSON on a Unix socket. Every change must
//! be confirmed within a few seconds or lactd reverts it, so each setter
//! sends `confirm_pending_config` right after. Only the iGPU is ever
//! queried; asking about the NVIDIA GPU would go through NVML.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use serde_json::{Value, json};

const SOCKET: &str = "/run/lactd.sock";
/// Overrides the socket path, for tests against a stand-in daemon.
const SOCKET_ENV: &str = "SYSTEM_PROFILER_LACTD_SOCKET";
const TIMEOUT: Duration = Duration::from_secs(3);

/// Performance levels offered in the UI, in display order.
pub const LEVELS: [&str; 3] = [
    "auto", "low", "high",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Igpu {
    /// lactd's device id, e.g. `1002:1636-1043:16A1-0000:06:00.0`.
    pub id: String,
    /// Index into [`LEVELS`]; `None` for levels the UI does not offer
    /// (e.g. `manual`).
    pub level: Option<usize>,
    /// Power profiles as (index, display name), in lactd's order.
    pub modes: Vec<(u16, String)>,
    /// lactd index of the active profile.
    pub active_mode: Option<u16>,
}

fn socket_path() -> String {
    std::env::var(SOCKET_ENV).unwrap_or_else(|_| SOCKET.to_owned())
}

/// Send one request; returns `data` of an `ok` response.
fn request(body: &Value) -> Result<Value, String> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path).map_err(|e| format!("{path}: {e}"))?;
    stream
        .set_read_timeout(Some(TIMEOUT))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .map_err(|e| e.to_string())?;
    let mut line = body.to_string();
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut reply = String::new();
    BufReader::new(stream)
        .read_line(&mut reply)
        .map_err(|e| e.to_string())?;
    let reply: Value = serde_json::from_str(&reply).map_err(|e| e.to_string())?;
    match reply.get("status").and_then(Value::as_str) {
        Some("ok") => Ok(reply.get("data").cloned().unwrap_or(Value::Null)),
        _ => Err(reply
            .get("data")
            .map_or_else(|| "lactd returned an error".to_owned(), Value::to_string)),
    }
}

fn command(name: &str, args: Option<Value>) -> Value {
    let mut request = serde_json::Map::new();
    request.insert("command".to_owned(), Value::from(name));
    if let Some(args) = args {
        request.insert("args".to_owned(), args);
    }
    Value::Object(request)
}

/// Whether lactd is reachable.
pub fn is_running() -> bool {
    request(&command("ping", None)).is_ok()
}

/// lactd's id for the GPU at `pci_address`, from a `list_devices` reply.
fn device_id(devices: &Value, pci_address: &str) -> Option<String> {
    let wanted = pci_address.to_lowercase();
    devices
        .as_array()?
        .iter()
        .filter_map(|d| d.get("id")?.as_str())
        .find(|id| id.to_lowercase().ends_with(&wanted))
        .map(str::to_owned)
}

/// `3D_FULL_SCREEN` -> "3D Full Screen"; short words such as `VR` stay.
fn mode_name(raw: &str) -> String {
    raw.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            if w.len() <= 2 || w.starts_with(|c: char| c.is_ascii_digit()) {
                w.to_owned()
            } else {
                let mut chars = w.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_string() + &chars.as_str().to_lowercase()
                })
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Profiles from a `device_power_profile_modes` reply, sorted by index.
fn parse_modes(table: &Value) -> (Vec<(u16, String)>, Option<u16>) {
    let mut modes: Vec<(u16, String)> = table
        .get("modes")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(index, mode)| {
                    let index = index.parse().ok()?;
                    let name = mode.get("name")?.as_str()?;
                    Some((index, mode_name(name)))
                })
                .collect()
        })
        .unwrap_or_default();
    modes.sort_by_key(|&(i, _)| i);
    let active = table
        .get("active")
        .and_then(Value::as_u64)
        .and_then(|a| u16::try_from(a).ok());
    (modes, active)
}

fn parse_level(stats: &Value) -> Option<usize> {
    let level = stats.get("performance_level")?.as_str()?;
    LEVELS.iter().position(|&l| l == level)
}

/// Current iGPU settings, or why they are unavailable.
pub fn igpu(pci_address: &str) -> Result<Igpu, String> {
    let devices = request(&command("list_devices", None))?;
    let id = device_id(&devices, pci_address)
        .ok_or_else(|| format!("lactd does not list {pci_address}"))?;
    let stats = request(&command("device_stats", Some(json!({ "id": id }))))?;
    let (modes, active_mode) = request(&command(
        "device_power_profile_modes",
        Some(json!({ "id": id })),
    ))
    .map(|t| parse_modes(&t))
    .unwrap_or_default();
    Ok(Igpu {
        level: parse_level(&stats),
        modes,
        active_mode,
        id,
    })
}

fn confirm() -> Result<(), String> {
    request(&command(
        "confirm_pending_config",
        Some(json!({ "command": "confirm" })),
    ))
    .map(|_| ())
}

pub fn set_level(id: &str, level: usize) -> Result<(), String> {
    let level = LEVELS.get(level).ok_or("unknown performance level")?;
    request(&command(
        "set_performance_level",
        Some(json!({ "id": id, "performance_level": level })),
    ))?;
    confirm()
}

pub fn set_mode(id: &str, index: u16) -> Result<(), String> {
    request(&command(
        "set_power_profile_mode",
        Some(json!({ "id": id, "index": index, "custom_heuristics": [] })),
    ))?;
    confirm()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_match_lacts_format() {
        assert_eq!(
            command("list_devices", None).to_string(),
            r#"{"command":"list_devices"}"#
        );
        assert_eq!(
            command(
                "confirm_pending_config",
                Some(json!({ "command": "confirm" }))
            )
            .to_string(),
            r#"{"args":{"command":"confirm"},"command":"confirm_pending_config"}"#
        );
    }

    #[test]
    fn finds_the_igpu_by_pci_address() {
        let devices = json!([
            { "id": "10DE:25A2-1043:16A1-0000:01:00.0", "name": "GA107M" },
            { "id": "1002:1636-1043:16A1-0000:06:00.0", "name": "Renoir" },
        ]);
        assert_eq!(
            device_id(&devices, "0000:06:00.0").as_deref(),
            Some("1002:1636-1043:16A1-0000:06:00.0")
        );
        assert_eq!(device_id(&devices, "0000:07:00.0"), None);
    }

    #[test]
    fn parses_modes_and_level() {
        // Shapes from LACT's own test snapshots.
        let table = json!({
            "active": 3,
            "modes": {
                "6": { "name": "CUSTOM", "components": [] },
                "1": { "name": "3D_FULL_SCREEN", "components": [] },
                "3": { "name": "VIDEO", "components": [] },
                "4": { "name": "VR", "components": [] },
            }
        });
        let (modes, active) = parse_modes(&table);
        let names: Vec<&str> = modes.iter().map(|(_, n)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "3D Full Screen", "Video", "VR", "Custom"
            ]
        );
        assert_eq!(active, Some(3));
        assert_eq!(
            parse_level(&json!({ "performance_level": "high" })),
            Some(2)
        );
        assert_eq!(parse_level(&json!({ "performance_level": "manual" })), None);
        assert_eq!(mode_name("POWER_SAVING"), "Power Saving");
    }
}
