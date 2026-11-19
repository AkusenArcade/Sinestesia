// Generato da tools/effects: si modifica source/imaging.wgsli o source/common.wgsli.
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

// Imaging: il semicerchio davanti all'ascoltatore, che sta al centro della
// corda. L'arco è il palco stereo: le estremità sono i due diffusori. Tre
// lobi, bassi, medi e alti: in ogni direzione, l'energia che arriva da lì
// (la parte localizzata stretta attorno all'azimut, quella diffusa stesa
// piatta sull'arco). Una luce per banda sull'arco, al suo azimut, che
// sbiadisce quanto la banda è diffusa.
//
// values (diversi dagli effetti 2D): 0..63 l'azimut di ogni banda
// (radianti, ±π/6 i diffusori), 64..127 la diffusività, 128..133 i colori,
// 134 il gain, 140..203 l'energia.

const IMG_RADIUS: f32 = 0.92;
const IMG_Y_OFFSET: f32 = -0.46;
const IMG_LOBE_MIN: f32 = 0.02;
const IMG_RING_W: f32 = 0.006;
const IMG_LOBE_W: f32 = 0.010;
const IMG_DIR_SPREAD: f32 = 0.16;
const IMG_STAGE_SCALE: f32 = 3.0;
const IMG_LOBE_KNEE: f32 = 4.5;
const IMG_LOBE_GAMMA: f32 = 0.45;

fn img_energy(b: i32) -> f32 { return clamp(value(140 + b) * value(134), 0.0, 1.0); }
fn img_diffuse(b: i32) -> f32 { return clamp(value(64 + b), 0.0, 1.0); }
fn img_angle(b: i32) -> f32 { return clamp(value(b) * IMG_STAGE_SCALE, -PI * 0.5, PI * 0.5); }

// La fascia di una banda: 0 sotto 250 Hz, 1 fino a 4 kHz, 2 sopra.
fn img_group(b: i32) -> i32 {
    let f = 30.0 * pow(16000.0 / 30.0, (f32(b) + 0.5) / 64.0);
    if (f < 250.0) {
        return 0;
    }
    if (f < 4000.0) {
        return 1;
    }
    return 2;
}

// L'energia media del lobo g dalla direzione a (disegnata, 0 davanti), e
// quanto è diffusa (media pesata dall'energia).
fn lobe(g: i32, a: f32) -> vec2<f32> {
    let inv2s2 = 1.0 / (2.0 * IMG_DIR_SPREAD * IMG_DIR_SPREAD);
    var sum = 0.0;
    var count = 0.0;
    var diffuse = 0.0;
    var weight = 0.0;
    for (var b = 0; b < 64; b = b + 1) {
        if (img_group(b) != g) {
            continue;
        }
        count = count + 1.0;
        let e = img_energy(b);
        if (e <= 0.01) {
            continue;
        }
        let d = img_diffuse(b);
        let delta = a - img_angle(b);
        sum = sum + e * (1.0 - d) * exp(-delta * delta * inv2s2) + e * d * 0.5;
        diffuse = diffuse + d * e;
        weight = weight + e;
    }
    return vec2<f32>(sum / max(count, 1.0), diffuse / max(weight, 1e-4));
}

fn glow(d: f32) -> f32 {
    return exp(-d * d);
}

// Organico, come le forme di Bioma: nessun arco né tacca disegnati. Il palco
// è un alone appena accennato, i diffusori due luci tenui ai suoi capi,
// l'ascoltatore un nucleo. Ogni lobo è una cellula: un corpo appena
// tinto e una membrana luminosa, nitida quanto il suono è localizzato,
// sfumata quanto è diffuso. Verso la corda tutto svanisce invece di
// essere tagliato. Ogni banda è una spora alla sua direzione, tanto più
// lontana quanto è acuta, grande con l'energia, sfocata con la diffusività.
fn shade(p: vec2<f32>) -> vec4<f32> {
    let q = polar_q(p);
    let rel = q - vec2<f32>(0.0, IMG_Y_OFFSET);
    let rho = length(rel);
    let a = atan2(rel.x, rel.y);
    let unit = shader.size.y * 0.5;
    // La base: svanisce sotto la corda, invece di un taglio.
    let base = smoothstep(-0.06, 0.12, rel.y);
    var rgb = vec3<f32>(0.0);

    // Il palco: un alone tenue che si spegne verso il bordo, e i diffusori.
    let stage = smoothstep(IMG_RADIUS + 0.10, IMG_RADIUS - 0.25, rho) * 0.035;
    rgb = rgb + col_a() * stage * base;
    for (var side = -1; side <= 1; side = side + 2) {
        let at = vec2<f32>(f32(side) * IMG_RADIUS, 0.0);
        rgb = rgb + col_a() * 0.22 * glow(length(rel - at) / 0.035);
    }
    // L'ascoltatore.
    rgb = rgb + col_b() * 0.35 * glow(rho / 0.03);

    // I tre lobi.
    let fa = clamp(a, -PI * 0.5, PI * 0.5);
    for (var g = 0; g < 3; g = g + 1) {
        let c = mix(col_a(), col_b(), f32(g) * 0.5);
        let l = lobe(g, fa);
        let v = 1.0 - exp(-l.x * IMG_LOBE_KNEE);
        if (v <= 0.002) {
            continue;
        }
        let r = IMG_LOBE_MIN + v * (IMG_RADIUS - IMG_LOBE_MIN);
        let bright = pow(v, IMG_LOBE_GAMMA);
        // La membrana: più larga e morbida per il suono diffuso.
        let width = 0.012 + l.y * 0.05;
        let x = rho / max(r, 1e-4);
        let body = smoothstep(1.0 + width / max(r, 0.05), 1.0 - width / max(r, 0.05), x) * (0.04 + 0.10 * x * x);
        let membrane = glow((rho - r) / width) * (0.45 + v * 0.5);
        rgb = rgb + c * bright * (body + membrane) * base;
    }

    // Le spore: una per banda.
    for (var b = 0; b < 64; b = b + 1) {
        let e = img_energy(b);
        if (e <= 0.02) {
            continue;
        }
        let d = img_diffuse(b);
        let ang = img_angle(b);
        let radius = 0.12 + (f32(b) / 63.0) * (IMG_RADIUS - 0.18);
        let at = vec2<f32>(sin(ang), cos(ang)) * radius;
        let size = (0.008 + e * 0.022) * (1.0 + d * 1.5);
        let light = e * (1.0 - d * 0.6) * 0.9;
        rgb = rgb + mix(col_a(), col_b(), f32(b) / 63.0) * light * glow(length(rel - at) / size) * base;
    }
    return vec4<f32>(rgb, clamp(max(rgb.r, max(rgb.g, rgb.b)), 0.0, 1.0));
}
