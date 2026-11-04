//! asusd's platform interface: operating mode, charge limit, CPU energy
//! preference and automatic switching.

use std::sync::mpsc::Sender;

use rog_dbus::zbus_platform::PlatformProxyBlocking;
use rog_platform::cpu::CPUEPP;
use rog_platform::platform::PlatformProfile;
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::{AppWindow, Device};

const CPU0_EPP: &str = "/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference";
/// Number of UI modes (Silent, Performance, Turbo).
const MODES: usize = 3;

/// A change requested from the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// UI mode index.
    Mode(i32),
    ChargeLimit(u8),
    OneShotCharge,
    AutoSwitch(bool),
    ModeOnAc(i32),
    ModeOnBattery(i32),
    LinkedEpp(bool),
    ModeEpp {
        mode: i32,
        epp: i32,
    },
}

/// What the UI shows about asusd.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    version: Option<String>,
    /// UI mode index, see [`mode_from_profile`]; -1 if unknown.
    mode: i32,
    charge_limit: i32,
    auto_switch: bool,
    mode_on_ac: i32,
    mode_on_battery: i32,
    linked_epp: bool,
    /// EPP index per UI mode, see [`epp_index`].
    mode_epp: [i32; MODES],
    /// Live EPP of cpu0, e.g. `balance_power`.
    cpu_epp: String,
}

impl State {
    /// UI mode index; -1 if unknown.
    pub const fn mode(&self) -> i32 {
        self.mode
    }

    /// asusd answered (it reported its version).
    pub const fn is_connected(&self) -> bool {
        self.version.is_some()
    }

    pub const fn disconnected() -> Self {
        Self {
            version: None,
            mode: -1,
            charge_limit: -1,
            auto_switch: false,
            mode_on_ac: -1,
            mode_on_battery: -1,
            linked_epp: false,
            mode_epp: [-1; MODES],
            cpu_epp: String::new(),
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

/// Name and one-line summary of a UI mode, for notifications.
pub const fn describe_mode(mode: i32) -> Option<(&'static str, &'static str)> {
    match mode {
        0 => Some(("Silent", "Quiet fans; cool and efficient")),
        1 => Some(("Performance", "Balanced speed and noise")),
        2 => Some(("Turbo", "Highest clocks; fans at full speed")),
        _ => None,
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

/// UI index of an EPP choice; the UI lists them in `CPUEPP` order.
const fn epp_index(epp: CPUEPP) -> i32 {
    match epp {
        CPUEPP::Default => 0,
        CPUEPP::Performance => 1,
        CPUEPP::BalancePerformance => 2,
        CPUEPP::BalancePower => 3,
        CPUEPP::Power => 4,
    }
}

const fn epp_from_index(index: i32) -> Option<CPUEPP> {
    match index {
        0 => Some(CPUEPP::Default),
        1 => Some(CPUEPP::Performance),
        2 => Some(CPUEPP::BalancePerformance),
        3 => Some(CPUEPP::BalancePower),
        4 => Some(CPUEPP::Power),
        _ => None,
    }
}

pub fn read(proxy: &PlatformProxyBlocking<'_>) -> State {
    let mode = |p: zbus::Result<PlatformProfile>| p.map_or(-1, mode_from_profile);
    let epp = |e: zbus::Result<CPUEPP>| e.map_or(-1, epp_index);
    State {
        version: proxy.version().ok(),
        mode: mode(proxy.platform_profile()),
        charge_limit: proxy.charge_control_end_threshold().map_or(-1, i32::from),
        // The UI has one switch; asusd keeps a flag per power source.
        auto_switch: proxy.change_platform_profile_on_ac().unwrap_or(false)
            || proxy.change_platform_profile_on_battery().unwrap_or(false),
        mode_on_ac: mode(proxy.platform_profile_on_ac()),
        mode_on_battery: mode(proxy.platform_profile_on_battery()),
        linked_epp: proxy.platform_profile_linked_epp().unwrap_or(false),
        mode_epp: [
            epp(proxy.profile_quiet_epp()),
            epp(proxy.profile_balanced_epp()),
            epp(proxy.profile_performance_epp()),
        ],
        cpu_epp: read_cpu_epp(),
    }
}

/// Re-read only what can change behind the app's back at any moment
/// (Fn+F5, unplugging, asusctl); [`read`] covers the rest.
pub fn refresh(proxy: &PlatformProxyBlocking<'_>, state: &mut State) {
    state.version = proxy.version().ok();
    state.mode = proxy.platform_profile().map_or(-1, mode_from_profile);
    state.charge_limit = proxy.charge_control_end_threshold().map_or(-1, i32::from);
    state.cpu_epp = read_cpu_epp();
}

fn read_cpu_epp() -> String {
    std::fs::read_to_string(CPU0_EPP)
        .map(|v| v.trim().to_owned())
        .unwrap_or_default()
}

pub fn execute(proxy: &PlatformProxyBlocking<'_>, command: Command) {
    let result = match command {
        Command::Mode(mode) => match profile_from_mode(mode) {
            Some(profile) => proxy.set_platform_profile(profile),
            None => return,
        },
        Command::ChargeLimit(limit) => proxy.set_charge_control_end_threshold(limit),
        Command::OneShotCharge => proxy.one_shot_full_charge(),
        Command::AutoSwitch(on) => proxy
            .set_change_platform_profile_on_ac(on)
            .and_then(|()| proxy.set_change_platform_profile_on_battery(on)),
        Command::ModeOnAc(mode) => match profile_from_mode(mode) {
            Some(profile) => proxy.set_platform_profile_on_ac(profile),
            None => return,
        },
        Command::ModeOnBattery(mode) => match profile_from_mode(mode) {
            Some(profile) => proxy.set_platform_profile_on_battery(profile),
            None => return,
        },
        Command::LinkedEpp(on) => proxy.set_platform_profile_linked_epp(on),
        Command::ModeEpp { mode, epp } => match (profile_from_mode(mode), epp_from_index(epp)) {
            (Some(PlatformProfile::Quiet), Some(epp)) => proxy.set_profile_quiet_epp(epp),
            (Some(PlatformProfile::Balanced), Some(epp)) => proxy.set_profile_balanced_epp(epp),
            (Some(PlatformProfile::Performance), Some(epp)) => {
                proxy.set_profile_performance_epp(epp)
            }
            _ => return,
        },
    };
    if let Err(e) = result {
        log::warn!("asusd rejected {command:?}: {e}");
    }
}

/// Show `state` in the Device global. Runs on the UI thread.
pub fn apply(ui: &AppWindow, state: State) {
    crate::tray::set_state(state.mode, state.charge_limit);
    let device = ui.global::<Device>();
    device.set_daemon_ok(state.version.is_some());
    device.set_daemon_version(state.version.unwrap_or_default().into());
    device.set_mode(state.mode);
    device.set_charge_limit(state.charge_limit);
    device.set_auto_switch(state.auto_switch);
    device.set_mode_on_ac(state.mode_on_ac);
    device.set_mode_on_battery(state.mode_on_battery);
    device.set_linked_epp(state.linked_epp);
    device.set_cpu_epp(state.cpu_epp.into());
    // Update rows in place so bound widgets are not rebuilt.
    let mode_epp = device.get_mode_epp();
    for (row, epp) in state.mode_epp.into_iter().enumerate() {
        if mode_epp.row_data(row) != Some(epp) {
            mode_epp.set_row_data(row, epp);
        }
    }
}

/// Route the Device callbacks to the worker.
pub fn wire(ui: &AppWindow, tx: &Sender<super::Command>) {
    let device = ui.global::<Device>();
    device.set_mode_epp(ModelRc::new(VecModel::from(vec![-1; MODES])));
    let send = || super::sender(tx, super::Command::Platform);

    let s = send();
    device.on_set_mode(move |mode| s(Command::Mode(mode)));
    let s = send();
    device.on_set_charge_limit(move |limit| {
        if let Ok(limit) = u8::try_from(limit) {
            s(Command::ChargeLimit(limit));
        }
    });
    let s = send();
    device.on_one_shot_charge(move || s(Command::OneShotCharge));
    let s = send();
    device.on_set_auto_switch(move |on| s(Command::AutoSwitch(on)));
    let s = send();
    device.on_set_mode_on_ac(move |mode| s(Command::ModeOnAc(mode)));
    let s = send();
    device.on_set_mode_on_battery(move |mode| s(Command::ModeOnBattery(mode)));
    let s = send();
    device.on_set_linked_epp(move |on| s(Command::LinkedEpp(on)));
    let s = send();
    device.on_set_mode_epp(move |mode, epp| s(Command::ModeEpp { mode, epp }));
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
    fn epp_mapping_round_trips() {
        for index in 0..5 {
            let epp = epp_from_index(index).unwrap();
            assert_eq!(epp_index(epp), index);
        }
        assert_eq!(epp_from_index(-1), None);
        assert_eq!(epp_from_index(5), None);
    }

    #[test]
    fn asus_names_match_firmware_profiles() {
        // Silent = quiet, Performance = balanced, Turbo = performance.
        assert_eq!(profile_from_mode(0), Some(PlatformProfile::Quiet));
        assert_eq!(profile_from_mode(1), Some(PlatformProfile::Balanced));
        assert_eq!(profile_from_mode(2), Some(PlatformProfile::Performance));
    }
}
