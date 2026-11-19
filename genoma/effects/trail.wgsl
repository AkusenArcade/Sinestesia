// La Scia: il quadro di prima, sbiadito, sotto quello nuovo. value(0) è
// quanto ne resta a ogni passo (1 − il velo nero di Sinestesia).
fn shade(p: vec2<f32>) -> vec4<f32> {
    let now = g_backdrop(p);
    let before = g_previous(p) * value(0);
    return now + before * (1.0 - now.a);
}
