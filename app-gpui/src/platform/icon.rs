//! ZapFast's tray icon, copied from the root package's `src/util.rs`
//! (`app_icon_rgba`, `tray_template_rgba`): its logo, rasterized with resvg.
//! `fastframe-tray` takes plain function pointers, so these cannot capture.

/// ZapFast's logo (`packaging/icons`), referenced by repository path.
const MARK: &[u8] = include_bytes!("../../../packaging/icons/zapfast.svg");
/// The same mark without its rim and shading, which blur at small sizes.
const SMALL_MARK: &[u8] = include_bytes!("../../../packaging/icons/zapfast-small.svg");
const SMALL_BELOW: usize = 40;

/// The full-colour icon as straight-alpha RGBA, `size` pixels square.
pub fn app_icon_rgba(size: usize) -> Vec<u8> {
    let mark = if size < SMALL_BELOW { SMALL_MARK } else { MARK };
    render_mark(mark, size).unwrap_or_else(|| plain_disc(size))
}

fn render_mark(mark: &[u8], size: usize) -> Option<Vec<u8>> {
    let side = size.max(1) as u32;
    let tree = resvg::usvg::Tree::from_data(mark, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(side, side)?;
    let scale = side as f32 / tree.size().width();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Some(
        pixmap
            .pixels()
            .iter()
            .flat_map(|pixel| {
                let color = pixel.demultiply();
                [color.red(), color.green(), color.blue(), color.alpha()]
            })
            .collect(),
    )
}

fn plain_disc(size: usize) -> Vec<u8> {
    let mut rgba = vec![0u8; size * size * 4];
    let center = size as f32 / 2.0;
    let radius = center - 2.0;
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let distance = ((px - center).powi(2) + (py - center).powi(2)).sqrt();
            let coverage = (radius - distance + 0.5).clamp(0.0, 1.0);
            let index = (y * size + x) * 4;
            rgba[index + 1] = 168;
            rgba[index + 2] = 132;
            rgba[index + 3] = (coverage * 255.0) as u8;
        }
    }
    rgba
}

/// The macOS menu-bar template: the disc with the bubble cut out of it, black
/// on transparent, which the menu bar recolours.
pub fn tray_template_rgba(size: usize) -> Vec<u8> {
    let mut rgba = render_mark(SMALL_MARK, size).unwrap_or_else(|| plain_disc(size));
    const INK: f32 = 14.0;
    const DISC: f32 = 168.0;
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let disc = ((f32::from(pixel[1]) - INK) / (DISC - INK)).clamp(0.0, 1.0);
        pixel[3] = (f32::from(pixel[3]) * disc).round() as u8;
        pixel[0] = 0;
        pixel[1] = 0;
        pixel[2] = 0;
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_logo_rasterizes_at_tray_and_dock_sizes() {
        for size in [36, 64, 128] {
            let rgba = app_icon_rgba(size);
            assert_eq!(rgba.len(), size * size * 4);
            let opaque = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[3] > 200)
                .count();
            assert!(opaque > size * size / 4, "{size}: {opaque} opaque pixels");
        }
    }

    #[test]
    fn the_template_is_black_on_transparent() {
        let template = tray_template_rgba(36);
        assert!(
            template
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[..3] == [0, 0, 0])
        );
        let inked = template
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 128)
            .count();
        let clear = template
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] == 0)
            .count();
        assert!(inked > 100 && clear > 100, "{inked} inked, {clear} clear");
    }
}
