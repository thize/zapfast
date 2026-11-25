//! Pictures made at runtime, so the spike downloads nothing: PNG, JPEG, lossless
//! WebP, an animated GIF, and the synthetic call video's frames.

use image::codecs::gif::{GifEncoder, Repeat};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::webp::WebPEncoder;
use image::{ColorType, Delay, Frame, ImageEncoder, Rgba, RgbaImage};
use std::path::{Path, PathBuf};

/// One generated file and what the scene needs to lay it out.
#[derive(Clone, Debug)]
pub struct Picture {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
}

/// The generated pictures. The temp directory lives as long as this does.
pub struct Fixtures {
    _dir: tempfile::TempDir,
    pub png: Picture,
    pub jpeg: Picture,
    pub webp: Picture,
    pub gif: Picture,
}

impl Fixtures {
    pub fn generate() -> std::io::Result<Self> {
        let dir = tempfile::Builder::new()
            .prefix("gpui-chat-spike-")
            .tempdir()?;
        let png = write(dir.path(), "landscape.png", &art(960, 540, 0), Format::Png)?;
        let jpeg = write(dir.path(), "portrait.jpg", &art(540, 960, 1), Format::Jpeg)?;
        let webp = write(dir.path(), "square.webp", &art(600, 600, 2), Format::Webp)?;
        let gif = write_gif(dir.path(), "bounce.gif")?;
        Ok(Self {
            _dir: dir,
            png,
            jpeg,
            webp,
            gif,
        })
    }
}

enum Format {
    Png,
    Jpeg,
    Webp,
}

fn write(dir: &Path, name: &str, image: &RgbaImage, format: Format) -> std::io::Result<Picture> {
    let path = dir.join(name);
    let file = std::io::BufWriter::new(std::fs::File::create(&path)?);
    let (width, height) = image.dimensions();
    let invalid = |e: image::ImageError| std::io::Error::other(e);
    match format {
        Format::Png => image::codecs::png::PngEncoder::new(file)
            .write_image(image.as_raw(), width, height, ColorType::Rgba8.into())
            .map_err(invalid)?,
        Format::Jpeg => {
            // JPEG has no alpha: drop it.
            let rgb = image::DynamicImage::ImageRgba8(image.clone()).into_rgb8();
            JpegEncoder::new_with_quality(file, 88)
                .write_image(rgb.as_raw(), width, height, ColorType::Rgb8.into())
                .map_err(invalid)?
        }
        Format::Webp => WebPEncoder::new_lossless(file)
            .write_image(image.as_raw(), width, height, ColorType::Rgba8.into())
            .map_err(invalid)?,
    }
    Ok(Picture {
        bytes: std::fs::metadata(&path)?.len(),
        path,
        width,
        height,
    })
}

/// A small animated GIF: a ball bouncing over a colour-cycling background,
/// 16 frames of 70 ms.
fn write_gif(dir: &Path, name: &str) -> std::io::Result<Picture> {
    const SIZE: u32 = 200;
    const FRAMES: u32 = 16;
    let path = dir.join(name);
    let file = std::io::BufWriter::new(std::fs::File::create(&path)?);
    let mut encoder = GifEncoder::new(file);
    encoder
        .set_repeat(Repeat::Infinite)
        .map_err(std::io::Error::other)?;
    for n in 0..FRAMES {
        let t = n as f32 / FRAMES as f32;
        let mut image = RgbaImage::new(SIZE, SIZE);
        let ball_x = SIZE as f32 * 0.5;
        // |sin| makes a bounce that touches the floor each loop.
        let ball_y = SIZE as f32 * (0.82 - 0.6 * (t * std::f32::consts::PI).sin().abs());
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let (fx, fy) = (x as f32 / SIZE as f32, y as f32 / SIZE as f32);
            let hue = (t + fx * 0.25 + fy * 0.15).fract();
            let [r, g, b] = hsv(hue, 0.55, 0.9);
            let distance = ((x as f32 - ball_x).powi(2) + (y as f32 - ball_y).powi(2)).sqrt();
            *pixel = if distance < 26.0 {
                Rgba([255, 255, 255, 255])
            } else if y as f32 > SIZE as f32 * 0.88 {
                Rgba([r / 2, g / 2, b / 2, 255])
            } else {
                Rgba([r, g, b, 255])
            };
        }
        encoder
            .encode_frame(Frame::from_parts(
                image,
                0,
                0,
                Delay::from_numer_denom_ms(70, 1),
            ))
            .map_err(std::io::Error::other)?;
    }
    drop(encoder);
    Ok(Picture {
        bytes: std::fs::metadata(&path)?.len(),
        path,
        width: SIZE,
        height: SIZE,
    })
}

fn hsv(h: f32, s: f32, v: f32) -> [u8; 3] {
    let h = h.fract() * 6.0;
    let (c, x) = (v * s, v * s * (1.0 - ((h % 2.0) - 1.0).abs()));
    let m = v - c;
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    ]
}

/// A landscape of a sky gradient, a sun and three ridges, in one of three
/// palettes. Cheap, deterministic, and detailed enough that JPEG and WebP
/// artefacts would show.
pub fn art(width: u32, height: u32, palette: usize) -> RgbaImage {
    const SKIES: [([f32; 3], [f32; 3]); 3] = [
        ([255.0, 190.0, 120.0], [70.0, 60.0, 140.0]),
        ([150.0, 220.0, 255.0], [20.0, 90.0, 160.0]),
        ([255.0, 150.0, 170.0], [40.0, 30.0, 90.0]),
    ];
    let (low, high) = SKIES[palette % SKIES.len()];
    let (w, h) = (width as f32, height as f32);
    let sun = (w * 0.7, h * 0.28, w.min(h) * 0.12);
    RgbaImage::from_fn(width, height, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let t = fy / h;
        let mut color = [
            high[0] + (low[0] - high[0]) * t,
            high[1] + (low[1] - high[1]) * t,
            high[2] + (low[2] - high[2]) * t,
        ];
        let distance = ((fx - sun.0).powi(2) + (fy - sun.1).powi(2)).sqrt();
        if distance < sun.2 {
            color = [255.0, 244.0, 200.0];
        } else if distance < sun.2 * 1.8 {
            let glow = 1.0 - (distance - sun.2) / (sun.2 * 0.8);
            for c in &mut color {
                *c += (255.0 - *c) * glow * 0.35;
            }
        }
        for ridge in 0..3 {
            let base = h * (0.62 + 0.11 * ridge as f32);
            let wave = (fx / w * (4.0 + ridge as f32 * 2.5) * std::f32::consts::TAU
                + ridge as f32 * 1.7)
                .sin()
                * h
                * 0.045
                + (fx / w * 23.0 + ridge as f32).sin() * h * 0.008;
            if fy > base + wave {
                let shade = 0.55 - 0.15 * ridge as f32;
                color = [
                    low[0] * shade * 0.4 + high[0] * 0.1,
                    low[1] * shade * 0.4 + high[1] * 0.1,
                    low[2] * shade * 0.4 + high[2] * 0.15,
                ];
            }
        }
        Rgba([color[0] as u8, color[1] as u8, color[2] as u8, 255])
    })
}

// ----- the synthetic call video ---------------------------------------------

pub const VIDEO_WIDTH: u32 = 640;
pub const VIDEO_HEIGHT: u32 = 360;
pub const VIDEO_FPS: f64 = 30.0;

/// 5x7 glyphs of the digits, one row per byte, bit 4 the leftmost column.
const DIGITS: [[u8; 7]; 10] = [
    [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
    [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
    [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
    [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
    [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
    [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
    [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
    [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
    [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
    [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
];

/// Frame `n` of the synthetic video, BGRA (GPUI's order), 640x360: a diagonal
/// gradient that scrolls and shifts hue, a box that bounces, and the frame
/// number in big digits. Pure and deterministic.
pub fn video_frame(n: u64) -> Vec<u8> {
    let (w, h) = (VIDEO_WIDTH as usize, VIDEO_HEIGHT as usize);
    let mut out = vec![0u8; w * h * 4];
    let phase = (n % 256) as usize;
    for y in 0..h {
        let row = &mut out[y * w * 4..(y + 1) * w * 4];
        for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let diagonal = (x + y + phase * 4) & 0xFF;
            // Three phase-shifted triangle waves make a smooth colour cycle.
            let wave = |offset: usize| -> u8 {
                let v = (diagonal + offset) & 0xFF;
                (if v < 128 { v * 2 } else { (255 - v) * 2 }) as u8
            };
            pixel[0] = wave(170);
            pixel[1] = wave(85);
            pixel[2] = wave(0);
            pixel[3] = 255;
        }
    }
    // The bouncing box.
    let t = n as f32 * 0.07;
    let bx = ((t.sin() * 0.5 + 0.5) * (w - 90) as f32) as usize;
    let by = (((t * 1.3).cos() * 0.5 + 0.5) * (h - 90) as f32) as usize;
    fill(&mut out, w, bx, by, 80, 80, [255, 255, 255, 255]);
    fill(&mut out, w, bx + 6, by + 6, 68, 68, [30, 30, 30, 255]);
    // The frame number, scaled 8x, on a dark plate.
    let text = format!("{n}");
    let scale = 8;
    let glyph_w = 5 * scale + scale;
    let plate_w = text.len() * glyph_w + scale * 2;
    fill(
        &mut out,
        w,
        24,
        24,
        plate_w,
        7 * scale + scale * 2,
        [0, 0, 0, 255],
    );
    for (i, digit) in text.bytes().enumerate() {
        let glyph = &DIGITS[usize::from(digit - b'0')];
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if bits & (0x10 >> col) != 0 {
                    fill(
                        &mut out,
                        w,
                        24 + scale + i * glyph_w + col * scale,
                        24 + scale + row * scale,
                        scale,
                        scale,
                        [255, 255, 255, 255],
                    );
                }
            }
        }
    }
    out
}

fn fill(buffer: &mut [u8], stride: usize, x: usize, y: usize, w: usize, h: usize, bgra: [u8; 4]) {
    let height = buffer.len() / (stride * 4);
    for row in y..(y + h).min(height) {
        for col in x..(x + w).min(stride) {
            let at = (row * stride + col) * 4;
            buffer[at..at + 4].copy_from_slice(&bgra);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageReader;

    #[test]
    fn every_format_is_written_and_reads_back() {
        let fixtures = Fixtures::generate().expect("fixtures");
        for (picture, format, frames) in [
            (&fixtures.png, image::ImageFormat::Png, 1),
            (&fixtures.jpeg, image::ImageFormat::Jpeg, 1),
            (&fixtures.webp, image::ImageFormat::WebP, 1),
            (&fixtures.gif, image::ImageFormat::Gif, 16),
        ] {
            assert!(picture.bytes > 100, "{:?}", picture.path);
            let decoded = ImageReader::open(&picture.path)
                .unwrap()
                .with_guessed_format()
                .unwrap();
            assert_eq!(decoded.format(), Some(format));
            let image = decoded.decode().unwrap();
            assert_eq!(
                (image.width(), image.height()),
                (picture.width, picture.height)
            );
            if frames > 1 {
                use image::AnimationDecoder as _;
                let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(
                    std::fs::File::open(&picture.path).unwrap(),
                ))
                .unwrap();
                assert_eq!(decoder.into_frames().count(), frames);
            }
        }
        let landscape = (fixtures.png.width, fixtures.png.height);
        assert!(landscape.0 > landscape.1);
        assert!(fixtures.jpeg.height > fixtures.jpeg.width);
    }

    #[test]
    fn video_frames_are_full_size_distinct_and_deterministic() {
        let a = video_frame(0);
        assert_eq!(a.len(), (VIDEO_WIDTH * VIDEO_HEIGHT * 4) as usize);
        assert_eq!(a, video_frame(0));
        assert_ne!(a, video_frame(1));
        assert!(a.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
        // The counter plate is black with white digits in it.
        let plate = (30 * VIDEO_WIDTH as usize + 30) * 4;
        assert_eq!(&a[plate..plate + 3], &[0, 0, 0]);
        assert!(video_frame(12345).len() == a.len());
    }
}
