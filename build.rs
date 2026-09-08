fn main() {
    // Emit this on every target; otherwise Cargo watches the whole workspace,
    // including DMG staging symlinks into /Applications during cross-checks.
    println!("cargo:rerun-if-changed=src/macos_dock.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/macos_dock.m")
            .flag("-fobjc-arc")
            .compile("codex_account_hub_macos");
        println!("cargo:rustc-link-lib=framework=AppKit");
    }
}
