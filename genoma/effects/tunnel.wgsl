// Generato da tools/effects: si modifica source/tunnel.wgsli o source/common.wgsli.
// Comune a tutti gli effetti 2D (incollato in testa a ciascuno da
// tools/effects: WGSL non ha include).
//
// values: 0..63 il canale sinistro, 64..127 il destro (basse → alte),
// 128..130 il colore A, 131..133 il colore B, 134 il gain, 135 il picco
// dopo il gain.

const PI: f32 = 3.14159265;

fn band(i: i32) -> f32 {
    return clamp(value(clamp(i, 0, 127)) * value(134), 0.0, 1.0);
}
fn col_a() -> vec3<f32> { return vec3<f32>(value(128), value(129), value(130)); }
fn col_b() -> vec3<f32> { return vec3<f32>(value(131), value(132), value(133)); }
fn peak() -> f32 { return value(135); }

fn catmull(a: f32, b: f32, c: f32, d: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    return 0.5 * ((2.0 * b) + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3);
}

// La sequenza speculare della linea: il sinistro dalle alte al centro,
// poi il destro dal centro alle alte (j 0..127).
fn mirrored(j: i32) -> f32 {
    let k = clamp(j, 0, 127);
    if (k <= 63) {
        return band(63 - k);
    }
    return band(k);
}

// L'altezza della linea (0..1) a x in [-1, 1], spline di Catmull-Rom.
fn line_curve(x: f32) -> f32 {
    var j0: i32;
    var t: f32;
    if (x < 0.0) {
        let u = clamp((x + 1.0) * 63.0, 0.0, 62.9999);
        j0 = i32(floor(u));
        t = u - floor(u);
    } else {
        let u = clamp(x * 63.0, 0.0, 62.9999);
        j0 = 64 + i32(floor(u));
        t = u - floor(u);
    }
    let h = catmull(mirrored(j0 - 1), mirrored(j0), mirrored(j0 + 1), mirrored(j0 + 2), t);
    return clamp(h, 0.0, 1.0);
}

// L'anello chiuso del radiale (n 0..127, ciclico): il destro dall'alto in
// basso, poi il sinistro dal basso in alto.
fn ring_point(n: i32) -> f32 {
    let k = ((n % 128) + 128) % 128;
    if (k <= 63) {
        return band(64 + k);
    }
    return band(127 - k);
}

const RADIAL_INNER: f32 = 0.28;
const RADIAL_SPAN: f32 = 0.62;

// Il raggio della curva radiale all'angolo theta (radianti, atan2).
fn ring_radius(theta: f32) -> f32 {
    var n: f32;
    if (cos(theta) >= 0.0) {
        n = clamp((PI * 0.5 - theta) / PI * 63.0, 0.0, 63.0);
    } else {
        var tl = theta;
        if (tl < 0.0) {
            tl = tl + 2.0 * PI;
        }
        let v = clamp((tl - PI * 0.5) / PI * 63.0, 0.0, 63.0);
        n = 127.0 - v;
    }
    let k = i32(floor(n));
    let t = n - floor(n);
    let h = clamp(catmull(ring_point(k - 1), ring_point(k), ring_point(k + 1), ring_point(k + 2), t), 0.0, 1.0);
    return RADIAL_INNER + h * RADIAL_SPAN;
}

// Un punto in coordinate del radiale: centro dell'area, unità = metà
// altezza, y verso l'alto (come in Sinestesia: cerchi rotondi).
fn polar_q(p: vec2<f32>) -> vec2<f32> {
    let s = shader.size;
    return vec2<f32>((p.x - s.x * 0.5) / (s.y * 0.5), (s.y * 0.5 - p.y) / (s.y * 0.5));
}

// La distanza di p da un segmento.
fn seg_dist(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let ab = b - a;
    let t = clamp(dot(p - a, ab) / max(dot(ab, ab), 1e-6), 0.0, 1.0);
    return length(p - (a + ab * t));
}

// Un punto della linea, in pixel dall'angolo dell'area.
fn line_px(xn: f32) -> vec2<f32> {
    let s = shader.size;
    return vec2<f32>((xn + 1.0) * 0.5 * s.x, (1.0 - line_curve(xn)) * s.y);
}

// La distanza in pixel di p dalla linea: la polilinea campionata ogni 2 px
// attorno al pixel, fin dove l'alone arriva.
fn line_distance(p: vec2<f32>, reach: f32) -> f32 {
    let s = shader.size;
    let step = 2.0;
    let n = i32(ceil(reach / step)) + 1;
    let xn = p.x / s.x * 2.0 - 1.0;
    let dx = step / s.x * 2.0;
    var prev = line_px(xn - f32(n) * dx);
    var best = 1e9;
    for (var k = -n + 1; k <= n; k = k + 1) {
        let cur = line_px(xn + f32(k) * dx);
        best = min(best, seg_dist(p, prev, cur));
        prev = cur;
    }
    return best;
}

// Un punto della curva radiale, in pixel.
fn ring_px(theta: f32) -> vec2<f32> {
    let s = shader.size;
    let r = ring_radius(theta) * s.y * 0.5;
    return s * 0.5 + vec2<f32>(cos(theta), -sin(theta)) * r;
}

// La distanza in pixel di p dalla curva radiale, campionata lungo l'angolo.
fn ring_distance(p: vec2<f32>, reach: f32) -> f32 {
    let s = shader.size;
    let rel = p - s * 0.5;
    let theta = atan2(-rel.y, rel.x);
    let r_px = max(length(rel), 1.0);
    let step = 2.0 / r_px;
    let n = min(i32(ceil(reach / 2.0)) + 1, 48);
    var prev = ring_px(theta - f32(n) * step);
    var best = 1e9;
    for (var k = -n + 1; k <= n; k = k + 1) {
        let cur = ring_px(theta + f32(k) * step);
        best = min(best, seg_dist(p, prev, cur));
        prev = cur;
    }
    return best;
}

// L'alone del bordo "neon" in pixel: NEON_HALF_WIDTH di Sinestesia, in
// unità di metà altezza.
fn neon_reach() -> f32 {
    return 0.024 * shader.size.y * 0.5;
}

// Il bordo "neon": alone (additivo) e linea da 1 px, data la distanza
// dalla curva in pixel.
fn neon(rgb_in: vec3<f32>, alpha_in: f32, d_px: f32) -> vec4<f32> {
    let vis = peak();
    let reach = neon_reach();
    var rgb = rgb_in;
    var a = alpha_in;
    if (d_px < reach) {
        let halo = 1.0 - d_px / reach;
        rgb = rgb + col_a() * vis * halo * halo * 0.7;
    }
    let line = vis * clamp(1.0 - d_px, 0.0, 1.0);
    rgb = rgb * (1.0 - line) + col_a() * line;
    a = a + line * (1.0 - a);
    return vec4<f32>(rgb, max(a, max(rgb.r, max(rgb.g, rgb.b))));
}

// Tunnel: anelli che congelano la sagoma dello spettro e si espandono
// verso chi guarda, ciascuno ruotato di quanto ha vissuto (vortice).
//
// values (diversi dagli effetti 2D, oltre ai colori comuni a 128..133):
// 0 quanti anelli; da 140, 35 per anello: scala, angolo, tinta, poi le 64
// altezze della sagoma impaccate due a valore (a + b·256, 0..255).

const RING_DEATH: f32 = 1.85;
const RING_HALF_WIDTH: f32 = 0.012;
const RING_BASE: i32 = 140;
const RING_STRIDE: i32 = 35;

// L'altezza n (0..63, ciclica) della sagoma dell'anello k.
fn ring_height(k: i32, n: i32) -> f32 {
    let m = ((n % 64) + 64) % 64;
    let packed = value(RING_BASE + k * RING_STRIDE + 3 + m / 2);
    let lo = packed - floor(packed / 256.0) * 256.0;
    let hi = floor(packed / 256.0);
    if (m % 2 == 0) {
        return lo / 255.0;
    }
    return hi / 255.0;
}

// Il raggio della sagoma (raggio ~1) all'angolo phi, nel riferimento
// dell'anello: il destro dall'alto in basso, il sinistro risale.
fn shape_radius(k: i32, phi: f32) -> f32 {
    var n: f32;
    if (cos(phi) >= 0.0) {
        n = clamp((PI * 0.5 - phi) / PI * 31.5, 0.0, 31.5);
    } else {
        var pl = phi;
        if (pl < 0.0) {
            pl = pl + 2.0 * PI;
        }
        let v = clamp((pl - PI * 0.5) / PI * 31.5, 0.0, 31.5);
        n = 63.0 - v;
    }
    let i = i32(floor(n));
    let t = n - floor(n);
    let h = clamp(catmull(ring_height(k, i - 1), ring_height(k, i), ring_height(k, i + 1), ring_height(k, i + 2), t), 0.0, 1.0);
    return 0.72 + h * 0.55;
}

// Appare dal punto di fuga, piena per la traversata, sfuma uscendo.
fn ring_brightness(scale: f32) -> f32 {
    return clamp(scale / 0.28, 0.0, 1.0) * clamp((RING_DEATH - scale) / 0.75, 0.0, 1.0);
}

fn shade(p: vec2<f32>) -> vec4<f32> {
    let q = polar_q(p);
    let rho = length(q);
    let theta = atan2(q.y, q.x);
    let count = i32(value(0));
    var rgb = vec3<f32>(0.0);
    for (var k = 0; k < count; k = k + 1) {
        let base = RING_BASE + k * RING_STRIDE;
        let scale = value(base);
        let angle = value(base + 1);
        let tint = value(base + 2);
        let bright = ring_brightness(scale);
        let w = RING_HALF_WIDTH * (0.35 + scale);
        // Fuori dalla fascia che la sagoma può toccare: niente da fare.
        if (bright <= 0.002 || rho < scale * 0.72 - w || rho > scale * 1.27 + w) {
            continue;
        }
        let phi = theta - angle;
        let r = scale * shape_radius(k, phi);
        let e = 0.01;
        let dr = scale * (shape_radius(k, phi + e) - shape_radius(k, phi - e)) / (2.0 * e);
        let d = abs(rho - r) / sqrt(1.0 + (dr / max(rho, 1e-3)) * (dr / max(rho, 1e-3))) / w;
        if (d >= 1.0) {
            continue;
        }
        // Come lo shader del tunnel di Sinestesia: nucleo netto e alone.
        let core = smoothstep(0.30, 0.0, d);
        let halo = (1.0 - d) * (1.0 - d) * (1.0 - d) * 0.45;
        rgb = rgb + mix(col_a(), col_b(), clamp(tint, 0.0, 1.0)) * bright * (core + halo);
    }
    return vec4<f32>(rgb, clamp(max(rgb.r, max(rgb.g, rgb.b)), 0.0, 1.0));
}
