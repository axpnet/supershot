// SuperShot - Image editing operations
// Copyright (c) 2026 axpnet <https://github.com/axpnet>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Provides non-destructive editing state and processing functions.
// The preview window converts the pixbuf to a DynamicImage, applies all
// edits via the `image` crate, then converts back for live preview.
// The save pipeline uses the same functions for final output.

use image::DynamicImage;
use std::path::Path;

/// Non-destructive editing state applied to a screenshot.
///
/// All values represent adjustments from the original image.
/// Default state means no modifications.
#[derive(Clone, Debug)]
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
        self.rotation == 0
            && !self.flip_h
            && !self.flip_v
            && self.brightness == 0
            && self.contrast == 0
            && !self.grayscale
            && !self.invert
            && self.blur == 0.0
            && self.sharpen == 0.0
    }
}

// ---------------------------------------------------------------------------
// Pixbuf <-> DynamicImage conversion
// ---------------------------------------------------------------------------

/// Convert a GdkPixbuf to an `image` crate DynamicImage.
///
/// On malformed input (negative or zero dimensions, pixel buffer smaller
/// than rowstride × height) returns a blank image of the requested
/// dimensions instead of panicking on out-of-bounds access.
pub fn pixbuf_to_dynamic(pb: &gdk_pixbuf::Pixbuf) -> DynamicImage {
    let width = pb.width().max(0) as u32;
    let height = pb.height().max(0) as u32;
    let rowstride = pb.rowstride().max(0) as usize;
    let n_channels = pb.n_channels().max(0) as usize;
    let has_alpha = pb.has_alpha();

    if width == 0 || height == 0 || n_channels < 3 {
        return if has_alpha {
            DynamicImage::ImageRgba8(image::ImageBuffer::new(width.max(1), height.max(1)))
        } else {
            DynamicImage::ImageRgb8(image::ImageBuffer::new(width.max(1), height.max(1)))
        };
    }

    // SAFETY: `pixels()` returns a slice into pixbuf-owned memory valid
    // for as long as `pb` lives. We do not yield to the main loop while
    // the slice is borrowed and gdk-pixbuf's pixel buffer is not mutated
    // by external code in this code path.
    let pixels = unsafe { pb.pixels() };

    let last_row_offset = (height as usize - 1) * rowstride;
    let needed = last_row_offset + width as usize * n_channels;
    if pixels.len() < needed {
        return if has_alpha {
            DynamicImage::ImageRgba8(image::ImageBuffer::new(width, height))
        } else {
            DynamicImage::ImageRgb8(image::ImageBuffer::new(width, height))
        };
    }

    if has_alpha {
        let mut buf = image::ImageBuffer::<image::Rgba<u8>, Vec<u8>>::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let offset = y as usize * rowstride + x as usize * n_channels;
                buf.put_pixel(x, y, image::Rgba([
                    pixels[offset],
                    pixels[offset + 1],
                    pixels[offset + 2],
                    pixels[offset + 3],
                ]));
            }
        }
        DynamicImage::ImageRgba8(buf)
    } else {
        let mut buf = image::ImageBuffer::<image::Rgb<u8>, Vec<u8>>::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let offset = y as usize * rowstride + x as usize * n_channels;
                buf.put_pixel(x, y, image::Rgb([
                    pixels[offset],
                    pixels[offset + 1],
                    pixels[offset + 2],
                ]));
            }
        }
        DynamicImage::ImageRgb8(buf)
    }
}

/// Convert a DynamicImage back to a GdkPixbuf for display.
pub fn dynamic_to_pixbuf(img: &DynamicImage) -> gdk_pixbuf::Pixbuf {
    let rgba = img.to_rgba8();
    let width = rgba.width() as i32;
    let height = rgba.height() as i32;
    let raw = rgba.into_raw();

    gdk_pixbuf::Pixbuf::from_bytes(
        &gtk4::glib::Bytes::from(&raw),
        gdk_pixbuf::Colorspace::Rgb,
        true,
        8,
        width,
        height,
        width * 4,
    )
}

// ---------------------------------------------------------------------------
// Edit application
// ---------------------------------------------------------------------------

/// Load an image from a file:// URI or file path and apply all edits.
pub fn apply_edits_to_file(
    uri_or_path: &str,
    edits: &EditState,
) -> Result<DynamicImage, Box<dyn std::error::Error>> {
    let path_str = if uri_or_path.starts_with("file://") {
        uri_or_path.strip_prefix("file://").unwrap_or(uri_or_path)
    } else {
        uri_or_path
    };

    let img = image::open(Path::new(path_str))?;
    Ok(apply_edits(img, edits))
}

/// Apply all edits to a DynamicImage in the correct order.
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
        img = img.grayscale();
    }

    // 8. Invert
    if edits.invert {
        img.invert();
    }

    img
}

/// Apply all edits to a pixbuf for live preview.
/// Converts to DynamicImage, applies edits, converts back.
pub fn apply_preview_edits(
    pixbuf: &gdk_pixbuf::Pixbuf,
    edits: &EditState,
) -> gdk_pixbuf::Pixbuf {
    if edits.is_identity() {
        return pixbuf.clone();
    }
    let img = pixbuf_to_dynamic(pixbuf);
    let edited = apply_edits(img, edits);
    dynamic_to_pixbuf(&edited)
}

/// Save a DynamicImage to disk in the specified format.
pub fn save_edited_image(
    img: &DynamicImage,
    dest_path: &Path,
    format_idx: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    if format_idx == 1 {
        let rgb = img.to_rgb8();
        let mut writer = std::io::BufWriter::new(std::fs::File::create(dest_path)?);
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, 90);
        rgb.write_with_encoder(encoder)?;
    } else {
        img.save(dest_path)?;
    }
    Ok(())
}
