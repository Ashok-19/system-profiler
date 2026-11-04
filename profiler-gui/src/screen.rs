//! Connects the Screen global (Display page) to `display`. Every command
//! runs on a short-lived thread so the UI never waits on xrandr/xinput.

use std::thread;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel, Weak};

use crate::display::{self, Panel, Touchpad};
use crate::{AppWindow, Screen};

fn rate_label(hz: f32) -> SharedString {
    format!("{hz:.0} Hz").into()
}

fn apply(ui: &AppWindow, panel: Option<&Panel>, touchpad: Option<&Touchpad>) {
    let s = ui.global::<Screen>();
    s.set_available(panel.is_some());
    if let Some(p) = panel {
        s.set_output(p.output.as_str().into());
        s.set_resolution(format!("{} × {}", p.width, p.height).into());
        s.set_size(
            p.inches
                .map(|i| format!("{i:.1}″"))
                .unwrap_or_default()
                .into(),
        );
        let labels: Vec<SharedString> = p.rates.iter().copied().map(rate_label).collect();
        s.set_rates(ModelRc::new(VecModel::from(labels)));
        s.set_current(p.current.and_then(|c| i32::try_from(c).ok()).unwrap_or(-1));
    }
    s.set_has_touchpad(touchpad.is_some());
    if let Some(t) = touchpad {
        s.set_touchpad_on(t.enabled);
        s.set_touchpad_name(t.name.as_str().into());
    }
    s.set_busy(false);
}

/// Run `work` off the UI thread, then show the fresh state.
fn in_background(ui: &Weak<AppWindow>, work: impl FnOnce() + Send + 'static) {
    let ui = ui.clone();
    let spawned = thread::Builder::new()
        .name("display".to_owned())
        .spawn(move || {
            work();
            let panel = display::query_panel();
            let touchpad = display::query_touchpad();
            let _ =
                ui.upgrade_in_event_loop(move |ui| apply(&ui, panel.as_ref(), touchpad.as_ref()));
        });
    if let Err(e) = spawned {
        log::error!("Could not start the display thread: {e}");
    }
}

pub fn wire(ui: &AppWindow) {
    let s = ui.global::<Screen>();
    let weak = ui.as_weak();
    s.on_refresh(move || in_background(&weak, || {}));

    let weak = ui.as_weak();
    s.on_set_rate(move |index| {
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        in_background(&weak, move || {
            if let Some(panel) = display::query_panel()
                && let Some(&hz) = panel.rates.get(index)
            {
                display::set_rate(&panel, hz);
            }
        });
    });

    let weak = ui.as_weak();
    s.on_set_touchpad(move |on| {
        in_background(&weak, move || {
            if let Some(t) = display::query_touchpad() {
                display::set_touchpad(t.id, on);
            }
        });
    });
}

/// Switch the panel to its slowest rate on battery and back to its
/// fastest on AC. Called from a worker thread.
pub fn follow_power(on_ac: bool) {
    let Some(panel) = display::query_panel() else {
        return;
    };
    let current = panel.current.and_then(|i| panel.rates.get(i).copied());
    if let Some(hz) = display::rate_for_power(&panel, on_ac)
        && current != Some(hz)
    {
        log::info!("Power source changed; setting the panel to {hz:.0} Hz");
        display::set_rate(&panel, hz);
    }
}
