// Embeds the game's icon.ico (in the repo root) into the launcher exe.
//
// On Linux the launcher installs icon.png, the largest picture in icon.ico, for
// the game's app menu entry. This writes it to OUT_DIR.
fn main() {
    println!("cargo:rerun-if-changed=../icon.ico");
    let ico = std::fs::read("../icon.ico").expect("could not read icon.ico");
    let png = largest_png(&ico).expect("icon.ico has no PNG picture");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(out.join("icon.png"), png).expect("could not write icon.png");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../icon.ico");
        res.compile().expect("could not embed icon.ico");
    }
}

/// The biggest of the PNG pictures in an .ico file. An .ico starts with a
/// 6-byte header and then one 16-byte entry per picture, each with its width
/// (0 meaning 256), and the size and offset of its data.
fn largest_png(ico: &[u8]) -> Option<&[u8]> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(ico.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(ico.get(at..at + 4)?.try_into().ok()?) as usize);
    let count = u16_at(4)? as usize;
    (0..count)
        .filter_map(|i| {
            let entry = 6 + 16 * i;
            let width = match *ico.get(entry)? {
                0 => 256,
                width => width as u32,
            };
            let (size, offset) = (u32_at(entry + 8)?, u32_at(entry + 12)?);
            let data = ico.get(offset..offset.checked_add(size)?)?;
            data.starts_with(b"\x89PNG\r\n\x1a\n").then_some((width, data))
        })
        .max_by_key(|(width, _)| *width)
        .map(|(_, data)| data)
}
