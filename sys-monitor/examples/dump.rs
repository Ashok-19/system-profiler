//! Print live readings once a second.
//!
//! ```sh
//! cargo run -p sys_monitor --example dump -- 5   # five samples
//! ```

use std::thread::sleep;
use std::time::Duration;

use sys_monitor::{Monitor, Snapshot};

#[allow(clippy::cast_precision_loss)] // display only
fn gib(bytes: u64) -> f64 {
    bytes as f64 / f64::from(1u32 << 30)
}

fn print(s: &Snapshot) {
    let c = &s.cpu;
    println!(
        "CPU   {}  usage {:?}%  temp {:?}°C  avg {:?} MHz  max {:?} MHz",
        c.model.as_deref().unwrap_or("?"),
        c.usage_percent.map(f32::round),
        c.temp_c,
        c.avg_freq_mhz(),
        c.max_freq_mhz(),
    );
    for g in &s.gpus {
        println!(
            "GPU   {} {:?} [{:?}]  usage {:?}%  {:?} MHz  {:?}°C  {:?} W",
            g.card, g.driver, g.power_state, g.usage_percent, g.clock_mhz, g.temp_c, g.power_w,
        );
    }
    if let Some(m) = &s.memory {
        println!(
            "RAM   {:.1}/{:.1} GiB  swap {:.1}/{:.1} GiB",
            gib(m.used_bytes()),
            gib(m.total_bytes),
            gib(m.swap_used_bytes()),
            gib(m.swap_total_bytes),
        );
    }
    for f in &s.fans {
        println!("FAN   {}/{}  {} RPM", f.chip, f.label, f.rpm);
    }
    for t in &s.temps {
        println!("TEMP  {}/{}  {:.1}°C", t.chip, t.label, t.temp_c);
    }
    println!("AC    {:?}", s.power.ac_online);
    if let Some(b) = &s.power.battery {
        println!(
            "BAT   {} {:?} {:?}%  {:?} W  health {:?}%  limit {:?}%  remaining {:?} h",
            b.name,
            b.status,
            b.capacity_percent,
            b.power_w,
            b.health_percent().map(f32::round),
            b.charge_limit_percent,
            b.hours_remaining(),
        );
    }
    println!();
}

fn main() {
    let count: u32 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(3);
    let mut monitor = Monitor::new();
    for i in 0..count {
        if i > 0 {
            sleep(Duration::from_secs(1));
        }
        print(&monitor.sample());
    }
}
