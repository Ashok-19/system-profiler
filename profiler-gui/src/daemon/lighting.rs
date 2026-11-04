//! asusd's aura interface: keyboard effect, colour, speed, brightness and
//! when the backlight is lit.

use std::sync::mpsc::Sender;

use rog_aura::keyboard::{AuraPowerState, LaptopAuraPower};
use rog_aura::{
    AuraEffect, AuraModeNum, AuraZone, Colour, Direction, LedBrightness, PowerZones, Speed,
};
use rog_dbus::zbus_aura::AuraProxyBlocking;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::{AppWindow, Effect, Lighting};

/// Effects this app offers, in display order, if the keyboard supports them.
const EFFECTS: [(AuraModeNum, &str); 5] = [
    (AuraModeNum::Static, "Static"),
    (AuraModeNum::Breathe, "Breathing"),
    (AuraModeNum::RainbowCycle, "Color Cycle"),
    (AuraModeNum::RainbowWave, "Rainbow"),
    (AuraModeNum::Pulse, "Strobing"),
];

/// A change requested from the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Effect(Settings),
    Brightness(i32),
    /// Lit while booting, awake, asleep, shutting down.
    Power([bool; 4]),
}

/// One effect with its parameters, in UI terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// `AuraModeNum` discriminant.
    pub mode: i32,
    pub colour: (u8, u8, u8),
    /// 0 slow, 1 medium, 2 fast.
    pub speed: i32,
    /// 0 right, 1 left.
    pub direction: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// Supported effects as `AuraModeNum` discriminants, in display order.
    pub modes: Vec<i32>,
    pub settings: Option<Settings>,
    /// 0 off to 3 high; -1 unknown.
    pub brightness: i32,
    pub power: Option<[bool; 4]>,
}

impl State {
    pub const fn unavailable() -> Self {
        Self {
            modes: Vec::new(),
            settings: None,
            brightness: -1,
            power: None,
        }
    }
}

const fn speed_index(speed: Speed) -> i32 {
    match speed {
        Speed::Low => 0,
        Speed::Med => 1,
        Speed::High => 2,
    }
}

const fn speed_from_index(index: i32) -> Speed {
    match index {
        0 => Speed::Low,
        2 => Speed::High,
        _ => Speed::Med,
    }
}

const fn brightness_from_index(index: i32) -> Option<LedBrightness> {
    match index {
        0 => Some(LedBrightness::Off),
        1 => Some(LedBrightness::Low),
        2 => Some(LedBrightness::Med),
        3 => Some(LedBrightness::High),
        _ => None,
    }
}

fn mode_from_index(index: i32) -> Option<AuraModeNum> {
    EFFECTS
        .iter()
        .map(|&(mode, _)| mode)
        .find(|&mode| mode as i32 == index)
}

fn settings_from(effect: &AuraEffect) -> Settings {
    Settings {
        mode: effect.mode as i32,
        colour: (effect.colour1.r, effect.colour1.g, effect.colour1.b),
        speed: speed_index(effect.speed),
        direction: i32::from(effect.direction == Direction::Left),
    }
}

fn effect_from(settings: Settings) -> Option<AuraEffect> {
    let (r, g, b) = settings.colour;
    Some(AuraEffect {
        mode: mode_from_index(settings.mode)?,
        zone: AuraZone::None,
        colour1: Colour { r, g, b },
        colour2: Colour { r: 0, g: 0, b: 0 },
        speed: speed_from_index(settings.speed),
        direction: if settings.direction == 1 {
            Direction::Left
        } else {
            Direction::Right
        },
    })
}

const fn power_flags(state: AuraPowerState) -> [bool; 4] {
    [
        state.boot, state.awake, state.sleep, state.shutdown,
    ]
}

fn keyboard_power(power: &LaptopAuraPower) -> Option<&AuraPowerState> {
    power.states.iter().find(|s| s.zone == PowerZones::Keyboard)
}

pub fn read(proxy: &AuraProxyBlocking<'_>) -> State {
    let supported = proxy.supported_basic_modes().unwrap_or_default();
    State {
        modes: EFFECTS
            .iter()
            .filter(|(mode, _)| supported.contains(mode))
            .map(|&(mode, _)| mode as i32)
            .collect(),
        settings: proxy.led_mode_data().ok().map(|e| settings_from(&e)),
        brightness: proxy.brightness().map_or(-1, |b| b as i32),
        power: proxy
            .led_power()
            .ok()
            .and_then(|p| keyboard_power(&p).copied().map(power_flags)),
    }
}

/// Re-read what keyboard hotkeys can change; [`read`] covers the rest.
pub fn refresh(proxy: &AuraProxyBlocking<'_>, state: &mut State) {
    state.brightness = proxy.brightness().map_or(-1, |b| b as i32);
}

pub fn execute(proxy: &AuraProxyBlocking<'_>, command: Command) {
    let result = match command {
        Command::Effect(settings) => match effect_from(settings) {
            Some(effect) => proxy.set_led_mode_data(effect),
            None => return,
        },
        Command::Brightness(index) => match brightness_from_index(index) {
            Some(level) => proxy.set_brightness(level),
            None => return,
        },
        Command::Power(
            [
                boot,
                awake,
                sleep,
                shutdown,
            ],
        ) => proxy.led_power().and_then(|mut power| {
            for state in &mut power.states {
                if state.zone == PowerZones::Keyboard {
                    state.boot = boot;
                    state.awake = awake;
                    state.sleep = sleep;
                    state.shutdown = shutdown;
                }
            }
            proxy.set_led_power(power)
        }),
    };
    if let Err(e) = result {
        log::warn!("asusd rejected {command:?}: {e}");
    }
}

/// Show `state` in the Lighting global. Runs on the UI thread.
pub fn apply(ui: &AppWindow, state: &State) {
    let lighting = ui.global::<Lighting>();
    lighting.set_available(!state.modes.is_empty());
    if lighting.get_effects().row_count() != state.modes.len() {
        let effects: Vec<Effect> = EFFECTS
            .iter()
            .filter(|(mode, _)| state.modes.contains(&(*mode as i32)))
            .map(|&(mode, name)| Effect {
                mode: mode as i32,
                name: SharedString::from(name),
            })
            .collect();
        lighting.set_effects(ModelRc::new(VecModel::from(effects)));
    }
    if let Some(s) = state.settings {
        lighting.set_mode(s.mode);
        let (r, g, b) = s.colour;
        lighting.set_colour(slint::Color::from_rgb_u8(r, g, b));
        lighting.set_speed(s.speed);
        lighting.set_direction(s.direction);
    }
    lighting.set_brightness(state.brightness);
    if let Some(
        [
            boot,
            awake,
            sleep,
            shutdown,
        ],
    ) = state.power
    {
        lighting.set_on_boot(boot);
        lighting.set_on_awake(awake);
        lighting.set_on_sleep(sleep);
        lighting.set_on_shutdown(shutdown);
    }
}

/// Route the Lighting callbacks to the worker.
pub fn wire(ui: &AppWindow, tx: &Sender<super::Command>) {
    let lighting = ui.global::<Lighting>();
    let send = || super::sender(tx, super::Command::Lighting);

    // The effect is sent whole, so read every field from the UI.
    let s = send();
    let weak = ui.as_weak();
    lighting.on_apply_effect(move || {
        let Some(ui) = weak.upgrade() else { return };
        let l = ui.global::<Lighting>();
        let c = l.get_colour();
        s(Command::Effect(Settings {
            mode: l.get_mode(),
            colour: (c.red(), c.green(), c.blue()),
            speed: l.get_speed(),
            direction: l.get_direction(),
        }));
    });
    let s = send();
    lighting.on_set_brightness(move |level| s(Command::Brightness(level)));
    let s = send();
    let weak = ui.as_weak();
    lighting.on_apply_power(move || {
        let Some(ui) = weak.upgrade() else { return };
        let l = ui.global::<Lighting>();
        s(Command::Power([
            l.get_on_boot(),
            l.get_on_awake(),
            l.get_on_sleep(),
            l.get_on_shutdown(),
        ]));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_round_trips() {
        let settings = Settings {
            mode: AuraModeNum::RainbowWave as i32,
            colour: (255, 165, 26),
            speed: 2,
            direction: 1,
        };
        let effect = effect_from(settings).unwrap();
        assert_eq!(effect.mode, AuraModeNum::RainbowWave);
        assert_eq!(effect.speed, Speed::High);
        assert_eq!(effect.direction, Direction::Left);
        assert_eq!(settings_from(&effect), settings);
    }

    #[test]
    fn unknown_values_are_rejected_or_defaulted() {
        let mut settings = Settings {
            mode: AuraModeNum::Star as i32,
            colour: (0, 0, 0),
            speed: 9,
            direction: 0,
        };
        assert!(effect_from(settings).is_none(), "Star is not offered");
        settings.mode = 0;
        assert_eq!(effect_from(settings).unwrap().speed, Speed::Med);
        assert_eq!(brightness_from_index(4), None);
        assert_eq!(brightness_from_index(0), Some(LedBrightness::Off));
    }
}
