# System Profiler

An Armoury Crate–style control center for ASUS laptops on Linux, built for one
machine in particular: a **TUF Gaming A15 (FA506ICB)** running **Ubuntu 24.04
on X11**, kernel 6.8.

## Why

On Windows, ASUS ships Armoury Crate: one window for performance modes, battery
care, keyboard lighting and hardware monitoring. On Linux, [asusctl] already
does the hard part, talking to the firmware, but its GUI is aimed at ROG
laptops, Wayland and the newest kernels, and Ubuntu is not on its list.

This fork keeps asusctl's daemon as the engine and puts a different front end
on it: one that looks and feels like Armoury Crate, is light enough to leave
running in the tray (about 30 MiB of RAM, 0.3–2% of a CPU core), and is honest
about what the hardware cannot do. Also, because why not.

## What it does

| Page | |
|---|---|
| Home | Live CPU, GPU, fan and memory readings; mode and charge limit at a glance |
| Operating Mode | Silent / Performance / Turbo, switching on AC or battery, CPU energy preference per mode |
| GPU Mode | Discrete GPU state, apps using it, runtime power management |
| Battery Care | Health, 100/80/60% presets, a custom limit, one-time full charge |
| Display | Refresh rate, slowest rate on battery, touchpad on/off |
| Keyboard Lighting | Effects, colour, speed, brightness, when the backlight is lit |
| Scenario Profiles | Presets that switch on while chosen apps run and switch back after |
| System Monitor | 1, 5 or 15-minute charts and every sensor |
| System Information | Model, BIOS, CPU, GPUs, panel, memory, battery, OS |
| Personalization | Accent, background, glow, corners, density, text size |
| Settings | °C/°F, update interval, autostart, tray, notifications |

It lives in the tray when closed, notifies you about mode, charger and CPU
temperature changes, and a second launch brings up the running window.
Preferences live in `~/.config/system-profiler/config.ron`.

Not offered, on purpose: GPU **Eco/Optimized** (in a PRIME on-demand session
Xorg keeps the NVIDIA driver loaded, so the dGPU cannot be switched off safely;
it sleeps when idle instead), **Ultimate** (no MUX switch on this model) and
**fan curves** (the firmware has no interface for them).

## Install

On Ubuntu 24.04 (details and the feature status on kernel 6.8 are in
[docs/distributions/ubuntu.md](docs/distributions/ubuntu.md)):

```sh
sudo apt install build-essential cmake pkg-config libclang-dev libudev-dev \
    libfontconfig1-dev libfreetype-dev libexpat1-dev libxkbcommon-dev \
    libxkbcommon-x11-dev libwayland-dev libssl-dev libzstd-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
make && sudo make install
sudo systemctl mask power-profiles-daemon   # let asusd own the power profiles
sudo systemctl enable --now asusd
```

Then open **System Profiler** from the menu, or run `system-profiler`.
`cargo run --release -p profiler-gui` runs it from a checkout.

## Built on

- **[asusctl]** by Luke Jones, Denis Benato and the Open Gaming Collective
  (MPL-2.0). Its daemon, `asusd`, does every privileged change: platform
  profiles, charge limit, CPU energy preference and keyboard lighting. This
  repository is a fork; upstream is tracked as the `upstream` remote, and
  `asusctl` and `rog-control-center` still build and install alongside.
- **Linux kernel drivers**, read through sysfs, hwmon and procfs:
  `asus-nb-wmi`/`asus-wmi`, `amd-pstate-epp`, `k10temp`, `amdgpu`, `nvme`,
  and the NVIDIA driver's runtime power management.
- **[Slint]** for the UI (software renderer, winit X11 backend), **[zbus]** for
  D-Bus, **[ksni]** for the tray icon, **[serde]** + **[RON]** for settings,
  and **[dirs]**, **log** and **env_logger**.
- **xrandr** and **xinput** for refresh rate and touchpad, plus the
  freedesktop Notifications, StatusNotifierItem and XDG autostart specs.
- **[Open Sans]** and **[Michroma]** fonts (SIL OFL 1.1).

The design follows Armoury Crate's look; no ASUS logos or artwork are used, and
this project is not affiliated with ASUS.

## Layout

- `profiler-gui/` is the app (binary `system-profiler`).
- `sys-monitor/` is a small dependency-free sensor reader.
- Everything else is upstream asusctl: the `asusd` daemon, the `asusctl`
  CLI and `rog-control-center`. Their documentation, including other
  distributions and service management, is in [docs/asusctl.md](docs/asusctl.md).

[asusctl]: https://github.com/OpenGamingCollective/asusctl
[Slint]: https://slint.dev
[zbus]: https://github.com/dbus2/zbus
[ksni]: https://github.com/iovxw/ksni
[serde]: https://serde.rs
[RON]: https://github.com/ron-rs/ron
[dirs]: https://github.com/dirs-dev/dirs-rs
[Open Sans]: https://fonts.google.com/specimen/Open+Sans
[Michroma]: https://fonts.google.com/specimen/Michroma

## License and trademarks

System Profiler is a fork of asusctl and keeps its licence, the
[Mozilla Public License 2.0](LICENSE). The bundled Open Sans and Michroma
fonts are under the SIL Open Font License 1.1 (see `profiler-gui/fonts/`).

ASUS, ROG, TUF and Armoury Crate are trademarks of ASUSTeK Computer Inc. This
project is not affiliated with or endorsed by ASUS; product names are used only
to identify hardware.
