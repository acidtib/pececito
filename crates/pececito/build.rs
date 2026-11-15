use std::env;
use std::io;

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=../../assets/icon/pececito.ico");
    // Build scripts run on the host, so check the target, not `cfg!(windows)`.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("../../assets/icon/pececito.ico")
            .compile()?;
    }
    Ok(())
}
