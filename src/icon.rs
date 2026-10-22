//! Tray icon drawn at runtime (no image assets): a microphone on a coloured disc
//! whose colour shows the state.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum State {
    Active,
    Bypassed,
    Error,
}

const SIZE: u32 = 32;

/// Signed distance from point `p` to a rounded vertical segment (capsule).
fn capsule(p: (f32, f32), x: f32, y0: f32, y1: f32, r: f32) -> f32 {
    let y = p.1.clamp(y0, y1);
    ((p.0 - x).powi(2) + (p.1 - y).powi(2)).sqrt() - r
}

pub fn rgba(state: State) -> (Vec<u8>, u32, u32) {
    let bg = match state {
        State::Active => [0x1f, 0xa8, 0x5a],
        State::Bypassed => [0x80, 0x86, 0x8f],
        State::Error => [0xd6, 0x3c, 0x3c],
    };
    let c = SIZE as f32 / 2.0;
    let mut out = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let disc = ((p.0 - c).powi(2) + (p.1 - c).powi(2)).sqrt() - (c - 0.5);
            // Microphone: capsule head, U-shaped holder, stem and base.
            let head = capsule(p, c, 9.0, 15.0, 4.0);
            let ring = (((p.0 - c).powi(2) + (p.1 - 15.0).powi(2)).sqrt() - 7.0).abs() - 1.2;
            let holder = if p.1 >= 15.0 { ring } else { f32::MAX };
            let stem = capsule(p, c, 22.0, 24.5, 1.2);
            let base = if (p.0 - c).abs() <= 5.0 { (p.1 - 25.0).abs() - 1.2 } else { f32::MAX };
            let mic = head.min(holder).min(stem).min(base);

            let a_disc = (0.5 - disc).clamp(0.0, 1.0);
            let a_mic = (0.5 - mic).clamp(0.0, 1.0);
            for i in 0..3 {
                out.push((bg[i] as f32 * (1.0 - a_mic) + 255.0 * a_mic) as u8);
            }
            out.push((a_disc * 255.0) as u8);
        }
    }
    (out, SIZE, SIZE)
}
