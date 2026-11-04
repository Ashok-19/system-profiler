//! System tray icon (StatusNotifierItem): open the window, switch the
//! operating mode or charge limit, quit. Runs on ksni's own thread; menu
//! actions are handed to the UI thread.

use std::sync::OnceLock;

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{MenuItem, RadioGroup, RadioItem, StandardItem};
use slint::{ComponentHandle, Weak};

use crate::{AppWindow, Device, tray_icon};

const MODES: [&str; 3] = [
    "Silent", "Performance", "Turbo",
];
const LIMITS: [i32; 3] = [
    100, 80, 60,
];
const ICON_SIZES: [u32; 3] = [
    22, 32, 48,
];

static HANDLE: OnceLock<Handle<SystemTray>> = OnceLock::new();

pub struct SystemTray {
    ui: Weak<AppWindow>,
    /// UI mode index, -1 unknown.
    mode: i32,
    /// Charge limit in percent, -1 unknown.
    limit: i32,
    icons: Vec<ksni::Icon>,
}

fn icons(accent: (u8, u8, u8)) -> Vec<ksni::Icon> {
    ICON_SIZES
        .iter()
        .map(|&size| ksni::Icon {
            width: i32::try_from(size).unwrap_or(32),
            height: i32::try_from(size).unwrap_or(32),
            data: tray_icon::render(size, accent),
        })
        .collect()
}

/// Show and raise the main window.
pub fn show_window(ui: &Weak<AppWindow>) {
    let _ = ui.upgrade_in_event_loop(|ui| {
        if let Err(e) = ui.show() {
            log::warn!("Cannot show the window: {e}");
        }
        ui.window().set_minimized(false);
    });
}

impl ksni::Tray for SystemTray {
    fn id(&self) -> String {
        "system-profiler".into()
    }

    fn title(&self) -> String {
        "System Profiler".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icons.clone()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let mode = usize::try_from(self.mode)
            .ok()
            .and_then(|m| MODES.get(m))
            .map_or_else(String::new, |m| format!("{m} mode"));
        let limit = if self.limit > 0 {
            format!("Charge limit {}%", self.limit)
        } else {
            String::new()
        };
        ksni::ToolTip {
            title: "System Profiler".into(),
            description: [
                mode, limit,
            ]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · "),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        show_window(&self.ui);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let radio = |label: &str| RadioItem {
            label: label.into(),
            ..Default::default()
        };
        let mut items = vec![
            StandardItem {
                label: "Open System Profiler".into(),
                icon_name: "system-profiler".into(),
                activate: Box::new(|t: &mut Self| show_window(&t.ui)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
        ];
        if self.mode >= 0 {
            items.push(
                RadioGroup {
                    selected: usize::try_from(self.mode).unwrap_or(0),
                    select: Box::new(|t: &mut Self, index: usize| {
                        let Ok(mode) = i32::try_from(index) else {
                            return;
                        };
                        t.mode = mode;
                        let _ = t.ui.upgrade_in_event_loop(move |ui| {
                            let device = ui.global::<Device>();
                            device.set_mode(mode);
                            device.invoke_set_mode(mode);
                        });
                    }),
                    options: MODES.iter().map(|m| radio(m)).collect(),
                }
                .into(),
            );
            items.push(MenuItem::Separator);
        }
        if self.limit > 0 {
            items.push(
                RadioGroup {
                    selected: LIMITS
                        .iter()
                        .position(|&l| l == self.limit)
                        .unwrap_or(usize::MAX),
                    select: Box::new(|t: &mut Self, index: usize| {
                        let Some(&limit) = LIMITS.get(index) else {
                            return;
                        };
                        t.limit = limit;
                        let _ = t.ui.upgrade_in_event_loop(move |ui| {
                            let device = ui.global::<Device>();
                            device.set_charge_limit(limit);
                            device.invoke_set_charge_limit(limit);
                        });
                    }),
                    options: vec![
                        radio("Charge to 100%"),
                        radio("Charge to 80%"),
                        radio("Charge to 60%"),
                    ],
                }
                .into(),
            );
            items.push(MenuItem::Separator);
        }
        items.push(
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_: &mut Self| {
                    let _ = slint::quit_event_loop();
                }),
                ..Default::default()
            }
            .into(),
        );
        items
    }
}

/// Register the tray icon. Returns false when the desktop has no tray
/// (no `StatusNotifierWatcher`), in which case closing the window quits.
pub fn spawn(ui: &AppWindow, accent: (u8, u8, u8)) -> bool {
    let tray = SystemTray {
        ui: ui.as_weak(),
        mode: -1,
        limit: -1,
        icons: icons(accent),
    };
    match tray.disable_dbus_name(true).spawn() {
        Ok(handle) => {
            let _ = HANDLE.set(handle);
            true
        }
        Err(e) => {
            log::warn!("No system tray available ({e}); closing the window will quit");
            false
        }
    }
}

/// Keep the tray's radio items in step with asusd.
pub fn set_state(mode: i32, limit: i32) {
    if let Some(handle) = HANDLE.get() {
        handle.update(|t| {
            t.mode = mode;
            t.limit = limit;
        });
    }
}

/// Recolour the icon after an accent change.
pub fn set_accent(accent: (u8, u8, u8)) {
    if let Some(handle) = HANDLE.get() {
        let fresh = icons(accent);
        handle.update(move |t| t.icons = fresh);
    }
}
