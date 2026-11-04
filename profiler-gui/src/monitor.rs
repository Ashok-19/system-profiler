//! Background sampling of hardware sensors.

use std::thread;
use std::time::Duration;

use slint::{ComponentHandle, Model, ModelRc, VecModel, Weak};
use sys_monitor::Monitor;

use crate::view::{FactView, Formatter, LiveView, PanelView};
use crate::{AppWindow, Fact, Live, Metric, MonitorPanel};

const INTERVAL: Duration = Duration::from_secs(1);

fn metrics(panel: &PanelView) -> Vec<Metric> {
    panel
        .bars
        .iter()
        .map(|m| Metric {
            label: m.label.as_str().into(),
            value: m.value.as_str().into(),
            percent: m.percent,
        })
        .collect()
}

fn facts(rows: &[FactView]) -> Vec<Fact> {
    rows.iter()
        .map(|f| Fact {
            key: f.key.as_str().into(),
            value: f.value.as_str().into(),
        })
        .collect()
}

fn to_slint(panel: &PanelView) -> MonitorPanel {
    MonitorPanel {
        title: panel.title.as_str().into(),
        bars: ModelRc::new(VecModel::from(metrics(panel))),
        rows: ModelRc::new(VecModel::from(facts(&panel.rows))),
    }
}

/// Make `model` hold `items`, changing only the rows that differ so the UI
/// redraws as little as possible. Returns false if `model` is not a
/// `VecModel` (the caller must replace it).
fn sync_rows<T: Clone + PartialEq + 'static>(model: &ModelRc<T>, items: Vec<T>) -> bool {
    let Some(vec) = model.as_any().downcast_ref::<VecModel<T>>() else {
        return false;
    };
    if vec.row_count() == items.len() {
        for (i, item) in items.into_iter().enumerate() {
            if vec.row_data(i).as_ref() != Some(&item) {
                vec.set_row_data(i, item);
            }
        }
    } else {
        vec.set_vec(items);
    }
    true
}

/// Update the panels in place; rebuild only when their number changes.
fn sync_panels(live: &Live<'_>, panels: &[PanelView]) {
    let current = live.get_panels();
    let Some(model) = current
        .as_any()
        .downcast_ref::<VecModel<MonitorPanel>>()
        .filter(|m| m.row_count() == panels.len())
    else {
        let fresh: Vec<MonitorPanel> = panels.iter().map(to_slint).collect();
        live.set_panels(ModelRc::new(VecModel::from(fresh)));
        return;
    };
    for (i, panel) in panels.iter().enumerate() {
        let Some(row) = model.row_data(i) else {
            continue;
        };
        let in_place = row.title.as_str() == panel.title
            && sync_rows(&row.bars, metrics(panel))
            && sync_rows(&row.rows, facts(&panel.rows));
        if !in_place {
            model.set_row_data(i, to_slint(panel));
        }
    }
}

fn apply(ui: &AppWindow, view: &LiveView) {
    let live = ui.global::<Live>();
    sync_panels(&live, &view.panels);
    live.set_battery_percent(view.battery_percent);
    live.set_on_ac(view.on_ac);
    live.set_battery_status(view.battery.status.as_str().into());
    let battery_facts = facts(&view.battery.facts);
    let current = live.get_battery_facts();
    if current.as_any().is::<VecModel<Fact>>() {
        sync_rows(&current, battery_facts);
    } else {
        live.set_battery_facts(ModelRc::new(VecModel::from(battery_facts)));
    }
}

/// Sample sensors on a background thread and push them to the UI until the
/// window is gone. `monitor` should already have taken one sample so the
/// first update has usage percentages.
pub fn spawn(ui: Weak<AppWindow>, mut monitor: Monitor) {
    let spawned = thread::Builder::new()
        .name("monitor".to_owned())
        .spawn(move || {
            let mut formatter = Formatter::default();
            loop {
                thread::sleep(INTERVAL);
                let view = formatter.live(&monitor.sample());
                if ui
                    .upgrade_in_event_loop(move |ui| apply(&ui, &view))
                    .is_err()
                {
                    break;
                }
            }
        });
    if let Err(e) = spawned {
        log::error!("Could not start the monitor thread: {e}");
    }
}
