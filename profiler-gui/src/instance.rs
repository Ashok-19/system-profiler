//! One instance per session: the first claims a name on the session
//! bus; later launches ask it to show its window and exit.

use slint::{ComponentHandle, Weak};
use zbus::blocking::{Connection, connection};
use zbus::interface;

use crate::{AppWindow, tray};

const NAME: &str = "io.github.ashok19.SystemProfiler";
const PATH: &str = "/io/github/ashok19/SystemProfiler";

struct Remote {
    ui: Weak<AppWindow>,
}

#[interface(name = "io.github.ashok19.SystemProfiler")]
impl Remote {
    /// Show and raise the window.
    fn show(&self) {
        tray::show_window(&self.ui);
    }
}

/// If another instance is running, ask it to show itself and return true.
pub fn hand_over_to_running() -> bool {
    let Ok(bus) = Connection::session() else {
        return false;
    };
    let reply = bus.call_method(Some(NAME), PATH, Some(NAME), "Show", &());
    match reply {
        Ok(_) => {
            log::info!("System Profiler is already running; showing its window");
            true
        }
        // No owner of the name (ServiceUnknown) is the normal case.
        Err(_) => false,
    }
}

/// Claim the name so later launches find this instance. Keep the returned
/// connection alive for the life of the app.
pub fn claim(ui: &AppWindow) -> Option<Connection> {
    connection::Builder::session()
        .and_then(|b| b.name(NAME))
        .and_then(|b| b.serve_at(PATH, Remote { ui: ui.as_weak() }))
        .and_then(connection::Builder::build)
        .inspect_err(|e| log::warn!("Cannot claim {NAME} on the session bus: {e}"))
        .ok()
}
