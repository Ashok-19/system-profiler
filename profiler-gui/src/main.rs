//! System Profiler: an Armoury Crate-style control center for ASUS laptops.

mod config;
mod daemon;
mod history;
mod machine;
mod monitor;
mod prefs;
mod view;

use env_logger::Env;
use slint::{ComponentHandle, ModelRc, VecModel};
use sys_monitor::Monitor;

use crate::machine::MachineInfo;

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
    prefs::install(&ui);

    let mut monitor = Monitor::new();
    let first = monitor.sample();
    let info = MachineInfo::detect(&first);
    let machine = ui.global::<Machine>();
    machine.set_name(info.name.into());
    machine.set_board(info.board.into());
    machine.set_cpu(info.cpu.into());
    machine.set_dgpu(info.dgpu.into());
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

    monitor::spawn(&ui, monitor);
    daemon::spawn(&ui);

    ui.run()
}
