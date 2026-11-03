//! Static facts about the machine shown on the Home page.

use std::fs;
use std::path::Path;

use sys_monitor::Snapshot;

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

fn dgpu_name(snapshot: &Snapshot) -> Option<String> {
    let gpu = snapshot.gpus.iter().find(|g| !g.boot_vga)?;
    let vendor = parse_hex_id(gpu.vendor_id.as_deref()?)?;
    let device = parse_hex_id(gpu.device_id.as_deref()?)?;
    let ids = PCI_IDS.iter().find_map(|p| fs::read_to_string(p).ok())?;
    pci_name(&ids, vendor, device)
}

impl MachineInfo {
    /// Gather facts once at startup. `snapshot` supplies CPU/GPU identity.
    pub fn detect(snapshot: &Snapshot) -> Self {
        let root = Path::new("/");
        let family = read_dmi(root, "product_family").or_else(|| read_dmi(root, "product_name"));
        Self {
            name: family
                .as_deref()
                .map_or_else(|| "LAPTOP".to_owned(), display_name),
            board: read_dmi(root, "board_name").unwrap_or_default(),
            cpu: snapshot.cpu.model.clone().unwrap_or_default(),
            dgpu: dgpu_name(snapshot).unwrap_or_default(),
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
    fn names() {
        assert_eq!(display_name("ASUS TUF Gaming A15"), "TUF GAMING A15");
        assert_eq!(display_name("ROG Zephyrus G14"), "ROG ZEPHYRUS G14");
        assert_eq!(parse_hex_id("0x10de\n"), Some(0x10de));
    }
}
