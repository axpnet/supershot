// SuperShot - Image editing operations
// Copyright (c) 2026 axpnet <https://github.com/axpnet>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Holds the non-destructive edit state and the pixel operations that realise
// it. `image::RgbaImage` is the single canonical representation throughout:
// the preview loads the capture with the `image` crate, edits it, and converts
// the result to a Cairo surface only for display.
//
// The previous implementation round-tripped GdkPixbuf -> DynamicImage on every
// slider tick using an `unsafe` borrow of the pixbuf's pixel buffer and a
// per-pixel `put_pixel` loop. On a 4K capture that is 8.3 million bounds-checked
// calls per frame, on the GTK main thread, before the edit itself even runs.
// Loading through `image` directly removes both the unsafe block and the
// conversion entirely.

use gtk4::prelude::FileExt;
use image::{DynamicImage, RgbaImage};
use std::path::Path;

/// Longest edge of the image used for live preview rendering.
///
/// Edits are recomputed on every slider movement, and cost scales with pixel
/// count: a gaussian blur over a full 4K frame takes seconds. Working on a
/// bounded copy keeps interaction responsive while remaining visually faithful,
/// and the final save always re-runs the same edits at full resolution.
pub const PREVIEW_MAX_EDGE: u32 = 1600;

/// Non-destructive editing state applied to a screenshot.
///
/// All values represent adjustments from the original image.
/// Default state means no modifications.
#[derive(Clone, Debug, PartialEq)]
pub struct EditState {
    /// Rotation in degrees: 0, 90, 180, or 270.
    pub rotation: u32,
    /// Flip horizontally.
    pub flip_h: bool,
    /// Flip vertically.
    pub flip_v: bool,
    /// Brightness adjustment (-100 to +100).
    pub brightness: i32,
    /// Contrast adjustment (-100 to +100).
    pub contrast: i32,
    /// Convert to grayscale.
    pub grayscale: bool,
    /// Invert colors.
    pub invert: bool,
    /// Gaussian blur sigma (0.0 = off, max 10.0).
    pub blur: f32,
    /// Unsharp mask sigma for sharpening (0.0 = off, max 10.0).
    pub sharpen: f32,
}

impl Default for EditState {
    fn default() -> Self {
        Self {
            rotation: 0,
            flip_h: false,
            flip_v: false,
            brightness: 0,
            contrast: 0,
            grayscale: false,
            invert: false,
            blur: 0.0,
            sharpen: 0.0,
        }
    }
}

impl EditState {
    /// Returns true if no edits have been applied.
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    /// True when the edit changes the image's dimensions.
    ///
    /// Annotation coordinates are stored against the geometry-corrected image,
    /// so the preview has to know when a change invalidates them.
    pub fn changes_geometry(&self) -> bool {
        self.rotation == 90 || self.rotation == 270
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Resolve a `file://` URI or a bare path to a filesystem path.
///
/// A `file://` URI is percent-encoded, so it is decoded by GIO rather than by
/// stripping the scheme prefix; the naive strip corrupts every path containing
/// a space, a `#`, a `%` or a non-ASCII character.
pub fn source_path(uri_or_path: &str) -> Option<std::path::PathBuf> {
    if uri_or_path.is_empty() {
        return None;
    }
    if uri_or_path.starts_with("file://") {
        gtk4::gio::File::for_uri(uri_or_path).path()
    } else {
        Some(Path::new(uri_or_path).to_path_buf())
    }
}

/// Load a capture from a `file://` URI or a bare path.
pub fn load(uri_or_path: &str) -> Result<RgbaImage, String> {
    let path = source_path(uri_or_path)
        .ok_or_else(|| format!("cannot resolve {}", uri_or_path))?;

    image::open(&path)
        .map(|img| img.to_rgba8())
        .map_err(|e| format!("{}: {}", path.display(), e))
}

// ---------------------------------------------------------------------------
// Edit application
// ---------------------------------------------------------------------------

/// Apply all edits to a DynamicImage in the correct order.
///
/// Geometry first so that colour work happens on the final orientation, then
/// tonal adjustments, then the convolution filters, then the colour-space
/// conversions that are cheapest to apply last.
pub fn apply_edits(mut img: DynamicImage, edits: &EditState) -> DynamicImage {
    // 1. Rotation
    img = match edits.rotation {
        90 => img.rotate90(),
        180 => img.rotate180(),
        270 => img.rotate270(),
        _ => img,
    };

    // 2. Flip
    if edits.flip_h {
        img = img.fliph();
    }
    if edits.flip_v {
        img = img.flipv();
    }

    // 3. Brightness
    if edits.brightness != 0 {
        img = img.brighten(edits.brightness);
    }

    // 4. Contrast
    if edits.contrast != 0 {
        img = img.adjust_contrast(edits.contrast as f32);
    }

    // 5. Blur
    if edits.blur > 0.0 {
        img = img.blur(edits.blur);
    }

    // 6. Sharpen (unsharp mask)
    if edits.sharpen > 0.0 {
        img = img.unsharpen(edits.sharpen, 5);
    }

    // 7. Grayscale
    if edits.grayscale {
        // `grayscale()` returns a luma image; converting back to RGBA keeps the
        // pipeline's pixel type uniform for the stages that follow.
        img = DynamicImage::ImageRgba8(img.grayscale().to_rgba8());
    }

    // 8. Invert
    if edits.invert {
        img.invert();
    }

    img
}

/// Apply edits to an owned RGBA image.
pub fn apply_edits_rgba(img: &RgbaImage, edits: &EditState) -> RgbaImage {
    if edits.is_identity() {
        return img.clone();
    }
    apply_edits(DynamicImage::ImageRgba8(img.clone()), edits).to_rgba8()
}

/// Crop an RGBA image, clamping the rectangle to the image bounds.
///
/// Returns the input unchanged when the rectangle does not intersect the image,
/// which is safer than producing a zero-sized image the encoder would reject.
pub fn crop_rgba(img: RgbaImage, x: i32, y: i32, w: i32, h: i32) -> RgbaImage {
    let (iw, ih) = (img.width() as i64, img.height() as i64);

    let x0 = (x as i64).clamp(0, iw);
    let y0 = (y as i64).clamp(0, ih);
    let x1 = (x as i64 + w.max(0) as i64).clamp(0, iw);
    let y1 = (y as i64 + h.max(0) as i64).clamp(0, ih);

    let cw = (x1 - x0) as u32;
    let ch = (y1 - y0) as u32;
    if cw == 0 || ch == 0 {
        return img;
    }

    image::imageops::crop_imm(&img, x0 as u32, y0 as u32, cw, ch).to_image()
}

/// Downscale for live preview if the image exceeds `PREVIEW_MAX_EDGE`.
///
/// Returns the image together with the scale factor applied, so the caller can
/// map between preview and full-resolution coordinates.
pub fn downscale_for_preview(img: &RgbaImage) -> (RgbaImage, f64) {
    let longest = img.width().max(img.height());
    if longest <= PREVIEW_MAX_EDGE {
        return (img.clone(), 1.0);
    }

    let scale = PREVIEW_MAX_EDGE as f64 / longest as f64;
    let nw = ((img.width() as f64 * scale).round() as u32).max(1);
    let nh = ((img.height() as f64 * scale).round() as u32).max(1);

    // Triangle filter: markedly faster than Lanczos at this size and the
    // difference is invisible in a preview that is about to be scaled again for
    // display.
    let scaled = image::imageops::resize(img, nw, nh, image::imageops::FilterType::Triangle);
    (scaled, scale)
}

// ---------------------------------------------------------------------------
// Cairo interop
// ---------------------------------------------------------------------------

/// Convert an RGBA image into a Cairo ARGB32 surface for on-screen drawing.
///
/// Cairo expects premultiplied alpha in native-endian 32-bit words, i.e. bytes
/// ordered B, G, R, A on the platforms SuperShot targets. Rows are written
/// through the surface's own stride, which Cairo pads to a 4-byte boundary and
/// which therefore cannot be assumed equal to `width * 4`.
pub fn rgba_to_surface(img: &RgbaImage) -> Option<cairo::ImageSurface> {
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return None;
    }

    let mut surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, w as i32, h as i32).ok()?;
    let stride = surface.stride() as usize;

    {
        let mut data = surface.data().ok()?;
        for y in 0..h as usize {
            let row_start = y * stride;
            let src_row = &img.as_raw()[y * w as usize * 4..(y + 1) * w as usize * 4];

            for x in 0..w as usize {
                let px = &src_row[x * 4..x * 4 + 4];
                let a = px[3] as u32;
                let premul = |c: u8| -> u8 { ((c as u32 * a + 127) / 255) as u8 };

                let dst = row_start + x * 4;
                data[dst] = premul(px[2]);     // B
                data[dst + 1] = premul(px[1]); // G
                data[dst + 2] = premul(px[0]); // R
                data[dst + 3] = px[3];         // A
            }
        }
    }

    surface.mark_dirty();
    Some(surface)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, image::Rgba([1, 2, 3, 255]))
    }

    /// URIs are percent-encoded. Stripping the "file://" prefix by hand, as the
    /// pipeline used to, corrupts every path containing a space, a '#', a '%'
    /// or a non-ASCII character — routine for a localized Pictures folder.
    #[test]
    fn source_path_decodes_percent_encoded_uris() {
        let cases = [
            ("file:///home/u/My%20Shots/a.png", "/home/u/My Shots/a.png"),
            ("file:///home/u/Immagini/pi%C3%B9.png", "/home/u/Immagini/più.png"),
            ("file:///home/u/a%23b.png", "/home/u/a#b.png"),
            ("file:///home/u/100%25.png", "/home/u/100%.png"),
        ];

        for (uri, expected) in cases {
            let path = source_path(uri).expect("a file:// URI must resolve");
            assert_eq!(path, Path::new(expected), "failed for {uri}");
        }
    }

    #[test]
    fn source_path_passes_bare_paths_through() {
        assert_eq!(
            source_path("/tmp/a b.png").expect("a bare path must resolve"),
            Path::new("/tmp/a b.png")
        );
        assert!(source_path("").is_none());
    }

    #[test]
    fn crop_clamps_to_the_image_and_never_yields_an_empty_result() {
        let out = crop_rgba(img(100, 100), 90, 90, 50, 50);
        assert_eq!((out.width(), out.height()), (10, 10));

        // Entirely outside: the encoder cannot accept a zero-sized image, so
        // the input is returned untouched rather than degenerate.
        let out = crop_rgba(img(100, 100), 500, 500, 10, 10);
        assert_eq!((out.width(), out.height()), (100, 100));

        // Negative origin.
        let out = crop_rgba(img(100, 100), -20, -20, 50, 50);
        assert_eq!((out.width(), out.height()), (30, 30));

        // Zero-sized request.
        let out = crop_rgba(img(100, 100), 10, 10, 0, 0);
        assert_eq!((out.width(), out.height()), (100, 100));
    }

    #[test]
    fn downscale_bounds_the_longest_edge_and_reports_its_scale() {
        let (small, scale) = downscale_for_preview(&img(800, 600));
        assert_eq!((small.width(), small.height()), (800, 600));
        assert_eq!(scale, 1.0, "an image within the bound must not be resampled");

        let (big, scale) = downscale_for_preview(&img(3840, 2160));
        assert_eq!(big.width().max(big.height()), PREVIEW_MAX_EDGE);
        assert!((scale - PREVIEW_MAX_EDGE as f64 / 3840.0).abs() < 1e-9);
        // Aspect ratio is preserved.
        let ratio_in = 3840.0 / 2160.0;
        let ratio_out = big.width() as f64 / big.height() as f64;
        assert!((ratio_in - ratio_out).abs() < 0.01);

        // A tall image is bounded on its own longest edge.
        let (tall, _) = downscale_for_preview(&img(500, 4000));
        assert_eq!(tall.height(), PREVIEW_MAX_EDGE);
    }

    #[test]
    fn quarter_turns_swap_the_axes_and_half_turns_do_not() {
        let quarter = EditState { rotation: 90, ..EditState::default() };
        let out = apply_edits_rgba(&img(400, 200), &quarter);
        assert_eq!((out.width(), out.height()), (200, 400));

        let half = EditState { rotation: 180, ..EditState::default() };
        let out = apply_edits_rgba(&img(400, 200), &half);
        assert_eq!((out.width(), out.height()), (400, 200));

        assert!(quarter.changes_geometry());
        assert!(!half.changes_geometry());
        assert!(EditState { rotation: 270, ..EditState::default() }.changes_geometry());
    }

    #[test]
    fn the_identity_edit_state_leaves_the_image_untouched() {
        let source = img(64, 64);
        let out = apply_edits_rgba(&source, &EditState::default());
        assert!(EditState::default().is_identity());
        assert_eq!(out.as_raw(), source.as_raw());
    }

    #[test]
    fn grayscale_keeps_the_pixel_type_rgba_for_later_stages() {
        // A luma round-trip used to change the buffer type mid-pipeline; every
        // stage after it expects RGBA.
        let edits = EditState { grayscale: true, ..EditState::default() };
        let out = apply_edits_rgba(&RgbaImage::from_pixel(8, 8, image::Rgba([200, 40, 10, 255])), &edits);

        let px = out.get_pixel(0, 0);
        assert_eq!(px[0], px[1]);
        assert_eq!(px[1], px[2]);
        assert_eq!(px[3], 255, "alpha must survive the grayscale conversion");
    }

    #[test]
    fn invert_and_flips_are_involutions() {
        let source = RgbaImage::from_pixel(8, 8, image::Rgba([10, 200, 30, 255]));

        let inverted = apply_edits_rgba(&source, &EditState { invert: true, ..EditState::default() });
        assert_ne!(inverted.as_raw(), source.as_raw());
        let back = apply_edits_rgba(&inverted, &EditState { invert: true, ..EditState::default() });
        assert_eq!(back.as_raw(), source.as_raw());

        for edits in [
            EditState { flip_h: true, ..EditState::default() },
            EditState { flip_v: true, ..EditState::default() },
        ] {
            let once = apply_edits_rgba(&source, &edits);
            let twice = apply_edits_rgba(&once, &edits);
            assert_eq!(twice.as_raw(), source.as_raw());
        }
    }

    /// Cairo pads each row to a 4-byte boundary, so the surface stride cannot
    /// be assumed to equal width * 4; a width that forces padding catches a
    /// conversion that ignores it.
    #[test]
    fn rgba_to_surface_round_trips_through_a_padded_stride() {
        let mut source = RgbaImage::new(7, 3);
        for (x, y, px) in source.enumerate_pixels_mut() {
            *px = image::Rgba([(x * 30) as u8, (y * 80) as u8, 128, 255]);
        }

        let surface = rgba_to_surface(&source).expect("surface creation must succeed");
        assert_eq!((surface.width(), surface.height()), (7, 3));

        let stride = surface.stride() as usize;
        let data = surface.take_data().expect("surface data must be readable");

        for y in 0..3usize {
            for x in 0..7usize {
                let px = &data[y * stride + x * 4..y * stride + x * 4 + 4];
                let expected = source.get_pixel(x as u32, y as u32).0;
                // Opaque pixels are unaffected by premultiplication; the bytes
                // are stored B, G, R, A.
                assert_eq!(px[0], expected[2], "blue at {x},{y}");
                assert_eq!(px[1], expected[1], "green at {x},{y}");
                assert_eq!(px[2], expected[0], "red at {x},{y}");
                assert_eq!(px[3], 255);
            }
        }
    }

    #[test]
    fn rgba_to_surface_rejects_degenerate_sizes() {
        assert!(rgba_to_surface(&RgbaImage::new(0, 4)).is_none());
        assert!(rgba_to_surface(&RgbaImage::new(4, 0)).is_none());
    }
}
