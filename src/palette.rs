//! Los colores de la portada: los dos con más presencia de entre los vivos
//! (y distintos entre sí), para teñir la interfaz con la canción que suena.

use image::DynamicImage;
use ratatui::style::Color;

/// Tono (0..360), saturación y luminosidad (0..1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsl {
    pub h: f64,
    pub s: f64,
    pub l: f64,
}

/// Los dos colores de la portada; None si es gris (o casi).
pub fn of(img: &DynamicImage) -> Option<(Hsl, Hsl)> {
    let small = img.thumbnail(48, 48).to_rgb8();
    // 18 cubos de tono (20° cada uno); cuenta más lo vivo que lo apagado
    let mut buckets = [(0.0f64, 0.0f64, 0.0f64, 0.0f64); 18]; // peso, Σh·w, Σs·w, Σl·w
    let mut total = 0.0;
    for p in small.pixels() {
        let c = hsl(p[0], p[1], p[2]);
        let w = c.s * (1.0 - (2.0 * c.l - 1.0).abs());
        if w < 0.12 {
            continue;
        }
        let b = &mut buckets[((c.h / 20.0) as usize).min(17)];
        b.0 += w;
        b.1 += c.h * w;
        b.2 += c.s * w;
        b.3 += c.l * w;
        total += w;
    }
    if total / (small.width() * small.height()) as f64 <= 0.02 {
        return None;
    }
    let mut found: Vec<(f64, Hsl)> = buckets
        .iter()
        .filter(|b| b.0 > 0.0)
        .map(|b| (b.0, Hsl { h: b.1 / b.0, s: b.2 / b.0, l: b.3 / b.0 }))
        .collect();
    found.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (w1, first) = found[0];
    // el segundo, de otro tono y con algo de presencia; si no hay, uno vecino
    let second = found
        .iter()
        .find(|(w, c)| hue_dist(c.h, first.h) >= 50.0 && *w >= w1 * 0.12)
        .map(|x| x.1)
        .unwrap_or(Hsl { h: (first.h + 40.0) % 360.0, ..first });
    Some((first, second))
}

/// Para que se lea sobre el fondo del tema: vivo, y claro sobre oscuro (u
/// oscuro sobre claro).
pub fn adapt(c: Hsl, dark_bg: bool) -> Color {
    let s = c.s.clamp(0.45, 0.85);
    let l = if dark_bg { c.l.clamp(0.62, 0.75) } else { c.l.clamp(0.30, 0.42) };
    rgb(Hsl { s, l, ..c })
}

/// ¿El fondo es oscuro? Se mira el texto: si es claro, el fondo no.
pub fn dark(fg: Color) -> bool {
    match fg {
        Color::Rgb(r, g, b) => 0.2126 * r as f64 + 0.7152 * g as f64 + 0.0722 * b as f64 > 128.0,
        Color::Black => false,
        _ => true,
    }
}

fn hue_dist(a: f64, b: f64) -> f64 {
    let d = (a - b).abs() % 360.0;
    d.min(360.0 - d)
}

pub fn hsl(r: u8, g: u8, b: u8) -> Hsl {
    let (r, g, b) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return Hsl { h: 0.0, s: 0.0, l };
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    Hsl { h, s, l }
}

pub fn rgb(c: Hsl) -> Color {
    let ch = (1.0 - (2.0 * c.l - 1.0).abs()) * c.s;
    let x = ch * (1.0 - ((c.h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = c.l - ch / 2.0;
    let (r, g, b) = match (c.h.rem_euclid(360.0) / 60.0) as u8 {
        0 => (ch, x, 0.0),
        1 => (x, ch, 0.0),
        2 => (0.0, ch, x),
        3 => (0.0, x, ch),
        4 => (x, 0.0, ch),
        _ => (ch, 0.0, x),
    };
    let to = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Color::Rgb(to(r), to(g), to(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    #[test]
    fn round_trip() {
        for (r, g, b) in [(255, 0, 0), (12, 200, 90), (30, 60, 220), (250, 250, 10)] {
            assert_eq!(rgb(hsl(r, g, b)), Color::Rgb(r, g, b));
        }
    }

    #[test]
    fn two_colors_of_a_cover() {
        // tres cuartos rojo, un cuarto azul, y una franja gris que no cuenta
        let img = RgbImage::from_fn(64, 64, |x, y| match (x, y) {
            (_, 0..8) => Rgb([128, 128, 128]),
            (0..48, _) => Rgb([220, 30, 40]),
            _ => Rgb([20, 60, 230]),
        });
        let (a, b) = of(&DynamicImage::ImageRgb8(img)).unwrap();
        assert!(a.h < 10.0 || a.h > 350.0, "{a:?}");
        assert!((b.h - 228.0).abs() < 10.0, "{b:?}");
        // gris entero: nada
        let grey = RgbImage::from_pixel(32, 32, Rgb([90, 90, 95]));
        assert!(of(&DynamicImage::ImageRgb8(grey)).is_none());
    }

    #[test]
    fn readable_on_the_background() {
        let dim = Hsl { h: 220.0, s: 0.9, l: 0.15 };
        let on_dark = hsl_of(adapt(dim, true));
        let on_light = hsl_of(adapt(dim, false));
        assert!(on_dark.l >= 0.6 && on_light.l <= 0.43);
        assert!(dark(Color::Rgb(220, 220, 220)) && !dark(Color::Rgb(30, 30, 30)));
    }

    fn hsl_of(c: Color) -> Hsl {
        let Color::Rgb(r, g, b) = c else { panic!() };
        hsl(r, g, b)
    }
}
