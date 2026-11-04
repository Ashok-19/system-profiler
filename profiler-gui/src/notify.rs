//! Desktop notifications through `org.freedesktop.Notifications`.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};

use zbus::blocking::Connection;
use zbus::zvariant::Value;

const APP_NAME: &str = "System Profiler";
const TIMEOUT_MS: i32 = 5000;

/// Session bus connection, opened on first use and shared by all threads.
fn session() -> Option<&'static Connection> {
    static SESSION: OnceLock<Option<Connection>> = OnceLock::new();
    SESSION
        .get_or_init(|| {
            Connection::session()
                .inspect_err(|e| log::warn!("No session bus for notifications: {e}"))
                .ok()
        })
        .as_ref()
}

/// Id of the last notification shown per tag, so a newer one replaces it.
fn last_ids() -> &'static Mutex<HashMap<&'static str, u32>> {
    static IDS: OnceLock<Mutex<HashMap<&'static str, u32>>> = OnceLock::new();
    IDS.get_or_init(Mutex::default)
}

/// Show a notification. A newer notification with the same `tag`
/// replaces the older one instead of stacking up.
pub fn show(tag: &'static str, summary: &str, body: &str) {
    let Some(bus) = session() else { return };
    let mut ids = last_ids().lock().unwrap_or_else(PoisonError::into_inner);
    let replaces = ids.get(tag).copied().unwrap_or(0);
    let hints: HashMap<&str, Value<'_>> =
        HashMap::from([("desktop-entry", Value::from("system-profiler"))]);
    let reply = bus.call_method(
        Some("org.freedesktop.Notifications"),
        "/org/freedesktop/Notifications",
        Some("org.freedesktop.Notifications"),
        "Notify",
        &(
            APP_NAME,
            replaces,
            "system-profiler",
            summary,
            body,
            Vec::<&str>::new(),
            hints,
            TIMEOUT_MS,
        ),
    );
    match reply.and_then(|m| m.body().deserialize::<u32>()) {
        Ok(id) => {
            ids.insert(tag, id);
        }
        Err(e) => log::warn!("Notification failed: {e}"),
    }
}
