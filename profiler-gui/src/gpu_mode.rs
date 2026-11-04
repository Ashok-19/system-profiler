//! Connects the Graphics global (GPU Mode page) to `graphics` and the
//! optional LACT daemon.

use std::path::Path;
use std::sync::{Mutex, PoisonError};
use std::thread;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use sys_monitor::{GpuStats, Monitor};

use crate::graphics::{self, Facts};
use crate::lact::{self, Igpu};
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

/// What lactd says about the integrated GPU.
#[derive(Debug, Clone)]
enum LactView {
    /// lactd is not installed or not running.
    Absent,
    Ready(Igpu),
    /// lactd runs but cannot report the iGPU.
    Unavailable(String),
}

/// The last iGPU reading, to turn UI indices into lactd's ids.
static LACT_IGPU: Mutex<Option<Igpu>> = Mutex::new(None);

fn igpu_address() -> Option<String> {
    Monitor::new()
        .sample()
        .gpus
        .into_iter()
        .find(|g| g.boot_vga)
        .and_then(|g| g.pci_address)
}

fn read_lact(igpu_pci: Option<&str>) -> LactView {
    if !lact::is_running() {
        return LactView::Absent;
    }
    let view = match igpu_pci {
        None => LactView::Unavailable("No integrated GPU found".to_owned()),
        Some(address) => lact::igpu(address).map_or_else(LactView::Unavailable, LactView::Ready),
    };
    *LACT_IGPU.lock().unwrap_or_else(PoisonError::into_inner) = match &view {
        LactView::Ready(igpu) => Some(igpu.clone()),
        _ => None,
    };
    view
}

fn apply_lact(ui: &AppWindow, view: &LactView) {
    let g = ui.global::<Graphics>();
    g.set_lact_busy(false);
    match view {
        LactView::Absent => g.set_lact_state(0),
        LactView::Unavailable(why) => {
            g.set_lact_state(2);
            g.set_lact_error(why.as_str().into());
        }
        LactView::Ready(igpu) => {
            g.set_lact_state(1);
            g.set_lact_level(igpu.level.and_then(|l| i32::try_from(l).ok()).unwrap_or(-1));
            let names: Vec<SharedString> =
                igpu.modes.iter().map(|(_, n)| n.as_str().into()).collect();
            g.set_lact_modes(ModelRc::new(VecModel::from(names)));
            let active = igpu
                .active_mode
                .and_then(|a| igpu.modes.iter().position(|&(i, _)| i == a))
                .and_then(|p| i32::try_from(p).ok());
            g.set_lact_mode(active.unwrap_or(-1));
        }
    }
}

/// Change something through lactd off the UI thread, then show the result.
fn change_lact(
    ui: &slint::Weak<AppWindow>,
    change: impl FnOnce(&Igpu) -> Result<(), String> + Send + 'static,
) {
    let ui = ui.clone();
    let spawned = thread::Builder::new()
        .name("lact".to_owned())
        .spawn(move || {
            let current = LACT_IGPU
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            if let Some(igpu) = current
                && let Err(e) = change(&igpu)
            {
                log::warn!("lactd refused the change: {e}");
            }
            let view = read_lact(igpu_address().as_deref());
            let _ = ui.upgrade_in_event_loop(move |ui| apply_lact(&ui, &view));
        });
    if let Err(e) = spawned {
        log::error!("Could not start the LACT thread: {e}");
    }
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
                let lact = read_lact(igpu.and_then(|g| g.pci_address.as_deref()));
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    apply(&ui, igpu_detail, dgpu_detail, &facts);
                    apply_lact(&ui, &lact);
                });
            });
        if let Err(e) = spawned {
            log::error!("Could not start the graphics thread: {e}");
        }
    });

    let g = ui.global::<Graphics>();
    let weak = ui.as_weak();
    g.on_set_lact_level(move |level| {
        let Ok(level) = usize::try_from(level) else {
            return;
        };
        change_lact(&weak, move |igpu| lact::set_level(&igpu.id, level));
    });
    let weak = ui.as_weak();
    g.on_set_lact_mode(move |position| {
        let Ok(position) = usize::try_from(position) else {
            return;
        };
        change_lact(&weak, move |igpu| {
            let &(index, _) = igpu.modes.get(position).ok_or("unknown power profile")?;
            lact::set_mode(&igpu.id, index)
        });
    });
}
