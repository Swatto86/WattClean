use super::*;

fn p(text: &str) -> PathBuf {
    PathBuf::from(text)
}

#[test]
fn keys_ignore_case_separators_and_dots() {
    assert_eq!(
        key(&p(r"C:/Users/Tester/./AppData/x/../Temp/")),
        r"c:\users\tester\appdata\temp"
    );
    assert_eq!(key(&p(r"\\?\C:\Temp")), r"c:\temp");
    assert_eq!(key(&p(r"C:\")), r"c:\");
}

#[test]
fn macos_spellings_of_one_folder_fold_into_one() {
    let boot = vec!["Macintosh HD".to_string()];
    let fold = |text: &str| {
        fold_mac_aliases(text.split('/').filter(|p| !p.is_empty()).collect(), &boot).join("/")
    };
    assert_eq!(
        fold("/System/Volumes/Data/Users/x/Documents"),
        "Users/x/Documents"
    );
    assert_eq!(fold("/System/Volumes/Data"), "");
    assert_eq!(fold("/private/var/folders/ab/T"), "var/folders/ab/T");
    assert_eq!(fold("/System/Volumes/Data/private/tmp/x"), "tmp/x");
    assert_eq!(fold("/Volumes/Macintosh HD/Users/x"), "Users/x");
    assert_eq!(
        fold("/System/Volumes/Data/Volumes/Macintosh HD/Users/x"),
        "Users/x"
    );
    assert_eq!(fold("/.nofollow/Users/x"), "Users/x");
    assert_eq!(fold("/.resolve/1/private/var/x"), "var/x");
    assert_eq!(fold("/.resolve/Users/x"), ".resolve/Users/x");
    assert_eq!(fold("/Users/x/.nofollow"), "Users/x/.nofollow");
    assert_eq!(fold("/Volumes/USB/Users/x"), "Volumes/USB/Users/x");
    assert_eq!(fold("/private/Users/x"), "private/Users/x");
    assert_eq!(
        fold("/Users/x/System/Volumes/Data"),
        "Users/x/System/Volumes/Data"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn the_guard_protects_a_folder_by_any_of_its_macos_spellings() {
    let guard = Guard::new(&[p("/Users/t"), p("/var/folders/ab/T")], &[]);
    assert_eq!(
        guard.check(&p("/System/Volumes/Data/Users/t")),
        Err(Refusal::Protected)
    );
    assert_eq!(
        guard.check(&p("/private/var/folders/ab/T")),
        Err(Refusal::Protected)
    );
    assert_eq!(
        guard.check(&p("/System/Volumes/Data")),
        Err(Refusal::Protected)
    );
    assert_eq!(guard.check(&p("/private/var/folders/ab/T/x.tmp")), Ok(()));
    assert_eq!(
        guard.check(&p("/.nofollow/Users/t")),
        Err(Refusal::Protected)
    );
}

/// Unix has no shares: a path starting `//` is the same path with one.
#[cfg(not(windows))]
#[test]
fn a_doubled_root_separator_is_one_on_unix() {
    assert_eq!(key(&p("//home/t/Keep/")), r"\home\t\keep");
    let guard = Guard::new(&[p("/home/t")], &[p("//home/t/Keep")]);
    assert_eq!(guard.check(&p("///home/t")), Err(Refusal::Protected));
    assert_eq!(guard.check(&p("//home/t/Keep/a.txt")), Err(Refusal::Kept));
    assert_eq!(guard.check(&p("/home/t/Keep/a.txt")), Err(Refusal::Kept));
}

#[test]
fn containment_is_by_whole_component() {
    assert!(is_within(r"c:\temp\a", r"c:\temp"));
    assert!(is_within(r"c:\temp", r"c:\temp"));
    assert!(!is_within(r"c:\temporary", r"c:\temp"));
    assert!(is_within(r"c:\anything", r"c:\"));
}

#[test]
fn the_guard_refuses_roots_known_folders_and_their_parents() {
    let guard = Guard::new(&[p(r"C:\Users\T\AppData\Local\Temp")], &[]);
    assert_eq!(guard.check(&p(r"C:\")), Err(Refusal::Protected));
    assert_eq!(guard.check(&p(r"C:\Windows")), Err(Refusal::Protected));
    assert_eq!(
        guard.check(&p(r"C:\Users\T\AppData\Local\Temp")),
        Err(Refusal::Protected)
    );
    assert_eq!(
        guard.check(&p(r"C:\Users\T\AppData")),
        Err(Refusal::Protected)
    );
    assert_eq!(
        guard.check(&p(r"C:\Users\T\AppData\Local\Temp\x.tmp")),
        Ok(())
    );
}

#[test]
fn the_keep_list_covers_its_contents_and_its_parents() {
    let guard = Guard::new(&[], &[p(r"C:\Users\T\AppData\Local\App\Keep")]);
    assert_eq!(
        guard.check(&p(r"c:\users\t\appdata\local\app\keep\a.txt")),
        Err(Refusal::Kept)
    );
    assert_eq!(
        guard.check(&p(r"C:\Users\T\AppData\Local\App")),
        Err(Refusal::Kept)
    );
    assert_eq!(
        guard.check(&p(r"C:\Users\T\AppData\Local\App\Other")),
        Ok(())
    );
    assert!(guard.is_kept(&p(r"C:\Users\T\AppData\Local\App\Keep\deep")));
    assert!(!guard.is_kept(&p(r"C:\Users\T\AppData\Local\App")));
}
