# Ubuntu

> Only Ubuntu 26.04 LTS is supported; asusctl is distributed using [Homebrew](https://brew.sh/)

## Ubuntu 24.04 LTS (system-profiler fork)

This fork is built and tested on Ubuntu 24.04 with the stock 6.8 kernel
(ASUS TUF Gaming A15 FA506IC, XFCE on X11). Build and install from source as
described in [asusctl.md](../asusctl.md#ubuntu-2404-supported-by-this-fork),
then:

```bash
sudo systemctl daemon-reload
# Mask (not just disable) so D-Bus activation cannot bring it back.
sudo systemctl mask --now power-profiles-daemon.service
sudo systemctl start asusd.service
```

`asusd` and `asus-shutdown` are started automatically on later boots by the
udev rule. To undo: `sudo systemctl unmask power-profiles-daemon.service` and
`sudo make uninstall`.

### What works on the stock 6.8 kernel

The 6.8 kernel has no `asus-armoury` driver (mainline since 6.19), so firmware
attributes are only available through the older `asus-nb-wmi` driver:

| Feature | Status on 6.8 |
| :--- | :--- |
| Platform profiles (Quiet/Balanced/Performance) and linked CPU EPP | Works |
| Separate AC / battery profiles | Works |
| Battery charge limit | Works |
| Keyboard brightness and TUF RGB modes | Works |
| `asusctl armoury` and the GUI's GPU / system pages | Unavailable (need `asus-armoury`) |
| dGPU disable (Eco mode) | Planned in this fork via `asus-nb-wmi` |
| Custom fan curves | Model dependent; not available on FA506IC |

The rest of this page is upstream's guide for Ubuntu 26.04.

## Installation

1. Install Homebrew for Linux if you don't have it already:

```bash
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
```

2. Add the ublue-os tap:

```bash
brew tap ublue-os/homebrew-tap
```

3. Install asusctl and the ROG Control Center:

```bash
brew install asusctl-linux
brew install rog-control-center-linux
```

The casks install the `asusd` systemd units, udev rules, and D-Bus configuration automatically. Verify the daemon is running:

```bash
sudo systemctl enable --now asusd.service asus-shutdown.service
systemctl --user daemon-reload
systemctl --user enable --now asusd-user.service
sudo udevadm control --reload
sudo udevadm trigger
systemctl status asusd.service
```

For everything else (platform profiles, fan curves, GPU switching), refer to the [Usage](../usage/asusctl.md) section.

## Power profiles

> [!IMPORTANT]
> **Power profiles**: `asusd` manages platform profiles and CPU EPP settings itself via the ACPI `platform_profile` interface. Running an external power profiles daemon (such as `power-profiles-daemon` or `tuned`) alongside `asusd` can cause race conditions over `/sys/firmware/acpi/platform_profile` and CPU EPP preferences. Ubuntu ships `power-profiles-daemon` by default, so you have two options:
>
> 1. **Let `asusd` manage profiles** and disable the external daemon:
>
> ```bash
> sudo systemctl disable --now power-profiles-daemon.service
> ```
>
> 2. **Keep the external daemon** and disable `asusd`'s profile management by setting the following to `false` in `/etc/asusd/asusd.ron`:
>
> ```ron
> change_platform_profile_on_ac: false,
> change_platform_profile_on_battery: false,
> platform_profile_linked_epp: false,
> ```
>
> A common way to switch profiles is binding the `Fn+F5` key to `asusctl profile next`
> Available profiles vary by system, see `asusctl profile list`.

## Graphics Switching

See [GPU Switching](../faq/gpu-switching.md) for how to manage the dGPU and MUX.
