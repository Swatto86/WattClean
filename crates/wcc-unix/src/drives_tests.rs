use super::*;

const MOUNTS: &str = "\
/dev/nvme0n1p2 / btrfs rw,subvol=/@ 0 0
proc /proc proc rw 0 0
tmpfs /tmp tmpfs rw 0 0
/dev/nvme0n1p2 /home btrfs rw,subvol=/@home 0 0
/dev/nvme0n1p1 /boot/efi vfat rw 0 0
/dev/loop3 /snap/core/16928 squashfs ro 0 0
/dev/sdb1 /media/t/USB\\040Stick exfat rw 0 0
";

#[test]
fn real_drives_are_listed_once_with_their_names_unescaped() {
    let mounts = parse_mounts(MOUNTS);
    let points: Vec<&str> = wipeable(&mounts).iter().map(|m| m.point.as_str()).collect();
    assert_eq!(points, ["/", "/media/t/USB Stick"]);
    assert_eq!(unescape(r"a\134b\x"), r"a\b\x");
}

/// Omarchy's and Fedora's Btrfs compresses (`compress=zstd`), where a
/// zero fill takes a sliver of its size: no mount of it is wiped.
#[test]
fn a_compressing_file_system_is_never_wiped() {
    let mounts = parse_mounts(
        "/dev/nvme0n1p2 / btrfs rw,relatime,compress=zstd:3,ssd,subvol=/@ 0 0\n\
         /dev/nvme0n1p2 /home btrfs rw,relatime,compress=zstd:3,ssd,subvol=/@home 0 0\n\
         /dev/sda1 /data btrfs rw,compress=no 0 0\n\
         /dev/sdb1 /backup btrfs rw,compress-force=zstd:1 0 0\n\
         /dev/sdc1 /media/t/bcache bcachefs rw,compression=lz4 0 0\n\
         /dev/sdd1 /media/t/plain bcachefs rw,compression=none 0 0\n",
    );
    let points: Vec<&str> = wipeable(&mounts).iter().map(|m| m.point.as_str()).collect();
    assert_eq!(points, ["/data", "/media/t/plain"]);
}

/// ostree systems mount the root device read-only at /sysroot first;
/// ZFS datasets are never wiped (a zero fill never fills a compressed
/// one).
#[test]
fn a_read_only_first_mount_does_not_hide_the_writable_one() {
    let mounts = parse_mounts(
        "/dev/vda3 /sysroot xfs ro,relatime 0 0\n\
         /dev/vda3 /var xfs rw,relatime 0 0\n\
         rpool/ROOT/ubuntu /srv/pool zfs rw,xattr 0 0\n",
    );
    let points: Vec<&str> = wipeable(&mounts).iter().map(|m| m.point.as_str()).collect();
    assert_eq!(points, ["/var"]);
}

/// openSUSE's layout: `/`, `/home` and `/var` are subvolumes of one
/// Btrfs device, `/tmp` is in memory.
#[test]
fn a_wipe_folder_counts_on_any_mount_of_the_drives_device() {
    let mounts = parse_mounts(
        "/dev/nvme0n1p2 / btrfs rw,subvol=/@/.snapshots/1/snapshot 0 0\n\
         tmpfs /tmp tmpfs rw 0 0\n\
         /dev/nvme0n1p2 /home btrfs rw,subvol=/@/home 0 0\n\
         /dev/nvme0n1p2 /var btrfs rw,subvol=/@/var 0 0\n\
         /dev/sdb1 /media/t/USB exfat rw 0 0\n\
         tmpfs /media/t/USB tmpfs rw 0 0\n",
    );
    let on_root = |folder: &str| same_device(&mounts, Path::new("/"), Path::new(folder));
    assert!(on_root("/home/t"));
    assert!(on_root("/var/tmp"));
    assert!(on_root("/homework"));
    assert!(!on_root("/tmp"));
    assert!(!on_root("/tmp/sub"));
    assert!(!on_root("/media/t/USB/folder"));
    let hidden = mount_of(&mounts, Path::new("/media/t/USB/folder")).unwrap();
    assert_eq!(
        hidden.file_system, "tmpfs",
        "the later mount hides the disk"
    );
    assert!(!same_device(
        &mounts,
        Path::new("/media/t/USB"),
        Path::new("/home/t")
    ));
    assert_eq!(
        mount_of(&mounts, Path::new("/home/t")).unwrap().point,
        "/home"
    );
    assert_eq!(
        mount_of(&mounts, Path::new("/homework")).unwrap().point,
        "/"
    );
}

#[test]
fn a_wipe_writes_on_the_drive_itself() {
    let dir = tempfile::tempdir().unwrap();
    let space = sys::space(dir.path()).unwrap();
    let drive = Drive {
        root: dir.path().to_string_lossy().into_owned(),
        label: String::new(),
        file_system: String::new(),
        free: space.free,
        total: space.total,
        removable: false,
    };
    let inside = dir.path().join("inside");
    std::fs::create_dir(&inside).unwrap();
    // A folder on another file system, even to an administrator: macOS's
    // read-only system volume, Linux's /proc (its `/` may share the
    // temporary folder's device).
    let elsewhere = PathBuf::from(if cfg!(target_os = "macos") {
        "/"
    } else {
        "/proc"
    });
    assert_eq!(wipe_folder(&drive, &[elsewhere, inside.clone()]), inside);
    assert_eq!(wipe_folder(&drive, &[]), dir.path());
}

/// macOS: the data volume's folders are not below its mount point.
#[cfg(target_os = "macos")]
#[test]
fn a_wipe_writes_on_the_same_volume_wherever_its_path_points() {
    let root = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let space = sys::space(root.path()).unwrap();
    let drive = Drive {
        root: root.path().to_string_lossy().into_owned(),
        label: String::new(),
        file_system: String::new(),
        free: space.free,
        total: space.total,
        removable: false,
    };
    let candidate = elsewhere.path().to_path_buf();
    assert_eq!(
        wipe_folder(&drive, std::slice::from_ref(&candidate)),
        candidate
    );
}

#[cfg(target_os = "macos")]
#[test]
fn this_machine_has_a_drive_to_wipe() {
    assert!(!drives().is_empty());
}

/// A Linux machine whose disks all compress (Btrfs as Fedora and
/// Omarchy mount it) has none to wipe; any other has.
#[cfg(not(target_os = "macos"))]
#[test]
fn this_machines_drives_are_its_wipeable_mounts() {
    let text = std::fs::read_to_string("/proc/self/mounts").unwrap();
    let mounts = parse_mounts(&text);
    let wipeable: Vec<&str> = wipeable(&mounts).iter().map(|m| m.point.as_str()).collect();
    let offered: Vec<String> = drives().into_iter().map(|drive| drive.root).collect();
    assert!(offered.iter().all(|root| wipeable.contains(&root.as_str())));
    assert_eq!(offered.is_empty(), wipeable.is_empty(), "{offered:?}");
}
