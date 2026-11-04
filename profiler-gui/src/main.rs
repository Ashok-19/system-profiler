//! System Profiler: an Armoury Crate-style control center for ASUS laptops.

mod alerts;
mod autostart;
mod config;
mod daemon;
mod display;
mod gpu_mode;
mod graphics;
mod history;
mod machine;
mod monitor;
mod notify;
mod prefs;
mod profiles;
mod screen;
mod shared;
mod view;

use std::sync::Arc;

use env_logger::Env;
use slint::{ComponentHandle, ModelRc, VecModel};
use sys_monitor::Monitor;

use crate::machine::MachineInfo;
use crate::shared::Shared;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info,zbus=warn,tracing=warn"))
        .format_timestamp(None)
        .init();

    // The software renderer needs ~30 MiB of RAM; OpenGL needs ~110 MiB for
    // the same window. `SLINT_BACKEND` still overrides this for testing.
    if std::env::var_os("SLINT_BACKEND").is_none() {
        slint::BackendSelector::new()
            .backend_name("winit".to_owned())
            .renderer_name("software".to_owned())
            .select()?;
    }

    let ui = AppWindow::new()?;
    ui.set_app_version(env!("CARGO_PKG_VERSION").into());
    let shared = Arc::new(Shared::default());
    prefs::install(&ui, &shared);

    let mut monitor = Monitor::new();
    let first = monitor.sample();
    let info = MachineInfo::detect(&first);
    let machine = ui.global::<Machine>();
    machine.set_name(info.name.into());
    machine.set_board(info.board.into());
    machine.set_cpu(info.cpu.into());
    machine.set_dgpu(info.dgpu.into());
    machine.set_igpu(info.igpu.into());
    machine.set_cpu_threads(i32::try_from(info.cpu_threads).unwrap_or(0));
    machine.set_cpufreq_driver(info.cpufreq_driver.into());
    let details: Vec<Fact> = info
        .details
        .into_iter()
        .map(|(key, value)| Fact {
            key: key.into(),
            value: value.into(),
        })
        .collect();
    machine.set_details(ModelRc::new(VecModel::from(details)));

    monitor::spawn(&ui, monitor, &shared);
    daemon::spawn(&ui, &shared);
    screen::wire(&ui);
    profiles::wire(&ui);
    // Know the current refresh rate, so automatic profiles can restore it.
    ui.global::<Screen>().invoke_refresh();
    gpu_mode::wire(&ui);

    ui.run()
}
