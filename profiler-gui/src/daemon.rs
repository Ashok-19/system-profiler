//! Talks to asusd over the system bus on a worker thread.
//!
//! The UI never blocks on D-Bus: callbacks queue a [`Command`], and the
//! worker re-reads the daemon state after each command and every few
//! seconds (so changes made elsewhere, e.g. with Fn+F5, show up).

use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use rog_dbus::zbus_platform::PlatformProxyBlocking;
use rog_platform::platform::PlatformProfile;
use slint::{ComponentHandle, Weak};

use crate::{AppWindow, Device};

const REFRESH: Duration = Duration::from_secs(2);
const RECONNECT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    SetMode(i32),
    SetChargeLimit(u8),
}

/// What the UI shows about asusd.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DeviceState {
    version: Option<String>,
    /// UI mode index, see [`mode_from_profile`].
    mode: i32,
    charge_limit: i32,
}

impl DeviceState {
    const fn disconnected() -> Self {
        Self {
            version: None,
            mode: -1,
            charge_limit: -1,
        }
    }
}

/// Firmware profile -> UI mode (0 Silent, 1 Performance, 2 Turbo).
///
/// ASUS names the TUF firmware profiles Silent/Performance/Turbo; asusd
/// calls the same profiles Quiet/Balanced/Performance.
const fn mode_from_profile(profile: PlatformProfile) -> i32 {
    match profile {
        PlatformProfile::Quiet => 0,
        PlatformProfile::Balanced => 1,
        PlatformProfile::Performance => 2,
        PlatformProfile::LowPower | PlatformProfile::Custom => -1,
    }
}

const fn profile_from_mode(mode: i32) -> Option<PlatformProfile> {
    match mode {
        0 => Some(PlatformProfile::Quiet),
        1 => Some(PlatformProfile::Balanced),
        2 => Some(PlatformProfile::Performance),
        _ => None,
    }
}

fn read_state(proxy: &PlatformProxyBlocking<'_>) -> DeviceState {
    DeviceState {
        version: proxy.version().ok(),
        mode: proxy.platform_profile().map_or(-1, mode_from_profile),
        charge_limit: proxy.charge_control_end_threshold().map_or(-1, i32::from),
    }
}

fn execute(proxy: &PlatformProxyBlocking<'_>, command: Command) {
    let result = match command {
        Command::SetMode(mode) => match profile_from_mode(mode) {
            Some(profile) => proxy.set_platform_profile(profile),
            None => return,
        },
        Command::SetChargeLimit(limit) => proxy.set_charge_control_end_threshold(limit),
    };
    if let Err(e) = result {
        log::warn!("asusd rejected {command:?}: {e}");
    }
}

fn publish(ui: &Weak<AppWindow>, state: DeviceState) -> bool {
    ui.upgrade_in_event_loop(move |ui| {
        let device = ui.global::<Device>();
        device.set_daemon_ok(state.version.is_some());
        device.set_daemon_version(state.version.unwrap_or_default().into());
        device.set_mode(state.mode);
        device.set_charge_limit(state.charge_limit);
    })
    .is_ok()
}

fn run(ui: &Weak<AppWindow>, commands: &Receiver<Command>) {
    loop {
        let connection = zbus::blocking::Connection::system()
            .and_then(|c| PlatformProxyBlocking::new(&c).map(|p| (c, p)));
        let Ok((_connection, proxy)) = connection else {
            log::warn!("Cannot reach asusd on the system bus; retrying");
            if !publish(ui, DeviceState::disconnected()) {
                return;
            }
            thread::sleep(RECONNECT);
            continue;
        };

        loop {
            let state = read_state(&proxy);
            let connected = state.version.is_some();
            if !publish(ui, state) {
                return;
            }
            if !connected {
                thread::sleep(RECONNECT);
                break;
            }
            match commands.recv_timeout(REFRESH) {
                Ok(command) => execute(&proxy, command),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

/// Wire the Device callbacks and start the worker.
pub fn spawn(ui: &AppWindow) {
    let (tx, rx) = mpsc::channel();
    let device = ui.global::<Device>();
    let mode_tx = tx.clone();
    device.on_set_mode(move |mode| {
        let _ = mode_tx.send(Command::SetMode(mode));
    });
    device.on_set_charge_limit(move |limit| {
        if let Ok(limit) = u8::try_from(limit) {
            let _ = tx.send(Command::SetChargeLimit(limit));
        }
    });

    let weak = ui.as_weak();
    let spawned = thread::Builder::new()
        .name("asusd".to_owned())
        .spawn(move || run(&weak, &rx));
    if let Err(e) = spawned {
        log::error!("Could not start the asusd thread: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_mapping_round_trips() {
        for mode in 0..3 {
            let profile = profile_from_mode(mode).unwrap();
            assert_eq!(mode_from_profile(profile), mode);
        }
        assert_eq!(profile_from_mode(-1), None);
        assert_eq!(profile_from_mode(3), None);
        assert_eq!(mode_from_profile(PlatformProfile::LowPower), -1);
    }

    #[test]
    fn asus_names_match_firmware_profiles() {
        // Silent = quiet, Performance = balanced, Turbo = performance.
        assert_eq!(profile_from_mode(0), Some(PlatformProfile::Quiet));
        assert_eq!(profile_from_mode(1), Some(PlatformProfile::Balanced));
        assert_eq!(profile_from_mode(2), Some(PlatformProfile::Performance));
    }
}
