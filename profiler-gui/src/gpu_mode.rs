//! Connects the Graphics global (GPU Mode page) to `graphics`.

use std::path::Path;
use std::thread;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use sys_monitor::{GpuStats, Monitor};

use crate::graphics::{self, Facts};
use crate::{AppWindow, Graphics};

/// "nvidia 595.99.02 · 0000:01:00.0" from what sysfs offers.
fn detail(gpu: &GpuStats, extra: &str) -> String {
    let mut driver = gpu.driver.clone().unwrap_or_else(|| "no driver".to_owned());
    if driver == "nvidia"
        && let Ok(v) = std::fs::read_to_string("/sys/module/nvidia/version")
    {
        driver = format!("nvidia {}", v.trim());
    }
    let mut parts = vec![driver];
    parts.extend(gpu.pci_address.clone());
    if !extra.is_empty() {
        parts.push(extra.to_owned());
    }
    parts.join(" · ")
}

fn apply(ui: &AppWindow, igpu: String, dgpu: String, facts: &Facts) {
    let g = ui.global::<Graphics>();
    g.set_igpu_detail(igpu.into());
    g.set_dgpu_detail(dgpu.into());
    let users: Vec<SharedString> = facts.users.iter().map(|u| u.as_str().into()).collect();
    g.set_users(ModelRc::new(VecModel::from(users)));
    g.set_can_sleep(facts.can_sleep.map_or(-1, i32::from));
    g.set_eco(facts.dgpu_disabled == Some(true));
    g.set_has_mux(facts.has_mux);
    g.set_has_powerd(facts.has_powerd);
}

pub fn wire(ui: &AppWindow) {
    let weak = ui.as_weak();
    ui.global::<Graphics>().on_refresh(move || {
        let weak = weak.clone();
        let spawned = thread::Builder::new()
            .name("graphics".to_owned())
            .spawn(move || {
                // A one-off sample: names, drivers and addresses. It reads
                // no metrics from a sleeping dGPU, so it does not wake it.
                let snapshot = Monitor::new().sample();
                let igpu = snapshot.gpus.iter().find(|g| g.boot_vga);
                let dgpu = snapshot.gpus.iter().find(|g| !g.boot_vga);
                let igpu_detail = igpu
                    .map(|g| detail(g, "drives the built-in screen"))
                    .unwrap_or_default();
                let dgpu_detail = dgpu.map(|g| detail(g, "")).unwrap_or_default();
                let facts =
                    graphics::facts(Path::new("/"), dgpu.and_then(|g| g.pci_address.as_deref()));
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    apply(&ui, igpu_detail, dgpu_detail, &facts);
                });
            });
        if let Err(e) = spawned {
            log::error!("Could not start the graphics thread: {e}");
        }
    });
}
