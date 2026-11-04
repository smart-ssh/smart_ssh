// Throwaway (issue #901): fails on the first attempt, passes on retry.
#[test]
fn passes_only_on_retry() {
    let marker = std::env::temp_dir().join("smart_ssh_flaky_demo_marker");
    if marker.exists() {
        return;
    }
    std::fs::write(&marker, b"x").unwrap();
    panic!("first attempt fails on purpose");
}
