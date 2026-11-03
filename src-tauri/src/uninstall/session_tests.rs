use super::restore_point_name;

#[test]
fn a_batch_restore_point_says_it_covers_the_batch() {
    assert_eq!(
        restore_point_name("Notepad++", 1),
        "Before uninstalling Notepad++"
    );
    assert_eq!(
        restore_point_name("Notepad++", 2),
        "Before uninstalling Notepad++ and 1 more program"
    );
    assert_eq!(
        restore_point_name("Notepad++", 4),
        "Before uninstalling Notepad++ and 3 more programs"
    );
}
