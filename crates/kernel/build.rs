fn main() {
    println!("cargo:rerun-if-changed=../regis-preview/assets/Mono.ttf");
    if std::env::var_os("CARGO_FEATURE_GRAPHICS").is_none() {
        return;
    }
    let font = fontdue::Font::from_bytes(
        include_bytes!("../regis-preview/assets/Mono.ttf") as &[u8],
        fontdue::FontSettings::default(),
    )
    .unwrap();
    let mut atlas = vec![0u8; 128 * 8 * 14];
    for c in 32u8..127 {
        let (m, bitmap) = font.rasterize(c as char, 12.0);
        for y in 0..m.height {
            for x in 0..m.width {
                let dx = x as i32 + m.xmin;
                let dy = 11 - m.ymin - m.height as i32 + y as i32;
                if (0..8).contains(&dx) && (0..14).contains(&dy) {
                    atlas[c as usize * 112 + dy as usize * 8 + dx as usize] =
                        bitmap[y * m.width + x];
                }
            }
        }
    }
    std::fs::write(
        std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("font.bin"),
        atlas,
    )
    .unwrap();
}
