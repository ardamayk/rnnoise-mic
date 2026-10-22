//! App icon drawn at runtime (no image assets): a microphone on a coloured disc
//! whose colour shows the state. `examples/make_icon.rs` renders the same art
//! into `assets/rnnoise-mic.ico` for the executable and installer.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum State {
    Active,
    Bypassed,
    Error,
}

/// Tray icon size; the art is designed on a 32x32 grid and scales to any size.
const SIZE: u32 = 32;

/// Signed distance from point `p` to a rounded vertical segment (capsule).
fn capsule(p: (f32, f32), x: f32, y0: f32, y1: f32, r: f32) -> f32 {
    let y = p.1.clamp(y0, y1);
    ((p.0 - x).powi(2) + (p.1 - y).powi(2)).sqrt() - r
}

pub fn rgba(state: State) -> (Vec<u8>, u32, u32) {
    (rgba_sized(state, SIZE), SIZE, SIZE)
}

/// Straight (non-premultiplied) RGBA, row-major, top row first.
pub fn rgba_sized(state: State, size: u32) -> Vec<u8> {
    let bg = match state {
        State::Active => [0x1f, 0xa8, 0x5a],
        State::Bypassed => [0x80, 0x86, 0x8f],
        State::Error => [0xd6, 0x3c, 0x3c],
    };
    let scale = size as f32 / 32.0;
    let c = 16.0;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            // Work in the 32-unit design grid; `px` converts distances back to pixels.
            let p = ((x as f32 + 0.5) / scale, (y as f32 + 0.5) / scale);
            let px = |d: f32| d * scale;
            let disc = ((p.0 - c).powi(2) + (p.1 - c).powi(2)).sqrt() - 15.5;
            // Microphone: capsule head, U-shaped holder, stem and base.
            let head = capsule(p, c, 9.0, 15.0, 4.0);
            let ring = (((p.0 - c).powi(2) + (p.1 - 15.0).powi(2)).sqrt() - 7.0).abs() - 1.2;
            let holder = if p.1 >= 15.0 { ring } else { f32::MAX };
            let stem = capsule(p, c, 22.0, 24.5, 1.2);
            let base = if (p.0 - c).abs() <= 5.0 { (p.1 - 25.0).abs() - 1.2 } else { f32::MAX };
            let mic = head.min(holder).min(stem).min(base);

            let a_disc = (0.5 - px(disc)).clamp(0.0, 1.0);
            let a_mic = (0.5 - px(mic)).clamp(0.0, 1.0);
            for i in 0..3 {
                out.push((bg[i] as f32 * (1.0 - a_mic) + 255.0 * a_mic) as u8);
            }
            out.push((a_disc * 255.0) as u8);
        }
    }
    out
}
