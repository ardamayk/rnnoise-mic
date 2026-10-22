//! Regenerate `assets/rnnoise-mic.ico` from the tray icon art:
//! `cargo run --example make_icon`

#[path = "../src/icon.rs"]
#[allow(dead_code)]
mod icon;

const SIZES: [u32; 5] = [16, 24, 32, 48, 256];

/// One ICO image: 32-bit BITMAPINFOHEADER DIB, bottom-up BGRA plus an empty AND mask.
fn dib(size: u32) -> Vec<u8> {
    let rgba = icon::rgba_sized(icon::State::Active, size);
    let mut d = Vec::new();
    for v in [40u32, size, size * 2] {
        d.extend(v.to_le_bytes());
    }
    d.extend(1u16.to_le_bytes()); // planes
    d.extend(32u16.to_le_bytes()); // bits per pixel
    d.extend([0u8; 24]); // no compression, sizes/resolution/palette unused
    for row in (0..size).rev() {
        for px in rgba[(row * size * 4) as usize..((row + 1) * size * 4) as usize].chunks_exact(4) {
            d.extend([px[2], px[1], px[0], px[3]]);
        }
    }
    let mask_row = (size as usize).div_ceil(32) * 4;
    d.extend(vec![0u8; mask_row * size as usize]);
    d
}

fn main() -> std::io::Result<()> {
    let images: Vec<Vec<u8>> = SIZES.iter().map(|&s| dib(s)).collect();
    let mut ico = Vec::new();
    ico.extend([0u16, 1, SIZES.len() as u16].iter().flat_map(|v| v.to_le_bytes()));
    let mut offset = 6 + 16 * SIZES.len() as u32;
    for (&s, img) in SIZES.iter().zip(&images) {
        let dim = if s >= 256 { 0 } else { s as u8 };
        ico.extend([dim, dim, 0, 0]);
        ico.extend(1u16.to_le_bytes());
        ico.extend(32u16.to_le_bytes());
        ico.extend((img.len() as u32).to_le_bytes());
        ico.extend(offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in &images {
        ico.extend(img);
    }
    std::fs::create_dir_all("assets")?;
    std::fs::write("assets/rnnoise-mic.ico", &ico)?;
    println!("wrote assets/rnnoise-mic.ico ({} bytes)", ico.len());
    Ok(())
}
