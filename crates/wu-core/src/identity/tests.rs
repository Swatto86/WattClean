use super::*;
use crate::fake::FakeSystem;
use crate::model::{Hive, RawValue};
use crate::programs::list_programs;

#[test]
fn product_names_lose_versions_and_architectures() {
    let cases = [
        ("7-Zip 24.08 (x64)", "7-Zip"),
        ("Mozilla Firefox (x64 en-GB)", "Mozilla Firefox"),
        ("Notepad++ (64-bit x64)", "Notepad++"),
        ("Python 3.12.1 (64-bit)", "Python"),
        ("VLC media player", "VLC media player"),
        ("Microsoft Office 2016", "Microsoft Office 2016"),
        ("Git version 2.45.1", "Git version"),
        ("Acme Widget v2.1 x64 -", "Acme Widget"),
    ];
    for (display, want) in cases {
        assert_eq!(product_name(display), want, "{display}");
    }
}

#[test]
fn name_keys_keep_plus_and_hash() {
    assert_eq!(name_key("Notepad++"), "notepad++");
    assert_eq!(name_key("Acme Widget"), "acmewidget");
    assert_eq!(name_key("C# Tools"), "c#tools");
    assert!(is_generic(&name_key("Common Files")));
    assert!(is_generic("ab"));
}

#[test]
fn publisher_keys_drop_legal_suffixes() {
    assert_eq!(
        publisher_key("Acme Software Ltd.").as_deref(),
        Some("acmesoftware")
    );
    assert_eq!(publisher_key("Fabrikam, Inc.").as_deref(), Some("fabrikam"));
    assert_eq!(publisher_key("Microsoft Corporation").as_deref(), None);
    assert_eq!(publisher_key("Inc."), None);
}

#[test]
fn identity_collects_names_dirs_and_executables() {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::seeded(dir.path()).unwrap();
    let folders = Folders::load(&system).unwrap();
    let widget = list_programs(&system, false)
        .into_iter()
        .find(|p| p.name.starts_with("Acme Widget"))
        .unwrap();
    let identity = Identity::of_program(&system, &folders, &widget);
    assert_eq!(
        identity.install_dirs,
        vec![folders.program_files.join(r"Acme\Widget")]
    );
    assert!(
        identity.names.contains(&"acmewidget".to_owned()),
        "{:?}",
        identity.names
    );
    assert_eq!(
        identity.inner_names,
        vec!["widget".to_owned()],
        "only inside Acme's folder"
    );
    assert!(!identity.matches("Widget", Match::Exact));
    assert!(identity.matches_inner("Widget", Match::Exact));
    assert!(
        identity.exe_names.contains(&"widget.exe".to_owned()),
        "{:?}",
        identity.exe_names
    );
    assert!(identity.exe_names.contains(&"widget-svc.exe".to_owned()));
    assert_eq!(identity.publisher_key.as_deref(), Some("acmesoftware"));
    assert!(identity.is_publisher("Acme"));
    assert!(!identity.is_publisher("Ac"));
}

#[test]
fn matching_strength_rises_with_the_mode() {
    let identity = Identity::named("Acme Widget 2.1", Some("Acme Software Ltd."), Vec::new());
    assert!(identity.matches("Acme Widget", Match::Exact));
    assert!(identity.matches("ACME-WIDGET", Match::Exact));
    assert!(!identity.matches("Acme Widget Pro", Match::Exact));
    assert!(identity.matches("Acme Widget Pro", Match::Prefix));
    assert!(!identity.matches("Old Acme Widget", Match::Prefix));
    assert!(identity.matches("Old Acme Widget", Match::Contains));
    assert!(
        !identity.matches("Settings", Match::Contains),
        "generic names never match"
    );
}

#[test]
fn shared_system_folders_are_not_install_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::seeded(dir.path()).unwrap();
    let folders = Folders::load(&system).unwrap();
    let mut program = list_programs(&system, false)
        .into_iter()
        .find(|p| p.name == "Fabrikam Viewer")
        .unwrap();
    program.install_location = None;
    program.icon = Some(
        folders
            .windows
            .join(r"Installer\{X}\viewer.ico")
            .display()
            .to_string(),
    );
    // The entry alone names no folder; Windows Installer's records may.
    let identity = Identity::of_entry(&system, &folders, &program);
    assert!(
        identity.install_dirs.is_empty(),
        "{:?}",
        identity.install_dirs
    );

    program.install_location = Some(folders.program_files.display().to_string());
    // The entry alone names no folder; Windows Installer's records may.
    let identity = Identity::of_entry(&system, &folders, &program);
    assert!(
        identity.install_dirs.is_empty(),
        "a root is never an install dir"
    );
}

#[test]
fn msi_component_folders_name_nothing_and_fold_into_the_install_folder() {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::seeded(dir.path()).unwrap();
    let folders = Folders::load(&system).unwrap();
    let viewer = list_programs(&system, false)
        .into_iter()
        .find(|p| p.name == "Fabrikam Viewer")
        .unwrap();
    let x86 = folders.program_files_x86.clone().unwrap();
    let install = x86.join(r"Fabrikam\Viewer");
    // A product like Go registers a component for every file, in hundreds
    // of folders whose names say nothing about the program.
    let components = RegPath::new(Hive::LocalMachine, crate::msi::COMPONENTS);
    let files = [
        r"src\encoding\json\decode.go",
        r"src\net\http\server.go",
        r"api\go1.txt",
        "LICENSE",
    ];
    for (n, file) in files.into_iter().enumerate() {
        system.put(
            &components.child(&format!("{{C9C9C9C9-0000-0000-0000-00000000000{n}}}")),
            vec![RawValue::string(
                crate::fake::VIEWER_PACKED,
                &install.join(file).display().to_string(),
            )],
        );
    }
    // The folder of a component shared with another product is not added.
    let identity = Identity::of_program(&system, &folders, &viewer);
    assert_eq!(identity.install_dirs, vec![install]);
    for name in ["src", "encoding", "json", "net", "http", "api"] {
        assert!(
            !identity.matches_inner(name, Match::Exact),
            "{name} is a folder inside the program, not its name: {:?}",
            identity.all_names()
        );
    }
}

#[test]
fn only_a_folder_at_a_root_or_under_the_publisher_names_the_program() {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::seeded(dir.path()).unwrap();
    let folders = Folders::load(&system).unwrap();
    let mut program = list_programs(&system, false)
        .into_iter()
        .find(|p| p.name == "Contoso Notes")
        .unwrap();
    let names = |program: &Program| Identity::of_entry(&system, &folders, program).all_names();
    let cases = [
        (r"NotesApp", true),
        (r"Contoso\NotesApp", true),
        (r"Contoso Notes\1.2.3", false),
        (r"Java\jdk-21", false),
    ];
    for (folder, named) in cases {
        program.install_location = Some(folders.program_files.join(folder).display().to_string());
        let names = names(&program);
        let last = name_key(folder.rsplit('\\').next().unwrap());
        assert_eq!(names.contains(&last), named, "{folder}: {names:?}");
    }
}

#[test]
fn references_need_a_path_inside_the_install_dir() {
    let identity = Identity::named(
        "Acme",
        None,
        vec![PathBuf::from(r"C:\Program Files\Acme\Widget")],
    );
    assert!(identity.references(r#""C:\Program Files\Acme\Widget\widget.exe" --tray"#));
    assert!(!identity.references(r"C:\Program Files\Acme\WidgetPro\widget.exe"));
    assert!(!identity.references("widget.exe"));
}

#[test]
fn shared_places_are_not_a_programs_own_folder() {
    let machine = crate::scan::tests::machine();
    let folders = &machine.folders;
    let root = folders.windows.ancestors().last().unwrap().to_path_buf();
    for dir in [
        folders.windows.clone(),
        folders.windows.join("System32"),
        root,
        folders.program_files.clone(),
        folders.users.clone(),
        folders.local_app_data.join("Programs"),
        folders.desktop.clone(),
    ] {
        assert!(!Identity::is_own_folder(&dir, folders), "{dir:?}");
    }
    for dir in [
        folders.program_files.join(r"Acme\Widget"),
        folders.local_app_data.join("Acme"),
        folders.program_files.join("Acme").join(""),
    ] {
        assert!(Identity::is_own_folder(&dir, folders), "{dir:?}");
    }
}
