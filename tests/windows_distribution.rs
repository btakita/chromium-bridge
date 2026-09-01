const RELEASE_WORKFLOW: &str = include_str!("../.github/workflows/release.yml");
const WINDOWS_INSTALLER: &str = include_str!("../install.ps1");

#[test]
fn release_workflow_builds_installable_windows_archives() {
    for target in ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"] {
        assert!(
            RELEASE_WORKFLOW.contains(target),
            "release workflow is missing {target}"
        );
    }

    for required in [
        "windows-latest",
        "Compress-Archive",
        "chromium-bridge.exe",
        "chromium-bridge-*.zip",
        "Smoke test Windows package",
    ] {
        assert!(
            RELEASE_WORKFLOW.contains(required),
            "release workflow is missing {required}"
        );
    }
}

#[test]
fn powershell_installer_matches_release_asset_contract() {
    for required in [
        "'X64' { 'x86_64' }",
        "'Arm64' { 'aarch64' }",
        "$targetArchitecture-pc-windows-msvc.zip",
        "Get-FileHash",
        "Expand-Archive",
        "chromium-bridge.exe",
        "SetEnvironmentVariable('Path'",
    ] {
        assert!(
            WINDOWS_INSTALLER.contains(required),
            "Windows installer is missing {required}"
        );
    }
}
