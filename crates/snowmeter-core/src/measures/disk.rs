//! FreeDiskSpace (`statvfs`).

use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DriveType {
    Error = 0,
    Removed = 1,
    Removable = 3,
    Fixed = 4,
    Network = 5,
    CdRom = 6,
    Ram = 7,
}

impl DriveType {
    pub fn name(self) -> &'static str {
        match self {
            DriveType::Error => "Error",
            DriveType::Removed => "Removed",
            DriveType::Removable => "Removable",
            DriveType::Fixed => "Fixed",
            DriveType::Network => "Network",
            DriveType::CdRom => "CDRom",
            DriveType::Ram => "Ram",
        }
    }
}

/// One `/proc/self/mountinfo` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Mount {
    pub mount_point: String,
    pub fstype: String,
    pub source: String,
}

pub fn parse_mountinfo(text: &str) -> Vec<Mount> {
    let unescape = |s: &str| s.replace("\\040", " ").replace("\\011", "\t").replace("\\134", "\\");
    let mut out = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split(' ').collect();
        let Some(dash) = parts.iter().position(|p| *p == "-") else { continue };
        if parts.len() < dash + 3 || parts.len() < 5 {
            continue;
        }
        out.push(Mount {
            mount_point: unescape(parts[4]),
            fstype: parts[dash + 1].to_string(),
            source: unescape(parts[dash + 2]),
        });
    }
    out
}

/// The mount containing `path` (longest matching mount point).
pub fn mount_for<'a>(mounts: &'a [Mount], path: &str) -> Option<&'a Mount> {
    mounts
        .iter()
        .filter(|m| {
            m.mount_point == "/"
                || path == m.mount_point
                || path.strip_prefix(m.mount_point.as_str()).is_some_and(|r| r.starts_with('/'))
        })
        .max_by_key(|m| m.mount_point.len())
}

/// Classify a mount like Windows `GetDriveType` would.
pub fn classify(m: &Mount, removable_block: bool) -> DriveType {
    match m.fstype.as_str() {
        "nfs" | "nfs4" | "cifs" | "smb3" | "smbfs" | "sshfs" | "fuse.sshfs" | "9p" | "afs" => DriveType::Network,
        "tmpfs" | "ramfs" => DriveType::Ram,
        "iso9660" | "udf" => DriveType::CdRom,
        _ => {
            if removable_block || m.mount_point.starts_with("/media") || m.mount_point.starts_with("/run/media") {
                DriveType::Removable
            } else {
                DriveType::Fixed
            }
        }
    }
}

/// Map a Rainmeter `Drive=` value to a path: `C:`/`C:\` and every other drive
/// letter become `/`; anything else is taken as a POSIX path.
pub fn map_drive(drive: &str) -> String {
    let d = drive.trim().replace('\\', "/");
    let b = d.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        let rest = &d[2..];
        return if rest.trim_matches('/').is_empty() { "/".to_string() } else { format!("/{}", rest.trim_start_matches('/')) };
    }
    if d.is_empty() {
        return d;
    }
    d
}

/// (total, free_to_user, free_total) in bytes for `path`.
pub fn statvfs(path: &str) -> Option<(f64, f64, f64)> {
    let c = std::ffi::CString::new(path).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c.as_ptr(), &mut s) };
    if rc != 0 {
        return None;
    }
    let fr = s.f_frsize as f64;
    Some((s.f_blocks as f64 * fr, s.f_bavail as f64 * fr, s.f_bfree as f64 * fr))
}

fn block_removable(source: &str) -> bool {
    let Some(dev) = source.strip_prefix("/dev/") else { return false };
    // partition -> parent disk (sda1 -> sda, nvme0n1p2 -> nvme0n1, mmcblk0p1 -> mmcblk0)
    let mut base = dev.to_string();
    while base.ends_with(|c: char| c.is_ascii_digit()) {
        base.pop();
        if base.ends_with('p') && (base.starts_with("nvme") || base.starts_with("mmcblk")) {
            base.pop();
            break;
        }
    }
    for cand in [dev.to_string(), base] {
        if let Ok(s) = std::fs::read_to_string(format!("/sys/block/{cand}/removable")) {
            return s.trim() == "1";
        }
    }
    false
}

fn volume_label(source: &str) -> String {
    let Ok(rd) = std::fs::read_dir("/dev/disk/by-label") else { return String::new() };
    let want = std::fs::canonicalize(source).ok();
    for e in rd.flatten() {
        if std::fs::canonicalize(e.path()).ok() == want && want.is_some() {
            let n = e.file_name().to_string_lossy().to_string();
            return unescape_hex(&n);
        }
    }
    String::new()
}

fn unescape_hex(s: &str) -> String {
    let mut out = String::new();
    let b = s.as_bytes();
    let mut bytes = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() && b[i + 1] == b'x' {
            if let Some(v) = s.get(i + 2..i + 4).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                bytes.push(v);
                i += 4;
                continue;
            }
        }
        bytes.push(b[i]);
        i += 1;
    }
    out.push_str(&String::from_utf8_lossy(&bytes));
    out
}

pub struct DiskMeasure {
    drive: String,
    total: bool,
    type_mode: bool,
    label: bool,
    ignore_removable: bool,
    disk_quota: bool,
    max: f64,
    value: f64,
    string: Option<String>,
    log: Vec<String>,
}

impl DiskMeasure {
    pub fn new() -> DiskMeasure {
        DiskMeasure {
            drive: "/".to_string(),
            total: false,
            type_mode: false,
            label: false,
            ignore_removable: true,
            disk_quota: true,
            max: 0.0,
            value: 0.0,
            string: None,
            log: Vec::new(),
        }
    }

    fn refresh(&mut self) {
        self.string = None;
        if self.drive.is_empty() {
            self.value = 0.0;
            self.max = 0.0;
            return;
        }
        let mounts = std::fs::read_to_string("/proc/self/mountinfo").map(|t| parse_mountinfo(&t)).unwrap_or_default();
        let path = self.drive.clone();
        let mount = mount_for(&mounts, &path);
        let removable = mount.is_some_and(|m| block_removable(&m.source));
        let dtype = match (Path::new(&path).exists(), mount) {
            (false, _) => DriveType::Removed,
            (true, Some(m)) => classify(m, removable),
            (true, None) => DriveType::Fixed,
        };
        if self.type_mode {
            self.value = dtype as i32 as f64;
            self.string = Some(dtype.name().to_string());
            self.max = 0.0;
            return;
        }
        if (self.ignore_removable && dtype == DriveType::Removable) || dtype == DriveType::CdRom || dtype == DriveType::Removed {
            self.value = 0.0;
            self.max = 0.0;
            if self.label {
                self.string = Some(String::new());
            }
            return;
        }
        match statvfs(&path) {
            Some((total, avail, free)) => {
                self.max = total;
                let f = if self.disk_quota { avail } else { free };
                self.value = if self.total { total } else { f };
            }
            None => {
                self.value = 0.0;
                self.max = 0.0;
            }
        }
        if self.label {
            self.string = Some(mount.map(|m| volume_label(&m.source)).unwrap_or_default());
        }
    }
}

impl Default for DiskMeasure {
    fn default() -> Self {
        DiskMeasure::new()
    }
}

impl MeasureImpl for DiskMeasure {
    fn read_options(&mut self, opt: &OptionReader) {
        let raw = opt.string("Drive", "C:\\");
        if raw.trim().is_empty() {
            self.log.push("FreeDiskSpace: Drive= empty".to_string());
        }
        self.drive = map_drive(&raw);
        self.total = opt.bool("Total", false);
        self.type_mode = opt.bool("Type", false);
        self.label = opt.bool("Label", false);
        self.ignore_removable = opt.bool("IgnoreRemovable", true);
        self.disk_quota = opt.bool("DiskQuota", true);
        self.refresh();
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        self.refresh();
        self.value
    }

    fn string_value(&self) -> Option<String> {
        self.string.clone()
    }

    fn forced_max(&self) -> Option<f64> {
        Some(self.max)
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::{ctx_run, skin};

    const MOUNTINFO: &str = "\
24 1 259:2 / / rw,relatime shared:1 - ext4 /dev/nvme0n1p2 rw\n\
30 24 0:25 / /tmp rw - tmpfs tmpfs rw\n\
40 24 8:17 / /run/media/me/USB\\040STICK rw - vfat /dev/sdb1 rw\n\
50 24 0:50 / /mnt/nas rw - cifs //srv/share rw\n\
60 24 11:0 / /mnt/cd ro - iso9660 /dev/sr0 ro\n";

    #[test]
    fn parses_and_classifies_mounts() {
        let m = parse_mountinfo(MOUNTINFO);
        assert_eq!(m.len(), 5);
        assert_eq!(m[2].mount_point, "/run/media/me/USB STICK");
        assert_eq!(m[2].fstype, "vfat");
        assert_eq!(classify(&m[0], false), DriveType::Fixed);
        assert_eq!(classify(&m[1], false), DriveType::Ram);
        assert_eq!(classify(&m[2], false), DriveType::Removable);
        assert_eq!(classify(&m[3], false), DriveType::Network);
        assert_eq!(classify(&m[4], false), DriveType::CdRom);
        assert_eq!(classify(&m[0], true), DriveType::Removable);
    }

    #[test]
    fn finds_longest_mount() {
        let m = parse_mountinfo(MOUNTINFO);
        assert_eq!(mount_for(&m, "/tmp/x/y").unwrap().mount_point, "/tmp");
        assert_eq!(mount_for(&m, "/tmpfoo").unwrap().mount_point, "/");
        assert_eq!(mount_for(&m, "/home/u").unwrap().mount_point, "/");
    }

    #[test]
    fn drive_letters_map_to_root() {
        assert_eq!(map_drive("C:\\"), "/");
        assert_eq!(map_drive("c:"), "/");
        assert_eq!(map_drive("D:\\"), "/");
        assert_eq!(map_drive("Z:\\Data\\"), "/Data/");
        assert_eq!(map_drive("/home"), "/home");
        assert_eq!(map_drive("/"), "/");
    }

    fn run(ini: &str) -> (DiskMeasure, f64) {
        let s = skin(ini);
        let mut d = DiskMeasure::new();
        d.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        let v = ctx_run(&s, |c| d.update(c));
        (d, v)
    }

    #[test]
    fn free_and_total_on_root() {
        let (d, free) = run("[M]\nMeasure=FreeDiskSpace\nDrive=C:\n");
        let max = d.forced_max().unwrap();
        assert!(max > 0.0 && free >= 0.0 && free <= max, "{free} {max}");
        let (_d, total) = run("[M]\nMeasure=FreeDiskSpace\nDrive=C:\nTotal=1\n");
        assert!((total - max).abs() < 1.0);
    }

    #[test]
    fn posix_path_drive_works() {
        let (d, free) = run("[M]\nMeasure=FreeDiskSpace\nDrive=/tmp\n");
        assert!(d.forced_max().unwrap() > 0.0 && free >= 0.0);
    }

    #[test]
    fn type_and_label_strings() {
        let (d, v) = run("[M]\nMeasure=FreeDiskSpace\nType=1\n");
        assert!(v == 4.0 || v == 7.0 || v == 5.0 || v == 3.0, "{v}");
        assert!(d.string_value().is_some());
        let (d, _) = run("[M]\nMeasure=FreeDiskSpace\nLabel=1\n");
        assert!(d.string_value().is_some());
        let (d, _) = run("[M]\nMeasure=FreeDiskSpace\n");
        assert!(d.string_value().is_none());
    }

    #[test]
    fn missing_path_is_removed() {
        let (d, v) = run("[M]\nMeasure=FreeDiskSpace\nDrive=/definitely/not/here\nType=1\n");
        assert_eq!(v, 1.0);
        assert_eq!(d.string_value().as_deref(), Some("Removed"));
    }

    #[test]
    fn empty_drive_warns() {
        let s = skin("[M]\nMeasure=FreeDiskSpace\nDrive=\n");
        let mut d = DiskMeasure::new();
        d.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        assert_eq!(d.take_log(), ["FreeDiskSpace: Drive= empty"]);
    }

    #[test]
    fn unescapes_label_names() {
        assert_eq!(unescape_hex("My\\x20Disk"), "My Disk");
    }
}
