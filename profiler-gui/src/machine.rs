//! Static facts about the machine, gathered once at startup for the Home
//! and System Information pages.

use std::fs;
use std::path::Path;

use sys_monitor::{GpuStats, Snapshot};

const PCI_IDS: &[&str] = &[
    "/usr/share/misc/pci.ids",
    "/usr/share/hwdata/pci.ids",
];

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MachineInfo {
    /// Display name, e.g. "TUF GAMING A15".
    pub name: String,
    /// Board/model code, e.g. "FA506ICB".
    pub board: String,
    pub cpu: String,
    /// Marketing name of the discrete GPU, if any.
    pub dgpu: String,
    /// Logical CPUs (hardware threads).
    pub cpu_threads: usize,
    /// cpufreq scaling driver, e.g. "amd-pstate-epp".
    pub cpufreq_driver: String,
    /// System Information rows, (label, value).
    pub details: Vec<(String, String)>,
}

fn read_dmi(root: &Path, field: &str) -> Option<String> {
    let value = fs::read_to_string(root.join("sys/class/dmi/id").join(field)).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// "ASUS TUF Gaming A15" -> "TUF GAMING A15".
fn display_name(family: &str) -> String {
    family
        .trim()
        .strip_prefix("ASUS ")
        .unwrap_or(family.trim())
        .to_uppercase()
}

/// Look up a PCI device in `pci.ids` text and return a short marketing name
/// such as `NVIDIA GeForce RTX 3050 Mobile`.
fn pci_name(ids: &str, vendor: u16, device: u16) -> Option<String> {
    let vendor_key = format!("{vendor:04x}");
    let device_key = format!("\t{device:04x}");
    let mut lines = ids.lines();
    let vendor_name = lines.find_map(|l| {
        l.strip_prefix(&vendor_key)
            .filter(|rest| rest.starts_with("  "))
            .map(str::trim)
    })?;
    // Device lines follow the vendor line until the next unindented line.
    let device_name = lines
        .take_while(|l| l.starts_with('\t') || l.starts_with('#') || l.is_empty())
        .filter(|l| !l.starts_with("\t\t"))
        .find_map(|l| {
            l.strip_prefix(&device_key)
                .filter(|rest| rest.starts_with("  "))
                .map(str::trim)
        })?;

    // "GA107M [GeForce RTX 3050 Mobile]" -> "GeForce RTX 3050 Mobile"
    let model = device_name
        .split_once('[')
        .and_then(|(_, rest)| rest.rsplit_once(']'))
        .map_or(device_name, |(inner, _)| inner);
    let short_vendor = if vendor_name.contains("[AMD") {
        "AMD"
    } else {
        vendor_name.split_whitespace().next().unwrap_or(vendor_name)
    };
    Some(format!("{short_vendor} {model}"))
}

fn parse_hex_id(id: &str) -> Option<u16> {
    u16::from_str_radix(id.trim().trim_start_matches("0x"), 16).ok()
}

fn gpu_name(gpu: &GpuStats, ids: &str) -> Option<String> {
    let vendor = parse_hex_id(gpu.vendor_id.as_deref()?)?;
    let device = parse_hex_id(gpu.device_id.as_deref()?)?;
    pci_name(ids, vendor, device)
}

fn read_text(root: &Path, path: &str) -> Option<String> {
    let text = fs::read_to_string(root.join(path)).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// DMI dates are `MM/DD/YYYY`; show them as `YYYY-MM-DD`.
fn iso_date(dmi: &str) -> Option<String> {
    let mut parts = dmi.split('/');
    let (m, d, y) = (parts.next()?, parts.next()?, parts.next()?);
    (m.len() == 2 && d.len() == 2 && y.len() == 4).then(|| format!("{y}-{m}-{d}"))
}

/// `PRETTY_NAME` from os-release text.
fn pretty_os_name(os_release: &str) -> Option<String> {
    os_release.lines().find_map(|l| {
        l.strip_prefix("PRETTY_NAME=")
            .map(|v| v.trim_matches('"').to_owned())
    })
}

/// Physical cores from /proc/cpuinfo text.
fn physical_cores(cpuinfo: &str) -> Option<u32> {
    cpuinfo
        .lines()
        .find(|l| l.starts_with("cpu cores"))
        .and_then(|l| l.split_once(':'))
        .and_then(|(_, v)| v.trim().parse().ok())
}

/// Native resolution of the built-in panel (first mode of an eDP/LVDS
/// connector), e.g. "1920 × 1080".
fn panel_resolution(root: &Path) -> Option<String> {
    let drm = fs::read_dir(root.join("sys/class/drm")).ok()?;
    drm.flatten()
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.contains("-eDP-") || name.contains("-LVDS-")
        })
        .find_map(|e| {
            let modes = fs::read_to_string(e.path().join("modes")).ok()?;
            let (w, h) = modes.lines().next()?.split_once('x')?;
            Some(format!("{w} × {h}"))
        })
}

#[allow(clippy::cast_precision_loss)] // display only
fn gib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}

fn details(root: &Path, snapshot: &Snapshot, ids: Option<&str>) -> Vec<(String, String)> {
    let dmi = |f: &str| read_dmi(root, f);
    let mut rows: Vec<(&str, Option<String>)> = Vec::new();

    let model = dmi("product_family").or_else(|| dmi("product_name"));
    rows.push((
        "Model",
        model.map(|m| match dmi("board_name") {
            Some(board) if !m.contains(&board) => format!("{m} ({board})"),
            _ => m,
        }),
    ));
    rows.push((
        "BIOS",
        dmi("bios_version").map(|v| match dmi("bios_date").as_deref().and_then(iso_date) {
            Some(date) => format!("{v} ({date})"),
            None => v,
        }),
    ));

    let cpu = &snapshot.cpu;
    let mut processor: Vec<String> = cpu.model.iter().cloned().collect();
    let cores = read_text(root, "proc/cpuinfo")
        .as_deref()
        .and_then(physical_cores);
    match (cores, cpu.cores.len()) {
        (Some(c), t) if t > 0 => processor.push(format!("{c} cores / {t} threads")),
        (None, t) if t > 0 => processor.push(format!("{t} threads")),
        _ => {}
    }
    if let Some(mhz) = cpu.rated_max_mhz {
        processor.push(format!("up to {:.1} GHz", f64::from(mhz) / 1000.0));
    }
    rows.push((
        "Processor",
        (!processor.is_empty()).then(|| processor.join(" · ")),
    ));

    let name = |g: &GpuStats| ids.and_then(|ids| gpu_name(g, ids));
    rows.push((
        "Integrated Graphics",
        snapshot.gpus.iter().find(|g| g.boot_vga).and_then(name),
    ));
    if let Some(dgpu) = snapshot.gpus.iter().find(|g| !g.boot_vga) {
        let driver = match dgpu.driver.as_deref() {
            Some("nvidia") => {
                read_text(root, "sys/module/nvidia/version").map(|v| format!("nvidia {v}"))
            }
            other => other.map(str::to_owned),
        };
        rows.push((
            "Discrete Graphics",
            name(dgpu).map(|n| match driver {
                Some(d) => format!("{n} · {d}"),
                None => n,
            }),
        ));
    }
    rows.push(("Display", panel_resolution(root)));
    rows.push((
        "Memory",
        snapshot.memory.as_ref().map(|m| {
            if m.swap_total_bytes > 0 {
                format!(
                    "{:.1} GiB · {:.1} GiB swap",
                    gib(m.total_bytes),
                    gib(m.swap_total_bytes)
                )
            } else {
                format!("{:.1} GiB", gib(m.total_bytes))
            }
        }),
    ));
    rows.push((
        "Battery",
        snapshot.power.battery.as_ref().map(|b| {
            let mut parts: Vec<String> = b.model.iter().cloned().collect();
            if let Some(wh) = b.energy_full_design_wh {
                parts.push(format!("{wh:.0} Wh design"));
            }
            parts.join(" · ")
        }),
    ));
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    let session = std::env::var("XDG_SESSION_TYPE").ok();
    rows.push((
        "Operating System",
        read_text(root, "etc/os-release")
            .as_deref()
            .and_then(pretty_os_name)
            .map(|os| match (desktop, session) {
                (Some(d), Some(s)) => format!("{os} · {d} ({})", s.to_uppercase()),
                (Some(d), None) => format!("{os} · {d}"),
                _ => os,
            }),
    ));
    rows.push((
        "Kernel",
        read_text(root, "proc/sys/kernel/osrelease").map(|r| format!("Linux {r}")),
    ));

    rows.into_iter()
        .filter_map(|(k, v)| Some((k.to_owned(), v.filter(|v| !v.is_empty())?)))
        .collect()
}

impl MachineInfo {
    /// Gather facts once at startup. `snapshot` supplies CPU/GPU identity.
    pub fn detect(snapshot: &Snapshot) -> Self {
        let root = Path::new("/");
        let family = read_dmi(root, "product_family").or_else(|| read_dmi(root, "product_name"));
        let ids = PCI_IDS.iter().find_map(|p| fs::read_to_string(p).ok());
        Self {
            name: family
                .as_deref()
                .map_or_else(|| "LAPTOP".to_owned(), display_name),
            board: read_dmi(root, "board_name").unwrap_or_default(),
            cpu: snapshot.cpu.model.clone().unwrap_or_default(),
            dgpu: snapshot
                .gpus
                .iter()
                .find(|g| !g.boot_vga)
                .zip(ids.as_deref())
                .and_then(|(g, ids)| gpu_name(g, ids))
                .unwrap_or_default(),
            cpu_threads: snapshot.cpu.cores.len(),
            cpufreq_driver: fs::read_to_string(
                root.join("sys/devices/system/cpu/cpu0/cpufreq/scaling_driver"),
            )
            .map(|d| d.trim().to_owned())
            .unwrap_or_default(),
            details: details(root, snapshot, ids.as_deref()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDS: &str = "\
# comment
1002  Advanced Micro Devices, Inc. [AMD/ATI]
\t1636  Renoir [Radeon RX Vega 6 (Ryzen 4000/5000 Mobile Series)]
\t\t1043 16a1  Some subsystem
8086  Intel Corporation
\t25a2  6300ESB PATA Storage Controller
10de  NVIDIA Corporation
\t0008  NV1 [STG2000X-B Series]
\t25a2  GA107M [GeForce RTX 3050 Mobile]
\t25a3  NoBrackets Device
10df  Emulex Corporation
";

    #[test]
    fn pci_lookup_is_scoped_to_vendor() {
        assert_eq!(
            pci_name(IDS, 0x10de, 0x25a2).as_deref(),
            Some("NVIDIA GeForce RTX 3050 Mobile")
        );
        assert_eq!(
            pci_name(IDS, 0x1002, 0x1636).as_deref(),
            Some("AMD Radeon RX Vega 6 (Ryzen 4000/5000 Mobile Series)")
        );
        assert_eq!(
            pci_name(IDS, 0x10de, 0x25a3).as_deref(),
            Some("NVIDIA NoBrackets Device")
        );
        assert_eq!(pci_name(IDS, 0x10de, 0xffff), None);
        assert_eq!(pci_name(IDS, 0x1234, 0x25a2), None);
    }

    #[test]
    fn parsers() {
        assert_eq!(iso_date("12/28/2022").as_deref(), Some("2022-12-28"));
        assert_eq!(iso_date("2022-12-28"), None);
        assert_eq!(
            pretty_os_name("NAME=\"Ubuntu\"\nPRETTY_NAME=\"Ubuntu 24.04.5 LTS\"\n").as_deref(),
            Some("Ubuntu 24.04.5 LTS")
        );
        assert_eq!(physical_cores("processor\t: 0\ncpu cores\t: 8\n"), Some(8));
    }

    #[test]
    fn names() {
        assert_eq!(display_name("ASUS TUF Gaming A15"), "TUF GAMING A15");
        assert_eq!(display_name("ROG Zephyrus G14"), "ROG ZEPHYRUS G14");
        assert_eq!(parse_hex_id("0x10de\n"), Some(0x10de));
    }
}
