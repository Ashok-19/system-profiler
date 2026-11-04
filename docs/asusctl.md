> [!NOTE]
> This is upstream [asusctl](https://github.com/OpenGamingCollective/asusctl)'s
> README, kept for reference: it covers `asusd`, `asusctl` and
> `rog-control-center`, which this fork still builds and installs. For System
> Profiler itself, see the [main README](../README.md). Sections marked
> **Fork** are this repository's additions.

# asusctl for ASUS ROG

<p align="center">
  <a href="https://ko-fi.com/neroreflex"><img src="../extra/icons/ko-fi-button.svg" width="190" height="32" alt="Support me on Ko-fi" /></a>
  <a href="https://asus-linux.org/"><img src="../extra/icons/rog-logo-button.svg" width="190" height="32" alt="Asus Linux Website" /></a>
  <a href="https://discord.gg/B8GftRW2Hd"><img src="../extra/icons/discord-button.svg" width="190" height="32" alt="Discord" /></a>
</p>

> [!WARNING]
> **Kernel Patch Requirement:** Many features are developed alongside Linux kernel updates. If an expected feature is missing, ensure your system is running the latest stable kernel or a kernel containing the required patches.

`asusctl` is a system control utility for Linux designed primarily for ASUS laptops, with reduced functionality available for non-ASUS hardware.

The project consists of three core components:
- `asusd`: System daemon controlled through D-Bus interfaces.
- `rog-control-center`: Graphical user interface for `asusd`.
- `asusctl`: Command-line client for `asusd`.

## Overview and goals

The primary goal of `asusctl` is to provide a safe, efficient abstraction layer over hardware features using D-Bus. It manages automated system responses, such as switching performance profiles when connecting or disconnecting AC power.

- **Clean interface:** Exposes hardware controls safely via D-Bus.
- **Resource efficiency:** Operates with minimal CPU overhead and under 1 MB of RAM during standard daemon execution.

## Hardware and kernel compatibility

### Supported laptops

`asusctl` supports most ASUS gaming laptops equipped with a USB keyboard. To verify device compatibility, run `lsusb` in your terminal and check for entries matching:

```plain
Bus 001 Device 002: ID 0b05:1866 ASUSTek Computer, Inc. N-KEY Device
```

or

```plain
Bus 003 Device 002: ID 0b05:19b6 ASUSTek Computer, Inc. [unknown]
```

Devices displaying these hardware IDs typically function without extra configuration. Features such as AniMe Matrix, LED controls, and Slash displays work regardless of your laptop make. However, if you are using a newer laptop model, adding explicit hardware support may be required. Support for new models is added upstream; see the [asusctl issue tracker](https://github.com/OpenGamingCollective/asusctl/issues).

Features such as battery charge thresholds use generic kernel interfaces and work on non-ASUS hardware, but platform and fan controls require ASUS-specific `asus-nb-wmi` or `asus-armoury` drivers.

### Kernel requirements

Due to ongoing development, the minimum suggested kernel version is always **the latest**, as improvements are merged upstream continuously.

Support for Thermal Design Power (TDP) is tied to the new `asus-armoury` driver: available mainline since Linux 6.19: everything older is not supported.

### Display server support (X11)

> [!NOTE]
> **Fork:** the GUI is built with X11 support by default (the `x11` feature of
> `rog-control-center`), alongside Wayland. This fork is developed and tested on
> XFCE/X11.
>
> Upstream considers X11 unsupported. Technical assistance is not provided upstream for X11 environments due to developer resource constraints and the unmaintained status of X11 itself.

## Implemented features

Feature availability depends on upstream Linux kernel support and specific hardware capabilities.

### Power and performance

- [x] **Battery charge thresholds:** Configure maximum charging limits (requires kernel support)
- [x] **Custom fan curves:** Adjust fan profiles on supported hardware
- [x] **GPU MUX toggling:** Switch GPU operational modes (G-Sync / MUX) on 2022 and newer laptops
- [x] **Power profile management:** Control system performance profiles as detailed in [MANUAL.md](MANUAL.md)

### Lighting and displays

- [x] **Built-in LED controls:** Adjust integrated keyboard lighting modes
- [x] **Per-key RGB configuration:** Customize individual key backlight settings
- [x] **Advanced lighting effects:** Apply custom animation modes (currently undergoing revision)
- [x] **AniMe Matrix displays:** Control panel rendering on equipped G14, M16, and Strix Scar 16/18 models

### System integration

- [x] **System daemon (`asusd`):** Background service handling hardware communications
- [x] **Graphical interface (`rog-control-center`):** Desktop application with system tray integration and notifications
- [x] **POST audio controls:** Toggle the BIOS boot sound setting

### Additional hardware configuration notes

Keyboard backlight support relies on hardware mappings defined in [`./rog-aura/data/aura_support.ron`](../rog-aura/data/aura_support.ron), installed to `/usr/share/asusd/aura_support.ron`. Because keyboard controller configurations vary across model generations and firmware revisions, explicit layout definitions prevent misconfigurations. Refer to the [rog-aura README](../rog-aura/README.md) for configuration details.

## Installation and setup

### Package installation

Pre-built binary packages are available in several Linux distribution repositories. Check your package manager before building from source.

| Distribution | Repository Source | Installation Command | Notes |
| :--- | :--- | :--- | :--- |
| **Ultramarine / Nobara** | Official Repositories | `sudo dnf install asusctl` | Direct package installation |
| **Fedora** | [Terra Repository](https://terrapkg.com/) | `sudo dnf install asusctl` | Requires Terra repository enabled |
| **openSUSE** | [OBS Repository](https://download.opensuse.org/repositories/home:/luke_nukem:/asus/) | Add OBS repository | Maintained on OpenSUSE Build Service |
| **Arch Linux** | [OGC Arch Repository](https://github.com/OpenGamingCollective/ogc-arch-packaging) | Refer to OGC Arch Guide | Maintained via OGC Arch pacman repository |
| **Nix / NixOS** | Nixpkgs | `nix-env -iA nixpkgs.asusctl` | Package name: `asusctl` |
| **Solus** | Official Repositories | `sudo eopkg install asusctl` | Direct package installation |

#### Service management

`asusctl` uses `udev` rules to initialize background services when hardware is detected.

On systems such as Fedora or Ultramarine, enable and start the services manually after installation:

```sh
sudo systemctl enable --now asusd.service
sudo systemctl enable --now asus-shutdown.service
```

On Debian, service activation may require manual intervention. On Pop!_OS systems, disable the `system76-power` GNOME extension and its associated `systemd` service to prevent power profile conflicts.

### Building from source

Compiling `asusctl` requires the Rust compiler and Cargo toolchain from [rustup.rs](https://rustup.rs/). Use the stable toolchain for build tasks.

#### Arch Linux

```sh
sudo pacman -S --needed --asdeps git cmake clang pkg-config libzip rust openssl
make
sudo make install
```

#### Fedora

```sh
sudo dnf install git make cmake clang-devel libxkbcommon-devel systemd-devel expat-devel pcre2-devel libzstd-devel gtk3-devel rust cargo
make
sudo make install
```

#### openSUSE

For KDE Plasma desktop environments without GTK dependencies:

```sh
sudo zypper in -t pattern devel_basis
sudo zypper in rustup make cmake clang-devel libxkbcommon-devel systemd-devel expat-devel pcre2-devel libzstd-devel
make
sudo make install
```

#### Debian (Unsupported)

Debian is officially unsupported, but you can attempt to build with:

```sh
sudo apt install libclang-dev libudev-dev libfontconfig-dev build-essential cmake libxkbcommon-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
make
sudo make install
```

#### Ubuntu 24.04 (supported by this fork)

Tested on Ubuntu 24.04 LTS (kernel 6.8, XFCE on X11). The toolchain pinned in
`rust-toolchain.toml` is installed automatically by [rustup](https://rustup.rs/);
Ubuntu's `cargo`/`rustc` packages are too old.

```sh
sudo apt install build-essential cmake pkg-config libclang-dev libudev-dev \
    libfontconfig1-dev libfreetype-dev libexpat1-dev libxkbcommon-dev \
    libxkbcommon-x11-dev libwayland-dev libssl-dev libzstd-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
make
sudo make install
```

`libsdl2-dev` is only needed to build the AniMe Matrix simulator
(`cargo build -p rog_simulators`), which is not part of `make`.

Pop!_OS is based on Ubuntu and should work the same way, but is not tested.

### Upgrading

When upgrading an existing installation, reload systemd service definitions and restart `asusd`:

```sh
sudo systemctl daemon-reload && sudo systemctl restart asusd
```

Alternatively, reboot the system to apply updates.

### Uninstalling

To remove installations built from source, stop and disable the services, run `sudo make uninstall` from the source directory, and reload systemd:

```sh
sudo systemctl disable --now asusd.service asus-shutdown.service
sudo make uninstall
sudo systemctl daemon-reload
```

Remove any remaining configuration files in `/etc/asusd/`.

For binary installations, remove `asusctl` using your distribution package manager.

## Development and testing

### Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md) for contribution guidelines and git hooks setup.

### AniMe Matrix simulator

An SDL2-based simulator is included for testing matrix display rendering without physical hardware.

To compile and launch the simulator:

```sh
cargo build --package rog_simulators
./target/debug/anime_sim
```

Restart `asusd` after starting the simulator to attach the service to the simulated display interface. Running the simulator on a laptop with a physical display redirects display output to the simulator window.
