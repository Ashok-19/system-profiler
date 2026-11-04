//! Background sampling of hardware sensors.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use slint::{ComponentHandle, Model, ModelRc, VecModel};
use sys_monitor::Monitor;

use crate::alerts::{Alerts, Reading, Wanted};
use crate::shared::Shared;
use crate::view::{ChartView, FactView, Formatter, LiveView, PanelView, SensorView};
use crate::{AppWindow, Chart, Fact, Live, Metric, MonitorPanel, Sensor, SensorRow};
use crate::{notify, screen};

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

/// Put `items` into the model from `get`, in place when it is already a
/// `VecModel`, otherwise through `set`.
fn sync_or_set<T: Clone + PartialEq + 'static>(
    current: &ModelRc<T>,
    items: Vec<T>,
    set: impl FnOnce(ModelRc<T>),
) {
    if current.as_any().is::<VecModel<T>>() {
        sync_rows(current, items);
    } else {
        set(ModelRc::new(VecModel::from(items)));
    }
}

fn to_chart(c: &ChartView) -> Chart {
    Chart {
        title: c.title.as_str().into(),
        now: c.now.as_str().into(),
        min: c.min.as_str().into(),
        max: c.max.as_str().into(),
        line: c.line.as_str().into(),
        area: c.area.as_str().into(),
    }
}

fn to_sensor(s: &SensorView) -> Sensor {
    Sensor {
        name: s.name.as_str().into(),
        source: s.source.as_str().into(),
        value: s.value.as_str().into(),
    }
}

/// Lay sensors out in two columns read top to bottom, left then right.
fn sensor_rows(sensors: &[SensorView]) -> Vec<SensorRow> {
    let half = sensors.len().div_ceil(2);
    let (left, right) = sensors.split_at(half);
    left.iter()
        .enumerate()
        .map(|(i, l)| SensorRow {
            left: to_sensor(l),
            right: right.get(i).map(to_sensor).unwrap_or_default(),
        })
        .collect()
}

fn apply(ui: &AppWindow, view: &LiveView) {
    let live = ui.global::<Live>();
    sync_panels(&live, &view.panels);
    live.set_battery_percent(view.battery_percent);
    live.set_on_ac(view.on_ac);
    live.set_battery_status(view.battery.status.as_str().into());
    live.set_dgpu_state(view.dgpu_state.as_str().into());
    sync_or_set(&live.get_battery_facts(), facts(&view.battery.facts), |m| {
        live.set_battery_facts(m);
    });
    sync_or_set(
        &live.get_charts(),
        view.charts.iter().map(to_chart).collect(),
        |m| live.set_charts(m),
    );
    sync_or_set(&live.get_sensors(), sensor_rows(&view.sensors), |m| {
        live.set_sensors(m);
    });
}

/// Sample sensors on a background thread and push them to the UI until the
/// window is gone. `monitor` should already have taken one sample so the
/// first update has usage percentages.
pub fn spawn(app: &AppWindow, mut monitor: Monitor, shared: &Arc<Shared>) {
    let chosen = Arc::clone(shared);
    app.global::<Live>().on_set_history_minutes(move |minutes| {
        chosen.set_window_minutes(u32::try_from(minutes).unwrap_or(1));
    });

    let ui = app.as_weak();
    let shared = Arc::clone(shared);
    let spawned = thread::Builder::new()
        .name("monitor".to_owned())
        .spawn(move || {
            let mut formatter = Formatter::default();
            let mut alerts = Alerts::default();
            let mut last_ac = None;
            loop {
                let interval = shared.interval_s().max(1);
                thread::sleep(Duration::from_secs(u64::from(interval)));
                formatter.window_minutes = shared.window_minutes();
                formatter.interval_s = interval;
                formatter.fahrenheit = shared.fahrenheit();

                let snapshot = monitor.sample();
                let reading = Reading {
                    on_ac: snapshot.power.ac_online,
                    battery_percent: snapshot
                        .power
                        .battery
                        .as_ref()
                        .and_then(|b| b.capacity_percent),
                    cpu_c: snapshot.cpu.temp_c,
                };
                let wanted = Wanted {
                    power: shared.notify_power(),
                    hot_cpu_c: shared.hot_cpu_c(),
                    fahrenheit: formatter.fahrenheit,
                };
                for alert in alerts.check(reading, wanted) {
                    notify::show(alert.tag, &alert.summary, &alert.body);
                }
                if let (Some(before), Some(now)) = (last_ac, reading.on_ac)
                    && before != now
                    && shared.battery_60hz()
                {
                    screen::follow_power(now);
                }
                if reading.on_ac.is_some() {
                    last_ac = reading.on_ac;
                }

                let view = formatter.live(&snapshot);
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
