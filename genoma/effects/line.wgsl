// Generato da tools/effects: si modifica source/line.wgsli o source/common.wgsli.
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

// Linea: l'area sotto la curva speculare, base trasparente, cima opaca.
fn shade(p: vec2<f32>) -> vec4<f32> {
    let s = shader.size;
    let x = p.x / s.x * 2.0 - 1.0;
    let y = 1.0 - p.y / s.y;
    let h = line_curve(x);
    if (h <= 0.0) {
        return vec4<f32>(0.0);
    }
    let cover = clamp((h - y) * s.y + 0.5, 0.0, 1.0);
    let t = clamp(y / h, 0.0, 1.0);
    let a = t * cover;
    return vec4<f32>(mix(col_a(), col_b(), t) * a, a);
}
