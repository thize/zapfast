//! The Lottie sticker. ZapFast has no Lottie fixture in the repository (its
//! tests build `.was` archives around an empty animation), so this one is
//! hand-written: a bouncing ball with a squash and a shadow, 128x128 at 30 fps
//! for two seconds. It is rendered with the crate and settings ZapFast uses
//! (`rasterlottie` 0.2.4, `images` and `text` only, `src/lottie.rs`).

use image::RgbaImage;
use rasterlottie::{Animation, RenderConfig, Renderer, Rgba8};

/// The animation, as Lottie JSON.
pub const BOUNCE: &str = r#"{
  "v": "5.7.6", "fr": 30, "ip": 0, "op": 60, "w": 128, "h": 128,
  "nm": "Bounce",
  "layers": [
    {
      "nm": "Ball", "ind": 1, "ty": 4, "ip": 0, "op": 60, "st": 0, "sr": 1,
      "ks": {
        "a": {"a": 0, "k": [0, 0, 0]}, "p": {"a": 0, "k": [0, 0, 0]},
        "s": {"a": 0, "k": [100, 100, 100]}, "r": {"a": 0, "k": 0},
        "o": {"a": 0, "k": 100}
      },
      "shapes": [
        {
          "ty": "gr",
          "it": [
            {
              "ty": "el",
              "p": {"a": 1, "k": [
                {"t": 0,  "s": [64, 26], "e": [64, 96],
                 "i": {"x": [0.6], "y": [1]}, "o": {"x": [0.8], "y": [0]}},
                {"t": 30, "s": [64, 96], "e": [64, 26],
                 "i": {"x": [0.2], "y": [1]}, "o": {"x": [0.4], "y": [0]}},
                {"t": 60, "s": [64, 26]}
              ]},
              "s": {"a": 1, "k": [
                {"t": 0,  "s": [38, 38], "e": [38, 38]},
                {"t": 27, "s": [38, 38], "e": [54, 24]},
                {"t": 30, "s": [54, 24], "e": [38, 38]},
                {"t": 34, "s": [38, 38], "e": [38, 38]},
                {"t": 60, "s": [38, 38]}
              ]}
            },
            {"ty": "fl", "c": {"a": 0, "k": [0.0, 0.659, 0.518, 1]}, "o": {"a": 0, "k": 100}},
            {"ty": "tr", "a": {"a": 0, "k": [0, 0]}, "p": {"a": 0, "k": [0, 0]},
             "s": {"a": 0, "k": [100, 100]}, "r": {"a": 0, "k": 0}, "o": {"a": 0, "k": 100}}
          ]
        }
      ]
    },
    {
      "nm": "Shadow", "ind": 2, "ty": 4, "ip": 0, "op": 60, "st": 0, "sr": 1,
      "ks": {
        "a": {"a": 0, "k": [0, 0, 0]}, "p": {"a": 0, "k": [0, 0, 0]},
        "s": {"a": 0, "k": [100, 100, 100]}, "r": {"a": 0, "k": 0},
        "o": {"a": 0, "k": 100}
      },
      "shapes": [
        {
          "ty": "gr",
          "it": [
            {"ty": "el", "p": {"a": 0, "k": [64, 114]},
             "s": {"a": 1, "k": [
               {"t": 0,  "s": [20, 5], "e": [56, 9]},
               {"t": 30, "s": [56, 9], "e": [20, 5]},
               {"t": 60, "s": [20, 5]}
             ]}},
            {"ty": "fl", "c": {"a": 0, "k": [0.1, 0.1, 0.1, 1]}, "o": {"a": 0, "k": 30}},
            {"ty": "tr", "a": {"a": 0, "k": [0, 0]}, "p": {"a": 0, "k": [0, 0]},
             "s": {"a": 0, "k": [100, 100]}, "r": {"a": 0, "k": 0}, "o": {"a": 0, "k": 100}}
          ]
        }
      ]
    }
  ]
}"#;

/// A rendered clip: straight-alpha BGRA frames, as GPUI paints them.
pub struct Clip {
    pub frames: Vec<RgbaImage>,
    /// The animation's own frame rate.
    pub fps: f32,
}

/// Rasterizes every frame of `json` at `scale` times its canvas size, the way
/// `src/lottie.rs::decode` does, but with all frames and the native rate.
pub fn render(json: &str, scale: f32) -> Result<Clip, String> {
    let animation = Animation::from_json_str(json).map_err(|e| e.to_string())?;
    if !animation.frame_rate.is_finite() || animation.frame_rate <= 0.0 {
        return Err("the animation has no frame rate".into());
    }
    let prepared = Renderer::default()
        .prepare(&animation)
        .map_err(|e| e.to_string())?;
    let config = RenderConfig::new(Rgba8::TRANSPARENT, scale);
    let count = animation.duration_frames().round().max(1.0) as usize;
    let mut frames = Vec::with_capacity(count);
    for index in 0..count {
        let raster = prepared
            .render_frame(animation.in_point + index as f32, config)
            .map_err(|e| e.to_string())?;
        let bgra = super::frames::premultiplied_rgba_to_bgra(&raster.pixels);
        frames.push(
            RgbaImage::from_raw(raster.width, raster.height, bgra)
                .ok_or("the raster has the wrong size")?,
        );
    }
    Ok(Clip {
        frames,
        fps: animation.frame_rate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bounce_renders_sixty_distinct_frames_at_thirty_fps() {
        let clip = render(BOUNCE, 2.0).expect("the animation renders");
        assert_eq!(clip.fps, 30.0);
        assert_eq!(clip.frames.len(), 60);
        assert_eq!(clip.frames[0].dimensions(), (256, 256));
        // Transparent corner, opaque ball: it is a sticker.
        let corner = clip.frames[0].get_pixel(0, 0);
        assert_eq!(corner.0[3], 0);
        let opaque = clip.frames[30].pixels().filter(|p| p.0[3] == 255).count();
        assert!(opaque > 1000, "{opaque} opaque pixels");
        // The ball moves: the first, the middle and the next-to-last differ.
        assert_ne!(clip.frames[0].as_raw(), clip.frames[15].as_raw());
        assert_ne!(clip.frames[15].as_raw(), clip.frames[30].as_raw());
        // And the clip loops: the last frame is close to the first.
        let diff = clip.frames[0]
            .as_raw()
            .iter()
            .zip(clip.frames[59].as_raw())
            .filter(|(a, b)| a != b)
            .count();
        assert!(diff < 20_000, "{diff} bytes differ between first and last");
        // Straight BGRA: the green ball (0, 168, 132) has blue 132, red 0.
        let ball = clip.frames[0]
            .pixels()
            .find(|p| p.0[3] == 255 && p.0[1] > 100)
            .expect("an opaque green pixel");
        assert!(ball.0[0] > ball.0[2], "BGRA puts blue first: {ball:?}");
    }
}
