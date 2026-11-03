use super::*;

#[test]
fn error_texts_fit_the_platform() {
    let text = |code| describe(&std::io::Error::from_raw_os_error(code));
    assert_eq!(text(32), "in use by another program");
    if cfg!(windows) {
        assert!(text(5).contains("administrator"));
    } else {
        assert!(
            !text(5).contains("administrator"),
            "EIO is not access denied"
        );
    }
    if cfg!(target_os = "macos") {
        assert!(text(1).contains("Full Disk Access"));
        assert!(
            !text(13).contains("Full Disk Access"),
            "EACCES is plain permissions"
        );
    }
}
