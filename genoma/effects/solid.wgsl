// Generato da tools/effects: si modifica source/solid.wgsli o source/common.wgsli.
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

// Poliedro: un icosaedro geodetico (162 vertici, 320 facce) gonfiato dallo
// spettro (la latitudine dà la frequenza, polo nord le basse; l'emisfero
// il canale), che gira e si avvicina sui bassi. Spigoli come tubi di luce,
// facce che si accendono con la loro banda e si staccano sui colpi, vertici
// come punti luminosi; la profondità li spegne verso il fondo.
//
// La simulazione: gli agenti 0..161 sono i vertici proiettati (at: lo
// schermo; velocity: prospettiva e ampiezza; data: la posizione ruotata e
// t), gli agenti 162..481 le facce (at, velocity, data.xy: i tre angoli
// sullo schermo; data.z la luce; data.w il distacco, che resta da un
// passo all'altro).
//
// values: 135 rotazione, 136 inclinazione, 137 distanza, 138 colpo, 139
// silenzio; da 140 le direzioni dei vertici (x, y, z); da 626 le facce
// (a + b·256 + c·65536).

const VERTS: i32 = 162;
const FACES: i32 = 320;
const R0: f32 = 0.56;
const AMP: f32 = 0.40;
const FOCAL: f32 = 2.2;
const DIST: f32 = 3.4;
const EDGE_W: f32 = 0.009;
const FACE_GAIN: f32 = 0.11;

fn dir(i: i32) -> vec3<f32> {
    return vec3<f32>(value(140 + i * 3), value(141 + i * 3), value(142 + i * 3));
}

fn face(f: i32) -> vec3<i32> {
    let packed = value(626 + f);
    let c = floor(packed / 65536.0);
    let rest = packed - c * 65536.0;
    let b = floor(rest / 256.0);
    let a = rest - b * 256.0;
    return vec3<i32>(i32(a), i32(b), i32(c));
}

fn sample_band(right: bool, t: f32) -> f32 {
    let f = clamp(t, 0.0, 1.0) * 63.0;
    let i = i32(floor(f));
    let j = min(i + 1, 63);
    var base = 0;
    if (right) {
        base = 64;
    }
    let a = band(base + i);
    let b = band(base + j);
    return a + (b - a) * (f - floor(f));
}

fn rot(p: vec3<f32>) -> vec3<f32> {
    let yaw = value(135);
    let pitch = value(136);
    let x1 = p.x * cos(yaw) + p.z * sin(yaw);
    let z1 = -p.x * sin(yaw) + p.z * cos(yaw);
    return vec3<f32>(x1, p.y * cos(pitch) - z1 * sin(pitch), p.y * sin(pitch) + z1 * cos(pitch));
}

fn persp(z: f32) -> f32 {
    return FOCAL / max(value(137) + z, 0.4);
}

fn on_screen(p: vec3<f32>, s: f32) -> vec2<f32> {
    let size = shader.size;
    return vec2<f32>(size.x * 0.5 + p.x * s * size.y * 0.5, size.y * 0.5 - p.y * s * size.y * 0.5);
}

// Il fondo si spegne: esponente 3, tetto basso, perché fronte e retro si
// separino e il centro denso non saturi.
fn depth_fog(s: f32) -> f32 {
    let k = s / (FOCAL / DIST);
    return clamp(k * k * k, 0.0, 1.25);
}

fn agent(i: i32, me: Agent) -> Agent {
    var a = me;
    if (i < VERTS) {
        let v = dir(i);
        let t = acos(clamp(v.y, -1.0, 1.0)) / PI;
        let amp = sample_band(v.x >= 0.0, t);
        let p = rot(v * (R0 + amp * AMP));
        let s = persp(p.z);
        a.at = on_screen(p, s);
        a.velocity = vec2<f32>(s, amp);
        a.data = vec4<f32>(p, t);
        return a;
    }
    let f = i - VERTS;
    let idx = face(f);
    let va = g_agent(idx.x);
    let vb = g_agent(idx.y);
    let vc = g_agent(idx.z);
    // Su una sfera la normale è la direzione del baricentro.
    let n = normalize(va.data.xyz + vb.data.xyz + vc.data.xyz);
    let ft = (va.data.w + vb.data.w + vc.data.w) / 3.0;
    let right = (dir(idx.x).x + dir(idx.y).x + dir(idx.z).x) >= 0.0;
    var pop = me.data.w;
    if (sim.frame == 0u) {
        pop = 0.0;
    }
    pop = min(pop * 0.88 + value(138) * sample_band(right, ft) * 0.30 * value(139), 0.5);
    let qa = va.data.xyz + n * pop;
    let qb = vb.data.xyz + n * pop;
    let qc = vc.data.xyz + n * pop;
    let sa = persp(qa.z);
    let sb = persp(qb.z);
    let sc = persp(qc.z);
    let amp = (va.velocity.y + vb.velocity.y + vc.velocity.y) / 3.0;
    let bright = depth_fog((sa + sb + sc) / 3.0) * (amp * 0.9 + pop * 1.2) * FACE_GAIN;
    a.at = on_screen(qa, sa);
    a.velocity = on_screen(qb, sb);
    a.data = vec4<f32>(on_screen(qc, sc), bright, pop);
    return a;
}

fn cross2(a: vec2<f32>, b: vec2<f32>) -> f32 {
    return a.x * b.y - a.y * b.x;
}

fn shade(p: vec2<f32>) -> vec4<f32> {
    let s0 = FOCAL / DIST;
    let unit = shader.size.y * 0.5;
    var rgb = vec3<f32>(0.0);
    for (var f = 0; f < FACES; f = f + 1) {
        let idx = face(f);
        // Le facce: dentro il triangolo, la loro luce.
        let fa = g_agent(VERTS + f);
        let bright = fa.data.z;
        let A = fa.at;
        let B = fa.velocity;
        let C = fa.data.xy;
        if (bright > 0.0005
            && p.x >= min(A.x, min(B.x, C.x)) && p.x <= max(A.x, max(B.x, C.x))
            && p.y >= min(A.y, min(B.y, C.y)) && p.y <= max(A.y, max(B.y, C.y))) {
            let w0 = cross2(B - A, p - A);
            let w1 = cross2(C - B, p - B);
            let w2 = cross2(A - C, p - C);
            if ((w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0) || (w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0)) {
                let ft = (g_agent(idx.x).data.w + g_agent(idx.y).data.w + g_agent(idx.z).data.w) / 3.0;
                rgb = rgb + mix(col_a(), col_b(), ft) * bright;
            }
        }
        // Gli spigoli: ciascuno una volta, da a < b.
        for (var e = 0; e < 3; e = e + 1) {
            var ia = idx.x;
            var ib = idx.y;
            if (e == 1) {
                ia = idx.y;
                ib = idx.z;
            } else if (e == 2) {
                ia = idx.z;
                ib = idx.x;
            }
            if (ia > ib) {
                continue;
            }
            let va = g_agent(ia);
            let vb = g_agent(ib);
            let s_avg = (va.velocity.x + vb.velocity.x) * 0.5;
            let w = EDGE_W * (s_avg / s0) * unit;
            let lo = min(va.at, vb.at) - vec2<f32>(w);
            let hi = max(va.at, vb.at) + vec2<f32>(w);
            if (p.x < lo.x || p.y < lo.y || p.x > hi.x || p.y > hi.y) {
                continue;
            }
            let d = seg_dist(p, va.at, vb.at) / max(w, 0.5);
            if (d >= 1.0) {
                continue;
            }
            let amp = (va.velocity.y + vb.velocity.y) * 0.5;
            let light = depth_fog(s_avg) * (0.18 + amp * 0.55);
            let core = smoothstep(0.30, 0.0, d);
            let halo = (1.0 - d) * (1.0 - d) * (1.0 - d) * 0.45;
            rgb = rgb + mix(col_a(), col_b(), (va.data.w + vb.data.w) * 0.5) * light * (core + halo);
        }
    }
    // I vertici: punti luminosi di 8 px.
    for (var i = 0; i < VERTS; i = i + 1) {
        let v = g_agent(i);
        let d = length(p - v.at) / 4.0;
        if (d >= 1.0) {
            continue;
        }
        let a = clamp((0.06 + v.velocity.y * 0.5) * depth_fog(v.velocity.x), 0.0, 1.0);
        rgb = rgb + mix(col_a(), col_b(), v.data.w) * a * smoothstep(1.0, 0.0, d) * 1.5;
    }
    return vec4<f32>(rgb, clamp(max(rgb.r, max(rgb.g, rgb.b)), 0.0, 1.0));
}
