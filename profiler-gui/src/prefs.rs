//! Binds the Prefs global to the config file.

use std::rc::Rc;

use slint::{Color, ComponentHandle};

use crate::config::{self, Config};
use crate::{AppWindow, Prefs};

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
    }
    .sanitized()
}

/// Load preferences into the UI and save them whenever they change.
pub fn install(ui: &AppWindow) {
    let path = Rc::new(config::default_path());
    show(ui, &config::load(&path));

    let save = move |config: &Config| {
        if let Err(e) = config::save(&path, config) {
            log::warn!("Cannot save {}: {e}", path.display());
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
            let defaults = Config::default();
            show(&ui, &defaults);
            save(&defaults);
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
