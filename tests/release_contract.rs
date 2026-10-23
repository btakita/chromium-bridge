const MAKEFILE: &str = include_str!("../Makefile");

#[test]
fn release_and_publish_targets_are_check_gated() {
    for required in [
        "release: check",
        "\tcargo build --release",
        "publish: check",
        "\tcargo publish",
    ] {
        assert!(
            MAKEFILE.contains(required),
            "Makefile is missing release contract: {required}"
        );
    }
}
