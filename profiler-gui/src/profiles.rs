//! Scenario Profiles: finding which profile's apps are running, and the
//! glue for adding and removing profiles in the UI.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::config::Profile;
use crate::{AppWindow, Prefs, ProfileData, Profiles};

/// The kernel truncates process names (`comm`) to 15 bytes.
const COMM_LEN: usize = 15;

/// Lowercased names of all running processes.
pub fn running(root: &Path) -> HashSet<String> {
    let Ok(procs) = fs::read_dir(root.join("proc")) else {
        return HashSet::new();
    };
    procs
        .flatten()
        .filter(|p| {
            p.file_name()
                .to_str()
                .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
        })
        .filter_map(|p| fs::read_to_string(p.path().join("comm")).ok())
        .map(|comm| comm.trim().to_lowercase())
        .collect()
}

/// How a trigger name appears in `comm`: lowercased and truncated.
fn as_comm(app: &str) -> String {
    let lower = app.trim().to_lowercase();
    let mut end = lower.len().min(COMM_LEN);
    while !lower.is_char_boundary(end) {
        end -= 1;
    }
    lower[..end].to_owned()
}

/// First profile with any of its trigger apps running. `triggers[i]` is
/// empty for profiles that are disabled or manual.
pub fn matching(triggers: &[Vec<String>], running: &HashSet<String>) -> Option<usize> {
    triggers
        .iter()
        .position(|apps| apps.iter().any(|a| running.contains(&as_comm(a))))
}

/// Trigger lists for automatic switching, aligned with `profiles`.
pub fn triggers(profiles: &[Profile]) -> Vec<Vec<String>> {
    profiles
        .iter()
        .map(|p| {
            if p.enabled {
                p.apps.clone()
            } else {
                Vec::new()
            }
        })
        .collect()
}

pub fn to_ui(p: &Profile) -> ProfileData {
    ProfileData {
        name: p.name.as_str().into(),
        enabled: p.enabled,
        apps: p.apps.join(", ").into(),
        mode: i32::from(p.mode),
        charge_limit: i32::from(p.charge_limit),
        refresh: i32::from(p.refresh),
        keyboard: i32::from(p.keyboard),
    }
}

pub fn from_ui(p: &ProfileData) -> Profile {
    let small = |v: i32| i8::try_from(v).unwrap_or(-1);
    Profile {
        name: p.name.trim().to_owned(),
        enabled: p.enabled,
        apps: p
            .apps
            .split([',', ' '])
            .map(str::trim)
            .filter(|a| !a.is_empty())
            .map(str::to_owned)
            .collect(),
        mode: small(p.mode),
        charge_limit: u8::try_from(p.charge_limit).unwrap_or(0),
        refresh: small(p.refresh),
        keyboard: small(p.keyboard),
    }
    .sanitized()
}

pub fn show(ui: &AppWindow, profiles: &[Profile], auto: bool) {
    let p = ui.global::<Profiles>();
    let rows: Vec<ProfileData> = profiles.iter().map(to_ui).collect();
    p.set_list(ModelRc::new(VecModel::from(rows)));
    p.set_auto(auto);
}

pub fn read(ui: &AppWindow) -> Vec<Profile> {
    ui.global::<Profiles>()
        .get_list()
        .iter()
        .map(|p| from_ui(&p))
        .collect()
}

fn list_model(ui: &AppWindow) -> Option<ModelRc<ProfileData>> {
    let list = ui.global::<Profiles>().get_list();
    list.as_any().is::<VecModel<ProfileData>>().then_some(list)
}

/// Adding and removing rows needs the model, so it happens here.
pub fn wire(ui: &AppWindow) {
    let p = ui.global::<Profiles>();
    let weak = ui.as_weak();
    p.on_add(move || {
        let Some(ui) = weak.upgrade() else { return };
        if let Some(list) = list_model(&ui)
            && let Some(vec) = list.as_any().downcast_ref::<VecModel<ProfileData>>()
        {
            vec.push(to_ui(&Profile::default()));
            ui.global::<Prefs>().invoke_save();
        }
    });
    let weak = ui.as_weak();
    p.on_remove(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        if let (Some(list), Ok(index)) = (list_model(&ui), usize::try_from(index))
            && let Some(vec) = list.as_any().downcast_ref::<VecModel<ProfileData>>()
            && index < vec.row_count()
        {
            vec.remove(index);
            ui.global::<Prefs>().invoke_save();
        }
    });
}

/// Summary of what a profile changes, e.g. "Turbo · Fastest refresh".
pub fn summary(p: &ProfileData) -> SharedString {
    let mut parts: Vec<String> = Vec::new();
    match p.mode {
        0 => parts.push("Silent".to_owned()),
        1 => parts.push("Performance".to_owned()),
        2 => parts.push("Turbo".to_owned()),
        _ => {}
    }
    if p.charge_limit > 0 {
        parts.push(format!("Charge to {}%", p.charge_limit));
    }
    match p.refresh {
        0 => parts.push("Fastest refresh".to_owned()),
        1 => parts.push("Slowest refresh".to_owned()),
        _ => {}
    }
    match p.keyboard {
        0 => parts.push("Keyboard off".to_owned()),
        1 => parts.push("Keyboard low".to_owned()),
        2 => parts.push("Keyboard medium".to_owned()),
        3 => parts.push("Keyboard high".to_owned()),
        _ => {}
    }
    if parts.is_empty() {
        "Changes nothing yet".into()
    } else {
        parts.join(" · ").into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_profiles;

    fn names(list: &[&str]) -> HashSet<String> {
        list.iter().map(|&n| n.to_owned()).collect()
    }

    #[test]
    fn first_running_profile_wins() {
        let t = triggers(&default_profiles());
        assert_eq!(
            matching(
                &t,
                &names(&[
                    "bash", "xfwm4"
                ])
            ),
            None
        );
        assert_eq!(matching(&t, &names(&["mpv"])), Some(2));
        assert_eq!(
            matching(
                &t,
                &names(&[
                    "mpv", "steam"
                ])
            ),
            Some(0)
        );
    }

    #[test]
    fn long_and_mixed_case_names() {
        let t = vec![vec![
            "SuperLongGameLauncher".to_owned(),
        ]];
        assert_eq!(matching(&t, &names(&["superlonggamela"])), Some(0));
        assert_eq!(as_comm(" Steam "), "steam");
        assert_eq!(as_comm("ééééééééééé"), "ééééééé", "cut on a char boundary");
    }

    #[test]
    fn disabled_profiles_never_trigger() {
        let mut profiles = default_profiles();
        profiles[2].enabled = false;
        assert_eq!(matching(&triggers(&profiles), &names(&["mpv"])), None);
    }

    #[test]
    fn ui_round_trip() {
        for p in default_profiles() {
            assert_eq!(from_ui(&to_ui(&p)), p);
        }
        let typed = ProfileData {
            name: "  Blender  ".into(),
            apps: "blender,  obs ,".into(),
            mode: 2,
            charge_limit: 80,
            refresh: -1,
            keyboard: 9,
            enabled: true,
        };
        let p = from_ui(&typed);
        assert_eq!(p.name, "Blender");
        assert_eq!(
            p.apps,
            [
                "blender", "obs"
            ]
        );
        assert_eq!(p.keyboard, -1);
        assert_eq!(summary(&typed), "Turbo · Charge to 80%");
    }

    #[test]
    fn running_reads_comm() {
        let dir = std::env::temp_dir().join(format!("profiler-procs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for (pid, comm) in [
            ("1", "systemd"),
            ("42", "Steam"),
            ("self", "nope"),
        ] {
            fs::create_dir_all(dir.join("proc").join(pid)).unwrap();
            fs::write(dir.join("proc").join(pid).join("comm"), format!("{comm}\n")).unwrap();
        }
        assert_eq!(
            running(&dir),
            names(&[
                "systemd", "steam"
            ])
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
