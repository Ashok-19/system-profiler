//! Binds the Prefs global to the config file and to the settings the
//! worker threads read.

use std::rc::Rc;
use std::sync::Arc;

use slint::{Color, ComponentHandle};

use crate::config::{self, Config};
use crate::shared::Shared;
use crate::{AppWindow, Prefs};
use crate::{autostart, notify, profiles};

fn colour_hex(c: Color) -> String {
    config::to_hex((c.red(), c.green(), c.blue()))
}

fn hex_colour(hex: &str) -> Color {
    let (r, g, b) = config::parse_hex(hex)
        .or_else(|| config::parse_hex(config::DEFAULT_ACCENT))
        .unwrap_or((255, 165, 26));
    Color::from_rgb_u8(r, g, b)
}

fn show(ui: &AppWindow, c: &Config) {
    let p = ui.global::<Prefs>();
    p.set_accent(hex_colour(&c.accent));
    p.set_background(i32::from(c.background));
    p.set_glow(i32::from(c.glow));
    p.set_rounded(c.rounded);
    p.set_compact(c.compact);
    p.set_text_size(i32::from(c.text_size));
    p.set_icons_only_sidebar(c.icons_only_sidebar);
    p.set_show_hero(c.show_hero);
    p.set_match_keyboard(c.match_keyboard);
    p.set_fahrenheit(c.fahrenheit);
    p.set_interval(i32::from(c.interval_s));
    p.set_notify_mode(c.notify_mode);
    p.set_notify_power(c.notify_power);
    p.set_hot_cpu(i32::from(c.hot_cpu_c));
    p.set_battery_60hz(c.battery_60hz);
    profiles::show(ui, &c.profiles, c.auto_profiles);
}

/// Hand the settings the worker threads use over to them.
fn share(c: &Config, shared: &Shared) {
    shared.set_fahrenheit(c.fahrenheit);
    shared.set_interval_s(u32::from(c.interval_s));
    shared.set_notify_mode(c.notify_mode);
    shared.set_notify_power(c.notify_power);
    shared.set_hot_cpu_c(u32::from(c.hot_cpu_c));
    shared.set_battery_60hz(c.battery_60hz);
    shared.set_auto_profiles(c.auto_profiles);
    shared.set_triggers(profiles::triggers(&c.profiles));
}

fn read(ui: &AppWindow) -> Config {
    let p = ui.global::<Prefs>();
    let byte = |v: i32| u8::try_from(v).unwrap_or(0);
    Config {
        accent: colour_hex(p.get_accent()),
        background: byte(p.get_background()),
        glow: byte(p.get_glow()),
        rounded: p.get_rounded(),
        compact: p.get_compact(),
        text_size: byte(p.get_text_size()),
        icons_only_sidebar: p.get_icons_only_sidebar(),
        show_hero: p.get_show_hero(),
        match_keyboard: p.get_match_keyboard(),
        fahrenheit: p.get_fahrenheit(),
        interval_s: byte(p.get_interval()),
        notify_mode: p.get_notify_mode(),
        notify_power: p.get_notify_power(),
        hot_cpu_c: byte(p.get_hot_cpu()),
        battery_60hz: p.get_battery_60hz(),
        profiles: profiles::read(ui),
        auto_profiles: ui.global::<crate::Profiles>().get_auto(),
    }
    .sanitized()
}

/// Load preferences into the UI and save them whenever they change.
pub fn install(ui: &AppWindow, shared: &Arc<Shared>) {
    let path = Rc::new(config::default_path());
    let loaded = config::load(&path);
    show(ui, &loaded);
    share(&loaded, shared);

    let save = {
        let shared = Arc::clone(shared);
        move |config: &Config| {
            share(config, &shared);
            if let Err(e) = config::save(&path, config) {
                log::warn!("Cannot save {}: {e}", path.display());
            }
        }
    };
    let prefs = ui.global::<Prefs>();
    let weak = ui.as_weak();
    let persist = save.clone();
    prefs.on_save(move || {
        if let Some(ui) = weak.upgrade() {
            persist(&read(&ui));
        }
    });
    let weak = ui.as_weak();
    prefs.on_reset(move || {
        if let Some(ui) = weak.upgrade() {
            let config = read(&ui).with_default_theme();
            show(&ui, &config);
            save(&config);
        }
    });

    prefs.on_test_notification(|| {
        notify::show("test", "System Profiler", "Notifications are working.");
    });

    // Autostart lives in its own file, not in the config.
    let entry = autostart::default_path();
    prefs.set_autostart(autostart::is_enabled(&entry));
    let weak = ui.as_weak();
    prefs.on_set_autostart(move |on| {
        let exe = std::env::current_exe().unwrap_or_else(|_| "system-profiler".into());
        if let Err(e) = autostart::set(&entry, on, &exe) {
            log::warn!("Cannot update {}: {e}", entry.display());
        }
        if let Some(ui) = weak.upgrade() {
            ui.global::<Prefs>()
                .set_autostart(autostart::is_enabled(&entry));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_round_trip() {
        assert_eq!(colour_hex(hex_colour("#22d3ee")), "#22d3ee");
        assert_eq!(colour_hex(hex_colour("nonsense")), config::DEFAULT_ACCENT);
    }
}
