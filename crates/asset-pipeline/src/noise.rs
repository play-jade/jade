//! Deterministic cross-platform procedural noise and material grain functions.
//!
//! Uses integer hashing (`SplitMix64`) and explicit IEEE-754 arithmetic (`f64`)
//! with no platform-specific RNG state so baked material textures reproduce
//! bit-identically across macOS, Linux, and Windows.

#[inline]
pub fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

#[inline]
pub fn hash2d(ix: i32, iy: i32, seed: u64) -> u64 {
    let ux = (ix as i64 as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93);
    let uy = (iy as i64 as u64).wrapping_mul(0xA5A3_564E_27F8_865B);
    splitmix64(seed ^ ux ^ uy)
}

#[inline]
pub fn hash_unit(ix: i32, iy: i32, seed: u64) -> f64 {
    let h = hash2d(ix, iy, seed);
    ((h >> 11) as f64) * (1.0 / ((1u64 << 53) as f64))
}

#[inline]
fn quintic(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

#[inline]
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Smooth 2D gradient noise in roughly `[-1.0, 1.0]`.
pub fn gradient_noise_2d(x: f64, y: f64, seed: u64) -> f64 {
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = x - (ix as f64);
    let fy = y - (iy as f64);

    let u = quintic(fx);
    let v = quintic(fy);

    let grad_dot = |gx: i32, gy: i32, dx: f64, dy: f64| -> f64 {
        let h = hash2d(gx, gy, seed);
        let angle = (((h >> 16) & 0xFFFF) as f64) * (std::f64::consts::TAU / 65536.0);
        angle.cos() * dx + angle.sin() * dy
    };

    let n00 = grad_dot(ix, iy, fx, fy);
    let n10 = grad_dot(ix + 1, iy, fx - 1.0, fy);
    let n01 = grad_dot(ix, iy + 1, fx, fy - 1.0);
    let n11 = grad_dot(ix + 1, iy + 1, fx - 1.0, fy - 1.0);

    let nx0 = lerp(n00, n10, u);
    let nx1 = lerp(n01, n11, u);
    lerp(nx0, nx1, v) * std::f64::consts::SQRT_2
}

/// Periodic 2D value/gradient noise with integer period `(px, py)` for seamless tileable board textures.
pub fn periodic_noise_2d(x: f64, y: f64, px: i32, py: i32, seed: u64) -> f64 {
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = x - (ix as f64);
    let fy = y - (iy as f64);

    let u = quintic(fx);
    let v = quintic(fy);

    let wrap = |c: i32, p: i32| -> i32 { c.rem_euclid(p) };
    let grad_dot = |gx: i32, gy: i32, dx: f64, dy: f64| -> f64 {
        let h = hash2d(wrap(gx, px), wrap(gy, py), seed);
        let angle = (((h >> 16) & 0xFFFF) as f64) * (std::f64::consts::TAU / 65536.0);
        angle.cos() * dx + angle.sin() * dy
    };

    let n00 = grad_dot(ix, iy, fx, fy);
    let n10 = grad_dot(ix + 1, iy, fx - 1.0, fy);
    let n01 = grad_dot(ix, iy + 1, fx, fy - 1.0);
    let n11 = grad_dot(ix + 1, iy + 1, fx - 1.0, fy - 1.0);

    lerp(lerp(n00, n10, u), lerp(n01, n11, u), v) * std::f64::consts::SQRT_2
}

/// Multi-octave fractional Brownian motion in roughly `[-1.0, 1.0]`.
pub fn fbm_2d(mut x: f64, mut y: f64, octaves: u32, gain: f64, lacunarity: f64, seed: u64) -> f64 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut norm = 0.0;
    for i in 0..octaves {
        let octave_seed = splitmix64(seed ^ (i as u64).wrapping_mul(0x517C_C1B7_2722_0A95));
        sum += amp * gradient_noise_2d(x, y, octave_seed);
        norm += amp;
        amp *= gain;
        x *= lacunarity;
        y *= lacunarity;
    }
    if norm > 0.0 { sum / norm } else { 0.0 }
}

/// Seamless periodic FBM for `256x256` tileable surfaces (`u, v` in `[0, 1)`).
pub fn periodic_fbm_2d(
    u: f64,
    v: f64,
    base_period: i32,
    octaves: u32,
    gain: f64,
    seed: u64,
) -> f64 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut norm = 0.0;
    let mut period = base_period;
    for i in 0..octaves {
        let octave_seed = splitmix64(seed ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let p = period as f64;
        sum += amp * periodic_noise_2d(u * p, v * p, period, period, octave_seed);
        norm += amp;
        amp *= gain;
        period *= 2;
    }
    if norm > 0.0 { sum / norm } else { 0.0 }
}

/// Domain-warped cloudy celadon nephrite density in `[-1.0, 1.0]`.
pub fn nephrite_cloud(nx: f64, ny: f64, seed: u64) -> f64 {
    let wx = fbm_2d(nx * 2.4 + 3.1, ny * 2.4 - 1.7, 4, 0.55, 2.02, seed ^ 0x1111);
    let wy = fbm_2d(nx * 2.4 - 4.3, ny * 2.4 + 2.9, 4, 0.55, 2.02, seed ^ 0x2222);
    let body = fbm_2d(
        nx * 3.1 + 0.45 * wx,
        ny * 3.1 + 0.45 * wy,
        5,
        0.52,
        2.05,
        seed ^ 0x3333,
    );
    let fibrous = 1.0
        - fbm_2d(
            nx * 6.5 - 0.30 * wy,
            ny * 6.5 + 0.30 * wx,
            3,
            0.5,
            2.0,
            seed ^ 0x4444,
        )
        .abs()
            * 1.6;
    (0.76 * body + 0.24 * fibrous).clamp(-1.0, 1.0)
}

/// Directional fine boxwood grain in `[-1.0, 1.0]`.
pub fn boxwood_grain(nx: f64, ny: f64, seed: u64) -> f64 {
    let angle = 0.24 + 0.12 * ((splitmix64(seed) & 0xFF) as f64 / 255.0 - 0.5);
    let ca = angle.cos();
    let sa = angle.sin();
    let rx = nx * ca - ny * sa;
    let ry = nx * sa + ny * ca;

    let warp = fbm_2d(rx * 2.2, ry * 1.4, 3, 0.5, 2.0, seed ^ 0x5555);
    let rings = ((ry * 9.5 + warp * 1.35).sin()) * 0.42;
    let fibers = fbm_2d(rx * 3.5, ry * 28.0, 4, 0.55, 2.0, seed ^ 0x6666) * 0.58;
    (rings + fibers).clamp(-1.0, 1.0)
}

/// Quiet matte limestone / ivory-stone micro-veining and mineral speckle in `[-1.0, 1.0]`.
pub fn ivory_stone_vein(nx: f64, ny: f64, seed: u64) -> f64 {
    let warp = fbm_2d(nx * 2.0, ny * 2.0, 3, 0.5, 2.0, seed ^ 0x7777);
    let soft_cloud = fbm_2d(
        nx * 2.8 + 0.25 * warp,
        ny * 2.8 - 0.25 * warp,
        4,
        0.5,
        2.0,
        seed ^ 0x8888,
    );
    let ridge_raw = fbm_2d(
        nx * 3.6 + 0.4 * warp,
        ny * 3.6 + 0.3 * warp,
        3,
        0.5,
        2.0,
        seed ^ 0x9999,
    )
    .abs();
    let fine_vein = (-ridge_raw * 9.0).exp() * 0.35;
    let pore = fbm_2d(nx * 18.0, ny * 18.0, 2, 0.5, 2.0, seed ^ 0xAAAA) * 0.22;
    (0.65 * soft_cloud - fine_vein + pore).clamp(-1.0, 1.0)
}
