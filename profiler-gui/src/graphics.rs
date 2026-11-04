//! GPU Mode page facts: which apps hold the NVIDIA device files, whether
//! the dGPU may sleep, and what the firmware offers (`dgpu_disable`, MUX).
//!
//! Nothing here touches the GPU itself, so reading it never wakes a
//! sleeping dGPU. It runs only when the page opens or asks to refresh.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const ASUS_WMI: &str = "sys/devices/platform/asus-nb-wmi";
const POWERD_UNITS: &[&str] = &[
    "usr/lib/systemd/system/nvidia-powerd.service",
    "lib/systemd/system/nvidia-powerd.service",
    "etc/systemd/system/nvidia-powerd.service",
];

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Facts {
    /// Names of this user's processes with an NVIDIA device open.
    pub users: Vec<String>,
    /// `power/control` of the dGPU is `auto` (runtime PM allowed).
    pub can_sleep: Option<bool>,
    /// The firmware's `dgpu_disable`: true means the dGPU is off (Eco).
    pub dgpu_disabled: Option<bool>,
    /// The firmware has a MUX switch (`gpu_mux_mode`).
    pub has_mux: bool,
    /// nvidia-powerd (Dynamic Boost) is installed.
    pub has_powerd: bool,
}

/// Process names (from `comm`) of processes in `root/proc` that have a
/// `/dev/nvidia*` file open. Processes we may not inspect are skipped.
pub fn gpu_users(root: &Path) -> Vec<String> {
    let Ok(procs) = fs::read_dir(root.join("proc")) else {
        return Vec::new();
    };
    let mut names = BTreeSet::new();
    for proc in procs.flatten() {
        let is_pid = proc
            .file_name()
            .to_str()
            .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()));
        if !is_pid {
            continue;
        }
        let Ok(fds) = fs::read_dir(proc.path().join("fd")) else {
            continue;
        };
        let uses_gpu = fds.flatten().any(|fd| {
            fs::read_link(fd.path())
                .is_ok_and(|target| target.to_string_lossy().starts_with("/dev/nvidia"))
        });
        if uses_gpu && let Ok(comm) = fs::read_to_string(proc.path().join("comm")) {
            names.insert(comm.trim().to_owned());
        }
    }
    names.into_iter().collect()
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_owned())
}

/// `pci_address` is the dGPU's address, e.g. `0000:01:00.0`.
pub fn facts(root: &Path, pci_address: Option<&str>) -> Facts {
    let wmi = root.join(ASUS_WMI);
    Facts {
        users: gpu_users(root),
        can_sleep: pci_address.and_then(|addr| {
            read_trimmed(&root.join(format!("sys/bus/pci/devices/{addr}/power/control")))
                .map(|c| c == "auto")
        }),
        dgpu_disabled: read_trimmed(&wmi.join("dgpu_disable")).map(|v| v == "1"),
        has_mux: wmi.join("gpu_mux_mode").exists(),
        has_powerd: POWERD_UNITS.iter().any(|u| root.join(u).exists()),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    use super::*;

    struct Root(PathBuf);

    impl Root {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("profiler-gfx-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn write(&self, rel: &str, text: &str) {
            let path = self.0.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }

        fn process(&self, pid: u32, comm: &str, fds: &[&str]) {
            self.write(&format!("proc/{pid}/comm"), &format!("{comm}\n"));
            let dir = self.0.join(format!("proc/{pid}/fd"));
            fs::create_dir_all(&dir).unwrap();
            for (i, target) in fds.iter().enumerate() {
                symlink(target, dir.join(i.to_string())).unwrap();
            }
        }
    }

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn finds_gpu_users_once_each() {
        let r = Root::new("users");
        r.process(
            10,
            "Xorg",
            &[
                "/dev/nvidia0", "/dev/nvidiactl",
            ],
        );
        r.process(11, "firefox", &["/dev/dri/renderD128"]);
        r.process(
            12,
            "steam",
            &[
                "/dev/null", "/dev/nvidia-uvm",
            ],
        );
        r.process(13, "steam", &["/dev/nvidia0"]);
        r.write("proc/self-not-a-pid/comm", "x\n");
        assert_eq!(
            gpu_users(&r.0),
            [
                "Xorg", "steam"
            ]
        );
    }

    #[test]
    fn fa506_facts() {
        let r = Root::new("facts");
        r.write("sys/devices/platform/asus-nb-wmi/dgpu_disable", "0\n");
        r.write("sys/bus/pci/devices/0000:01:00.0/power/control", "auto\n");
        let f = facts(&r.0, Some("0000:01:00.0"));
        assert_eq!(f.dgpu_disabled, Some(false));
        assert_eq!(f.can_sleep, Some(true));
        assert!(!f.has_mux);
        assert!(!f.has_powerd);
        assert!(f.users.is_empty());

        r.write("usr/lib/systemd/system/nvidia-powerd.service", "");
        r.write("sys/devices/platform/asus-nb-wmi/gpu_mux_mode", "1\n");
        let f = facts(&r.0, None);
        assert!(f.has_mux && f.has_powerd);
        assert_eq!(f.can_sleep, None);
    }
}
