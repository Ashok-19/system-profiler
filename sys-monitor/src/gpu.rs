//! GPU power state and, for awake GPUs, live metrics.
//!
//! Reading most GPU attributes resumes a runtime-suspended device. On hybrid
//! laptops that would keep the dGPU powered and drain the battery, so metrics
//! are only read from GPUs whose PCI runtime PM status is already `active`.

use std::path::Path;

use crate::cpu::millidegrees_to_c;
use crate::sysfs::{self, Hwmon};

/// PCI runtime power-management state of a GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    /// Powered and usable (includes devices without runtime PM).
    Active,
    /// Powered down by runtime PM (`suspended` or `suspending`).
    Suspended,
    /// Transitioning or in an error state.
    Unknown,
}

impl PowerState {
    fn from_runtime_status(status: Option<&str>) -> Self {
        match status {
            // `unsupported` means runtime PM is disabled: the device is always on.
            Some("active" | "unsupported") => Self::Active,
            Some("suspended" | "suspending") => Self::Suspended,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GpuStats {
    /// DRM card name, e.g. `card1`.
    pub card: String,
    /// Kernel driver, e.g. `amdgpu`, `nvidia`, `i915`.
    pub driver: Option<String>,
    /// PCI address, e.g. `0000:01:00.0`.
    pub pci_address: Option<String>,
    /// PCI vendor and device IDs, e.g. `0x10de`, `0x25a2`.
    pub vendor_id: Option<String>,
    pub device_id: Option<String>,
    /// Whether firmware used this GPU for the boot display (usually the iGPU
    /// on hybrid laptops).
    pub boot_vga: bool,
    pub power_state: PowerState,
    /// The remaining fields are only filled for `Active` GPUs whose driver
    /// exposes them through sysfs (currently `amdgpu`).
    pub usage_percent: Option<f32>,
    pub clock_mhz: Option<u32>,
    pub temp_c: Option<f32>,
    /// Power draw in watts. For AMD APUs this is the whole package (PPT).
    pub power_w: Option<f32>,
    pub vram_used_bytes: Option<u64>,
    pub vram_total_bytes: Option<u64>,
}

fn is_card_name(name: &str) -> bool {
    name.strip_prefix("card")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

#[allow(clippy::cast_precision_loss)] // sensor values are far below 2^24
fn micro_to_unit(v: i64) -> f32 {
    v as f32 / 1_000_000.0
}

fn read_gpu(card_path: &Path) -> Option<GpuStats> {
    let card = card_path.file_name()?.to_str()?.to_owned();
    let dev = card_path.join("device");
    let power_state = PowerState::from_runtime_status(
        sysfs::read_trimmed(&dev.join("power/runtime_status")).as_deref(),
    );
    let mut gpu = GpuStats {
        card,
        driver: sysfs::link_basename(&dev.join("driver")),
        pci_address: sysfs::link_basename(&dev),
        vendor_id: sysfs::read_trimmed(&dev.join("vendor")),
        device_id: sysfs::read_trimmed(&dev.join("device")),
        boot_vga: sysfs::read_u64(&dev.join("boot_vga")) == Some(1),
        power_state,
        usage_percent: None,
        clock_mhz: None,
        temp_c: None,
        power_w: None,
        vram_used_bytes: None,
        vram_total_bytes: None,
    };
    if power_state == PowerState::Active && gpu.driver.as_deref() == Some("amdgpu") {
        fill_amdgpu(&mut gpu, &dev);
    }
    Some(gpu)
}

fn fill_amdgpu(gpu: &mut GpuStats, dev: &Path) {
    #[allow(clippy::cast_precision_loss)] // percentage
    {
        gpu.usage_percent = sysfs::read_u64(&dev.join("gpu_busy_percent")).map(|v| v as f32);
    }
    gpu.vram_used_bytes = sysfs::read_u64(&dev.join("mem_info_vram_used"));
    gpu.vram_total_bytes = sysfs::read_u64(&dev.join("mem_info_vram_total"));

    let Some(path) = sysfs::dir_entries(&dev.join("hwmon"), |n| n.starts_with("hwmon"))
        .into_iter()
        .next()
    else {
        return;
    };
    let chip = Hwmon {
        name: "amdgpu".to_owned(),
        path,
    };
    gpu.temp_c = chip.read_i64("temp1_input").map(millidegrees_to_c);
    gpu.clock_mhz = chip
        .read_i64("freq1_input")
        .and_then(|hz| u32::try_from(hz / 1_000_000).ok());
    // Newer kernels expose power1_input, older ones power1_average.
    gpu.power_w = chip
        .read_i64("power1_average")
        .or_else(|| chip.read_i64("power1_input"))
        .map(micro_to_unit);
}

/// All DRM GPUs, in card order.
pub(crate) fn sample(root: &Path) -> Vec<GpuStats> {
    sysfs::dir_entries(&root.join("sys/class/drm"), is_card_name)
        .iter()
        .filter_map(|p| read_gpu(p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::Fixture;

    /// Mimics the FA506IC: amdgpu iGPU (card2) + runtime-suspended NVIDIA
    /// dGPU (card1), with a connector entry that must be ignored.
    fn hybrid_fixture() -> Fixture {
        let fx = Fixture::new();
        let nv = "sys/devices/pci0000:00/0000:00:01.1/0000:01:00.0";
        let amd = "sys/devices/pci0000:00/0000:00:08.1/0000:06:00.0";

        fx.write(&format!("{nv}/power/runtime_status"), "suspended\n");
        fx.write(&format!("{nv}/vendor"), "0x10de\n");
        fx.write(&format!("{nv}/device"), "0x25a2\n");
        fx.write(&format!("{nv}/boot_vga"), "0\n");
        // If this were read the test would notice a filled usage field.
        fx.write(&format!("{nv}/gpu_busy_percent"), "99\n");
        fx.symlink(
            &format!("{nv}/driver"),
            "../../../../bus/pci/drivers/nvidia",
        );

        fx.write(&format!("{amd}/power/runtime_status"), "active\n");
        fx.write(&format!("{amd}/vendor"), "0x1002\n");
        fx.write(&format!("{amd}/boot_vga"), "1\n");
        fx.write(&format!("{amd}/gpu_busy_percent"), "7\n");
        fx.write(&format!("{amd}/mem_info_vram_used"), "400183296\n");
        fx.write(&format!("{amd}/mem_info_vram_total"), "536870912\n");
        fx.write(&format!("{amd}/hwmon/hwmon8/temp1_input"), "69000\n");
        fx.write(&format!("{amd}/hwmon/hwmon8/freq1_input"), "400000000\n");
        fx.write(&format!("{amd}/hwmon/hwmon8/power1_input"), "40000000\n");
        fx.symlink(
            &format!("{amd}/driver"),
            "../../../../bus/pci/drivers/amdgpu",
        );

        for (card, dev) in [
            ("card1", nv),
            ("card2", amd),
        ] {
            fx.write(&format!("sys/class/drm/{card}/dev"), "226:1\n");
            fx.symlink(
                &format!("sys/class/drm/{card}/device"),
                fx.path(dev).to_str().unwrap(),
            );
        }
        fx.write("sys/class/drm/card2-eDP-1/status", "connected\n");
        fx
    }

    #[test]
    fn hybrid_laptop() {
        let fx = hybrid_fixture();
        let gpus = sample(fx.root());
        assert_eq!(gpus.len(), 2);

        let nv = &gpus[0];
        assert_eq!(nv.card, "card1");
        assert_eq!(nv.driver.as_deref(), Some("nvidia"));
        assert_eq!(nv.pci_address.as_deref(), Some("0000:01:00.0"));
        assert_eq!(nv.power_state, PowerState::Suspended);
        assert!(!nv.boot_vga);
        assert_eq!(nv.usage_percent, None, "must not read a suspended GPU");

        let amd = &gpus[1];
        assert_eq!(amd.driver.as_deref(), Some("amdgpu"));
        assert_eq!(amd.power_state, PowerState::Active);
        assert!(amd.boot_vga);
        assert_eq!(amd.usage_percent, Some(7.0));
        assert_eq!(amd.clock_mhz, Some(400));
        assert_eq!(amd.temp_c, Some(69.0));
        assert_eq!(amd.power_w, Some(40.0));
        assert_eq!(amd.vram_total_bytes, Some(536_870_912));
    }

    #[test]
    fn runtime_status_mapping() {
        use PowerState::{Active, Suspended, Unknown};
        for (s, want) in [
            (Some("active"), Active),
            (Some("unsupported"), Active),
            (Some("suspended"), Suspended),
            (Some("suspending"), Suspended),
            (Some("resuming"), Unknown),
            (None, Unknown),
        ] {
            assert_eq!(PowerState::from_runtime_status(s), want, "{s:?}");
        }
    }

    #[test]
    fn card_names() {
        assert!(is_card_name("card0"));
        assert!(is_card_name("card12"));
        assert!(!is_card_name("card1-eDP-1"));
        assert!(!is_card_name("card"));
        assert!(!is_card_name("renderD128"));
    }
}
