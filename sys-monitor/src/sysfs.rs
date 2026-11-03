//! Small helpers for reading procfs/sysfs files.
//!
//! Every reader returns `None` instead of an error: sensors come and go
//! (hotplug, runtime PM, driver differences) and a missing value must never
//! break a whole sample.

use std::fs;
use std::path::{Path, PathBuf};

/// Read a file and trim surrounding whitespace.
pub(crate) fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_owned())
}

/// Read a file containing a single unsigned integer.
pub(crate) fn read_u64(path: &Path) -> Option<u64> {
    read_trimmed(path)?.parse().ok()
}

/// Read a file containing a single signed integer.
pub(crate) fn read_i64(path: &Path) -> Option<i64> {
    read_trimmed(path)?.parse().ok()
}

/// File name of the target of a symlink, e.g. the driver name from
/// `.../device/driver -> ../../bus/pci/drivers/amdgpu`.
pub(crate) fn link_basename(path: &Path) -> Option<String> {
    let target = fs::read_link(path).ok()?;
    target.file_name()?.to_str().map(str::to_owned)
}

/// Sorted entries of a directory whose names satisfy `filter`.
pub(crate) fn dir_entries(dir: &Path, filter: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_str().is_some_and(&filter))
        .map(|e| e.path())
        .collect();
    paths.sort_by(|a, b| natural_cmp(a, b));
    paths
}

/// Compare paths so that `cpu2` sorts before `cpu10`.
fn natural_cmp(a: &Path, b: &Path) -> std::cmp::Ordering {
    fn split(p: &Path) -> (String, Option<u64>) {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let digits_at = name
            .rfind(|c: char| !c.is_ascii_digit())
            .map_or(0, |i| i + 1);
        let (prefix, num) = name.split_at(digits_at);
        (prefix.to_owned(), num.parse().ok())
    }
    split(a).cmp(&split(b))
}

/// A hardware monitoring chip under `/sys/class/hwmon`.
///
/// Chips are identified by `name` because the `hwmonN` index is not stable
/// across boots.
#[derive(Debug, Clone)]
pub(crate) struct Hwmon {
    pub name: String,
    pub path: PathBuf,
}

impl Hwmon {
    /// Read an integer attribute such as `temp1_input`.
    pub fn read_i64(&self, file: &str) -> Option<i64> {
        read_i64(&self.path.join(file))
    }

    /// Read the label of a channel, e.g. `fan1_label` for `("fan", "1")`.
    pub fn read_label(&self, prefix: &str, index: &str) -> Option<String> {
        read_trimmed(&self.path.join(format!("{prefix}{index}_label")))
    }

    /// Indices of all `<prefix>N_input` files, e.g. `["1", "2"]` for fans.
    pub fn input_indices(&self, prefix: &str) -> Vec<String> {
        let mut indices: Vec<String> = dir_entries(&self.path, |_| true)
            .iter()
            .filter_map(|p| p.file_name()?.to_str())
            .filter_map(|n| n.strip_prefix(prefix)?.strip_suffix("_input"))
            .filter(|idx| !idx.is_empty() && idx.bytes().all(|b| b.is_ascii_digit()))
            .map(str::to_owned)
            .collect();
        indices.sort_by_key(|i| i.parse::<u32>().unwrap_or(u32::MAX));
        indices
    }
}

/// All hwmon chips under `<root>/sys/class/hwmon`.
pub(crate) fn hwmons(root: &Path) -> Vec<Hwmon> {
    dir_entries(&root.join("sys/class/hwmon"), |n| n.starts_with("hwmon"))
        .into_iter()
        .filter_map(|path| {
            let name = read_trimmed(&path.join("name"))?;
            Some(Hwmon { name, path })
        })
        .collect()
}

/// First hwmon chip with the given name.
pub(crate) fn hwmon_named<'a>(chips: &'a [Hwmon], name: &str) -> Option<&'a Hwmon> {
    chips.iter().find(|h| h.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::Fixture;

    #[test]
    fn natural_sort_orders_numbers() {
        let fx = Fixture::new();
        for n in [
            "cpu10", "cpu2", "cpu1", "cpufreq",
        ] {
            fx.write(&format!("d/{n}"), "");
        }
        let names: Vec<String> = dir_entries(&fx.path("d"), |n| n.starts_with("cpu"))
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            names,
            [
                "cpu1", "cpu2", "cpu10", "cpufreq"
            ]
        );
    }

    #[test]
    fn hwmon_input_indices() {
        let fx = Fixture::new();
        fx.write("sys/class/hwmon/hwmon3/name", "asus\n");
        fx.write("sys/class/hwmon/hwmon3/fan1_input", "2700\n");
        fx.write("sys/class/hwmon/hwmon3/fan10_input", "1\n");
        fx.write("sys/class/hwmon/hwmon3/fan2_input", "2800\n");
        fx.write("sys/class/hwmon/hwmon3/fan1_label", "cpu_fan\n");
        fx.write("sys/class/hwmon/hwmon3/fanX_input", "bogus\n");
        let chips = hwmons(fx.root());
        let asus = hwmon_named(&chips, "asus").unwrap();
        assert_eq!(
            asus.input_indices("fan"),
            [
                "1", "2", "10"
            ]
        );
        assert_eq!(asus.read_label("fan", "1").as_deref(), Some("cpu_fan"));
        assert_eq!(asus.read_i64("fan2_input"), Some(2800));
    }

    #[test]
    fn missing_files_are_none() {
        let fx = Fixture::new();
        assert_eq!(read_u64(&fx.path("nope")), None);
        assert!(hwmons(fx.root()).is_empty());
    }
}
