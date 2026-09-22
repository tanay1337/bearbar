use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/icons.gresource.xml");
    println!("cargo:rerun-if-changed=assets/icons");

    let target =
        PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set")).join("bearbar.gresource");
    let status = Command::new("glib-compile-resources")
        .args([
            "assets/icons.gresource.xml",
            "--sourcedir=assets",
            "--target",
        ])
        .arg(target)
        .status()
        .expect("glib-compile-resources is required to build Bearbar");
    assert!(status.success(), "failed to compile Bearbar icon resources");
}
