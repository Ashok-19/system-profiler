//! "Start with the desktop session" through an XDG autostart entry in
//! `$XDG_CONFIG_HOME/autostart/system-profiler.desktop`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub fn default_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("autostart")
        .join("system-profiler.desktop")
}

pub fn is_enabled(path: &Path) -> bool {
    path.is_file()
}

/// Quote `exe` for an Exec line when it has characters the desktop
/// entry spec reserves.
fn exec_arg(exe: &Path) -> String {
    let s = exe.to_string_lossy();
    if s.chars()
        .any(|c| c.is_whitespace() || "\"'\\$`".contains(c))
    {
        let escaped: String = s
            .chars()
            .flat_map(|c| {
                let escape = matches!(c, '"' | '`' | '$' | '\\');
                escape.then_some('\\').into_iter().chain(std::iter::once(c))
            })
            .collect();
        format!("\"{escaped}\"")
    } else {
        s.into_owned()
    }
}

fn entry(exe: &Path, minimized: bool) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=System Profiler\n\
         Comment=ASUS laptop control center\n\
         Exec={}{}\n\
         Icon=system-profiler\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n",
        exec_arg(exe),
        if minimized { " --minimized" } else { "" }
    )
}

/// Create or remove the autostart entry for `exe`, optionally starting
/// hidden in the tray.
pub fn set(path: &Path, enabled: bool, exe: &Path, minimized: bool) -> io::Result<()> {
    if enabled {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, entry(exe, minimized))
    } else {
        match fs::remove_file(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_remove() {
        let dir = std::env::temp_dir().join(format!("profiler-autostart-{}", std::process::id()));
        let path = dir.join("autostart").join("system-profiler.desktop");
        assert!(!is_enabled(&path));
        set(&path, true, Path::new("/usr/bin/system-profiler"), false).unwrap();
        assert!(is_enabled(&path));
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\nExec=/usr/bin/system-profiler\n"), "{text}");
        set(&path, true, Path::new("/usr/bin/system-profiler"), true).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("\nExec=/usr/bin/system-profiler --minimized\n"),
            "{text}"
        );
        set(&path, false, Path::new("/unused"), false).unwrap();
        assert!(!is_enabled(&path));
        set(&path, false, Path::new("/unused"), false).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn quoting() {
        assert_eq!(exec_arg(Path::new("/opt/a b/app")), "\"/opt/a b/app\"");
        assert_eq!(exec_arg(Path::new("/opt/$x")), "\"/opt/\\$x\"");
        assert_eq!(exec_arg(Path::new("/usr/bin/app")), "/usr/bin/app");
    }
}
