//! Preferences the worker threads read on every cycle. The UI thread
//! writes them whenever the user changes a setting.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};

#[derive(Debug)]
pub struct Shared {
    /// System Monitor chart window, minutes.
    window_minutes: AtomicU32,
    /// Seconds between sensor samples.
    interval_s: AtomicU32,
    fahrenheit: AtomicBool,
    notify_mode: AtomicBool,
    notify_power: AtomicBool,
    /// Warn above this CPU temperature (°C); 0 = never.
    hot_cpu_c: AtomicU32,
    battery_60hz: AtomicBool,
    auto_profiles: AtomicBool,
    /// Trigger apps per profile; empty for disabled or manual ones.
    triggers: Mutex<Vec<Vec<String>>>,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            window_minutes: AtomicU32::new(1),
            interval_s: AtomicU32::new(1),
            fahrenheit: AtomicBool::new(false),
            notify_mode: AtomicBool::new(false),
            notify_power: AtomicBool::new(false),
            hot_cpu_c: AtomicU32::new(0),
            battery_60hz: AtomicBool::new(false),
            auto_profiles: AtomicBool::new(false),
            triggers: Mutex::default(),
        }
    }
}

// Each value is independent, so relaxed ordering is enough.
macro_rules! accessors {
    ($($get:ident, $set:ident: $ty:ty);* $(;)?) => {
        impl Shared {
            $(
                pub fn $get(&self) -> $ty {
                    self.$get.load(Ordering::Relaxed)
                }

                pub fn $set(&self, value: $ty) {
                    self.$get.store(value, Ordering::Relaxed);
                }
            )*
        }
    };
}

accessors! {
    window_minutes, set_window_minutes: u32;
    interval_s, set_interval_s: u32;
    fahrenheit, set_fahrenheit: bool;
    notify_mode, set_notify_mode: bool;
    notify_power, set_notify_power: bool;
    hot_cpu_c, set_hot_cpu_c: u32;
    battery_60hz, set_battery_60hz: bool;
    auto_profiles, set_auto_profiles: bool;
}

impl Shared {
    pub fn triggers(&self) -> Vec<Vec<String>> {
        self.triggers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn set_triggers(&self, triggers: Vec<Vec<String>>) {
        *self.triggers.lock().unwrap_or_else(PoisonError::into_inner) = triggers;
    }
}
