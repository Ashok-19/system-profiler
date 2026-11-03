//! System Profiler: an Armoury Crate-style control center for ASUS laptops.

mod daemon;
mod machine;
mod monitor;
mod view;

use env_logger::Env;
use slint::ComponentHandle;
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

    let mut monitor = Monitor::new();
    let first = monitor.sample();
    let info = MachineInfo::detect(&first);
    let machine = ui.global::<Machine>();
    machine.set_name(info.name.into());
    machine.set_board(info.board.into());
    machine.set_cpu(info.cpu.into());
    machine.set_dgpu(info.dgpu.into());

    monitor::spawn(ui.as_weak(), monitor);
    daemon::spawn(&ui);

    ui.run()
}
