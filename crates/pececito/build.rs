use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::Path;

const ITEM_ICONS: &str = "../../assets/item-icons";

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=../../assets/icon/pececito.ico");
    // Build scripts run on the host, so check the target, not `cfg!(windows)`.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("../../assets/icon/pececito.ico")
            .compile()?;
    }
    write_item_icons()
}

/// Builds every icon in assets/item-icons into the app, by icon name.
fn write_item_icons() -> io::Result<()> {
    println!("cargo:rerun-if-changed={ITEM_ICONS}");
    let folder = fs::canonicalize(ITEM_ICONS)?;
    let mut icons = Vec::new();
    for entry in fs::read_dir(&folder)? {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "jpg")
            && let Some(name) = path.file_stem().and_then(|stem| stem.to_str())
        {
            icons.push((name.to_string(), path.clone()));
        }
    }
    icons.sort();
    let mut source = String::from("pub static ITEM_ICONS: &[(&str, &[u8])] = &[\n");
    for (name, path) in &icons {
        writeln!(
            source,
            "    ({name:?}, include_bytes!({:?})),",
            path.display()
        )
        .map_err(io::Error::other)?;
    }
    source.push_str("];\n");
    let out = env::var("OUT_DIR").map_err(io::Error::other)?;
    fs::write(Path::new(&out).join("item_icons.rs"), source)
}
