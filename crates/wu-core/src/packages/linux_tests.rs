use super::*;
use crate::packages::testing::Machine;

#[test]
fn dpkg_lists_installed_packages_only() {
    let text = "ii \tvlc\t3.0.20-3\t300\tmultimedia player\n\
                rc \toldapp\t1.0\t10\tgone\n\
                ii \tlibc6\t2.39\t12000\tGNU C Library\n";
    let packages = parse_dpkg(text);
    assert_eq!(packages.len(), 2);
    assert_eq!(packages[0].size, Some(300 * 1024));
    assert_eq!(packages[0].summary.as_deref(), Some("multimedia player"));
}

#[test]
fn rpm_pacman_flatpak_and_snap_listings_are_read() {
    let rpm =
        parse_rpm("firefox\t131.0-1.fc41\t250000000\tMozilla Firefox\ngpg-pubkey\t1-1\t0\tkey\n");
    assert_eq!(rpm.len(), 1);
    assert_eq!(rpm[0].size, Some(250_000_000));

    let pacman = parse_pacman(
        "Name            : vlc\nVersion         : 3.0.21-1\nDescription     : Media player\nInstalled Size  : 12.50 MiB\n\n\
         Name            : glibc\nVersion         : 2.40\nInstalled Size  : 47.00 MiB\n",
    );
    assert_eq!(pacman.len(), 2);
    assert_eq!(pacman[0].size, Some(13_107_200));

    let flatpak = parse_flatpak(
        "org.gimp.GIMP\tGNU Image Manipulation Program\t2.10.38\t1.1 GB\tsystem\n\
         com.spotify.Client\tSpotify\t1.2.31\t293.5 MB\tuser\n",
    );
    assert_eq!(flatpak.len(), 2);
    assert!(flatpak.iter().all(|p| p.app));
    assert_eq!(flatpak[1].installation.as_deref(), Some("user"));

    let snap = parse_snap(
        "Name    Version   Rev    Tracking       Publisher   Notes\n\
         core22  20240111  1122   latest/stable  canonical✓  base\n\
         firefox 131.0     4793   latest/stable  mozilla✓    -\n\
         snapd   2.63      21759  latest/stable  canonical✓  snapd\n",
    );
    let names: Vec<_> = snap.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(names, ["firefox"]);
}

#[test]
fn appimage_names_lose_their_versions() {
    assert_eq!(app_name("Krita-5.2.2-x86_64"), "Krita");
    assert_eq!(app_name("Obsidian-1.6.7"), "Obsidian");
    assert_eq!(app_name("balena_etcher_amd64"), "balena etcher");
    assert_eq!(app_name("123"), "123");
}

#[test]
fn packages_with_a_visible_launcher_are_applications_named_by_it() {
    let machine = Machine::new(&["dpkg-query"]);
    let apps = machine.at("/usr/share/applications");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(
        apps.join("vlc.desktop"),
        "[Desktop Entry]\nName=VLC media player\nName[de]=VLC\nExec=vlc\n[Desktop Action new]\nName=New\n",
    )
    .unwrap();
    std::fs::write(
        apps.join("helper.desktop"),
        "[Desktop Entry]\nName=Helper\nNoDisplay=true\n",
    )
    .unwrap();
    machine.prints(
        "dpkg-query",
        "ii \tvlc\t3.0\t300\tplayer\nii \thelper\t1.0\t1\thelp\nii \tlibc6\t2.39\t1\tlibc\n\
         vlc-bin, vlc: /usr/share/applications/vlc.desktop\nhelper: /usr/share/applications/helper.desktop\n",
    );
    let (packages, problems) = list(&machine);
    assert!(problems.is_empty(), "{problems:?}");
    let vlc = packages.iter().find(|p| p.id == "vlc").unwrap();
    assert!(vlc.app);
    assert_eq!(vlc.name, "VLC media player");
    assert!(!packages.iter().find(|p| p.id == "helper").unwrap().app);
    assert!(!packages.iter().find(|p| p.id == "libc6").unwrap().app);
    let asked = machine.ran();
    assert!(
        asked
            .iter()
            .any(|line| line.starts_with("dpkg-query -S /usr/share/applications/")),
        "{asked:?}"
    );
}

#[test]
fn appimages_in_the_home_folder_are_applications() {
    let machine = Machine::new(&[]);
    let folder = machine.home().unwrap().join("Applications");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("Krita-5.2.2-x86_64.AppImage"), b"ELF").unwrap();
    std::fs::write(folder.join("notes.txt"), b"x").unwrap();
    let (packages, _) = list(&machine);
    assert_eq!(packages.len(), 1);
    assert_eq!(
        (packages[0].manager, packages[0].name.as_str()),
        (Manager::AppImage, "Krita")
    );
}
