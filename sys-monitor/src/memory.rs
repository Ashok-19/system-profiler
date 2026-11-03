//! RAM and swap usage from `/proc/meminfo`.

use std::path::Path;

use crate::sysfs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemoryStats {
    pub total_bytes: u64,
    /// Memory available for new allocations without swapping (`MemAvailable`).
    pub available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_free_bytes: u64,
}

impl MemoryStats {
    #[must_use]
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.available_bytes)
    }

    #[must_use]
    pub fn swap_used_bytes(&self) -> u64 {
        self.swap_total_bytes.saturating_sub(self.swap_free_bytes)
    }

    /// RAM in use, 0-100.
    #[must_use]
    pub fn used_percent(&self) -> Option<f32> {
        if self.total_bytes == 0 {
            return None;
        }
        #[allow(clippy::cast_precision_loss)] // ratio only
        Some(self.used_bytes() as f32 / self.total_bytes as f32 * 100.0)
    }
}

pub(crate) fn sample(root: &Path) -> Option<MemoryStats> {
    let text = sysfs::read_trimmed(&root.join("proc/meminfo"))?;
    let field = |key: &str| -> Option<u64> {
        let line = text.lines().find(|l| l.split(':').next() == Some(key))?;
        let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        kib.checked_mul(1024)
    };
    Some(MemoryStats {
        total_bytes: field("MemTotal")?,
        available_bytes: field("MemAvailable")?,
        swap_total_bytes: field("SwapTotal").unwrap_or(0),
        swap_free_bytes: field("SwapFree").unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::Fixture;

    #[test]
    fn parses_meminfo() {
        let fx = Fixture::new();
        fx.write(
            "proc/meminfo",
            "MemTotal:       16000 kB\nMemFree:  1000 kB\nMemAvailable:    4000 kB\n\
             SwapTotal:       2000 kB\nSwapFree:           500 kB\n",
        );
        let m = sample(fx.root()).unwrap();
        assert_eq!(m.total_bytes, 16000 * 1024);
        assert_eq!(m.used_bytes(), 12000 * 1024);
        assert_eq!(m.swap_used_bytes(), 1500 * 1024);
        assert_eq!(m.used_percent(), Some(75.0));
    }

    #[test]
    fn missing_meminfo() {
        assert_eq!(sample(Fixture::new().root()), None);
    }
}
