//! Talks to asusd over the system bus on a worker thread.
//!
//! The UI never blocks on D-Bus: callbacks queue a [`Command`], and the
//! worker re-reads the daemon state after each command and every few
//! seconds, so changes made elsewhere (Fn+F5, asusctl, unplugging) show up.

mod lighting;
mod platform;

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use rog_dbus::zbus_aura::AuraProxyBlocking;
use rog_dbus::zbus_platform::PlatformProxyBlocking;
use slint::{ComponentHandle, Weak};
use zbus::blocking::Connection;
use zbus::blocking::fdo::ObjectManagerProxy;
use zbus::proxy::CacheProperties;

use crate::AppWindow;

/// How often the volatile state (mode, charge limit, brightness) is read.
const REFRESH: Duration = Duration::from_secs(2);
/// Every this many refreshes, and after each command, read everything.
const FULL_READ_EVERY: u32 = 5;
const RECONNECT: Duration = Duration::from_secs(5);
const SERVICE: &str = "xyz.ljones.Asusd";
const AURA_INTERFACE: &str = "xyz.ljones.Aura";

type State = (platform::State, lighting::State);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Platform(platform::Command),
    Lighting(lighting::Command),
}

/// A closure that queues `wrap(c)` for the worker; for UI callbacks.
/// Sending fails only once the worker has exited, when there is nothing
/// left to do.
fn sender<C: 'static>(tx: &Sender<Command>, wrap: fn(C) -> Command) -> impl Fn(C) + 'static {
    let tx = tx.clone();
    move |command| {
        let _ = tx.send(wrap(command));
    }
}

/// Proxies to asusd's interfaces. Properties are read uncached: asusd
/// changes some of them (one-shot charge, limit restore on unplug,
/// AC/battery switching) without announcing it.
struct Asusd {
    _connection: Connection,
    platform: PlatformProxyBlocking<'static>,
    /// `None` when there is no supported keyboard backlight.
    aura: Option<AuraProxyBlocking<'static>>,
}

impl Asusd {
    fn connect() -> zbus::Result<Self> {
        let connection = Connection::system()?;
        let platform = PlatformProxyBlocking::builder(&connection)
            .cache_properties(CacheProperties::No)
            .build()?;
        let aura = Self::find_aura(&connection)
            .inspect_err(|e| log::info!("No keyboard lighting from asusd: {e}"))
            .ok();
        Ok(Self {
            _connection: connection,
            platform,
            aura,
        })
    }

    /// asusd exports one aura object per device, e.g. `/xyz/ljones/aura/tuf`.
    fn find_aura(connection: &Connection) -> zbus::Result<AuraProxyBlocking<'static>> {
        let objects = ObjectManagerProxy::new(connection, SERVICE, "/")?.get_managed_objects()?;
        let mut paths: Vec<_> = objects
            .into_iter()
            .filter(|(_, interfaces)| interfaces.keys().any(|i| i.as_str() == AURA_INTERFACE))
            .map(|(path, _)| path)
            .collect();
        paths.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        let path = paths
            .into_iter()
            .next()
            .ok_or_else(|| zbus::Error::InterfaceNotFound)?;
        AuraProxyBlocking::builder(connection)
            .path(path)?
            .cache_properties(CacheProperties::No)
            .build()
    }

    fn refresh(&self, (device, lights): &mut State) {
        platform::refresh(&self.platform, device);
        if let Some(aura) = &self.aura {
            lighting::refresh(aura, lights);
        }
    }

    fn read(&self) -> State {
        (
            platform::read(&self.platform),
            self.aura
                .as_ref()
                .map_or_else(lighting::State::unavailable, lighting::read),
        )
    }

    fn execute(&self, command: Command) {
        match command {
            Command::Platform(c) => platform::execute(&self.platform, c),
            Command::Lighting(c) => {
                if let Some(aura) = &self.aura {
                    lighting::execute(aura, c);
                }
            }
        }
    }
}

fn publish(ui: &Weak<AppWindow>, (device, lights): State) -> bool {
    ui.upgrade_in_event_loop(move |ui| {
        platform::apply(&ui, device);
        lighting::apply(&ui, &lights);
    })
    .is_ok()
}

fn disconnected() -> State {
    (
        platform::State::disconnected(),
        lighting::State::unavailable(),
    )
}

fn run(ui: &Weak<AppWindow>, commands: &Receiver<Command>) {
    loop {
        let asusd = match Asusd::connect() {
            Ok(asusd) => asusd,
            Err(e) => {
                log::warn!("Cannot reach asusd on the system bus ({e}); retrying");
                if !publish(ui, disconnected()) {
                    return;
                }
                thread::sleep(RECONNECT);
                continue;
            }
        };

        let mut state = asusd.read();
        let mut published = None;
        let mut ticks = 0_u32;
        loop {
            let connected = state.0.is_connected();
            // Only wake the UI thread when something changed.
            if published.as_ref() != Some(&state) {
                if !publish(ui, state.clone()) {
                    return;
                }
                published = Some(state.clone());
            }
            if !connected {
                thread::sleep(RECONNECT);
                break;
            }
            let full = match commands.recv_timeout(REFRESH) {
                Ok(command) => {
                    asusd.execute(command);
                    // The UI updated optimistically; always send the real
                    // state back, even if it is unchanged (a rejection).
                    published = None;
                    true
                }
                Err(RecvTimeoutError::Timeout) => {
                    ticks = ticks.wrapping_add(1);
                    ticks.is_multiple_of(FULL_READ_EVERY)
                }
                Err(RecvTimeoutError::Disconnected) => return,
            };
            if full {
                state = asusd.read();
            } else {
                asusd.refresh(&mut state);
            }
        }
    }
}

/// Wire the UI callbacks and start the worker.
pub fn spawn(ui: &AppWindow) {
    let (tx, rx) = mpsc::channel();
    platform::wire(ui, &tx);
    lighting::wire(ui, &tx);
    drop(tx);

    let weak = ui.as_weak();
    let spawned = thread::Builder::new()
        .name("asusd".to_owned())
        .spawn(move || run(&weak, &rx));
    if let Err(e) = spawned {
        log::error!("Could not start the asusd thread: {e}");
    }
}
