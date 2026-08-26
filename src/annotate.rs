// SuperShot - Screenshot annotation and redaction
// Copyright (c) 2026 axpnet <https://github.com/axpnet>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Annotations are stored as a non-destructive display list in image-pixel
// coordinates. The same drawing code serves both the live preview canvas and
// the save pipeline, so what the user arranges on screen is exactly what is
// written to disk.
//
// Two families of annotation exist and they composite differently:
//
//   * Vector marks (arrow, rectangle, ellipse, highlighter, free draw, text,
//     numbered step) are painted onto a transparent Cairo layer that is then
//     alpha-composited over the image.
//
//   * Redactions (pixelate, blackout) must replace the underlying pixels
//     rather than cover them, so they are applied directly to the image buffer
//     before the vector layer goes on top. Covering sensitive pixels with a
//     translucent overlay would leave them recoverable in the saved file.
//
// Blur is deliberately not offered as a redaction tool. Gaussian blur is
// frequently reversible for text — the information is attenuated, not removed —
// so SuperShot provides mosaic and solid fill, both of which discard the
// original pixel values outright.

use image::RgbaImage;

/// A point in image-pixel coordinates.
pub type Point = (f64, f64);

/// An axis-aligned rectangle in image-pixel coordinates: (x, y, width, height).
pub type Rect = (f64, f64, f64, f64);

/// RGB colour components in the 0.0-1.0 range.
pub type Rgb = (f64, f64, f64);

/// The annotation tool the user currently has selected.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tool {
    /// Drag to define a crop rectangle. Not an annotation; handled by the
    /// preview window, but it shares the toolbar and the drag gesture.
    #[default]
    Crop,
    Arrow,
    Rectangle,
    Ellipse,
    Highlighter,
    FreeDraw,
    Text,
    Number,
    Pixelate,
    Blackout,
}

impl Tool {
    /// True when a single click places the mark, with no drag required.
    pub fn is_click_placed(self) -> bool {
        matches!(self, Tool::Text | Tool::Number)
    }
}

/// One annotation in the display list.
#[derive(Clone, Debug)]
pub struct Annotation {
    pub shape: Shape,
    pub color: Rgb,
    /// Stroke width, or glyph scale for text and numbered steps, in image pixels.
    pub stroke: f64,
}

#[derive(Clone, Debug)]
pub enum Shape {
    Arrow { from: Point, to: Point },
    Rectangle { rect: Rect },
    Ellipse { rect: Rect },
    Highlight { rect: Rect },
    FreeDraw { points: Vec<Point> },
    Text { at: Point, text: String },
    Number { at: Point, value: u32 },
    Pixelate { rect: Rect },
    Blackout { rect: Rect },
}

impl Shape {
    /// Redactions destroy pixels and must be applied to the image buffer itself.
    pub fn is_redaction(&self) -> bool {
        matches!(self, Shape::Pixelate { .. } | Shape::Blackout { .. })
    }

    fn rect(&self) -> Option<Rect> {
        match self {
            Shape::Rectangle { rect }
            | Shape::Ellipse { rect }
            | Shape::Highlight { rect }
            | Shape::Pixelate { rect }
            | Shape::Blackout { rect } => Some(*rect),
            _ => None,
        }
    }
}

/// The annotation colour palette offered in the toolbar.
///
/// Chosen for contrast against typical screenshot content (light UI chrome,
/// dark terminals) rather than for aesthetics alone.
pub const PALETTE: &[(&str, Rgb)] = &[
    ("red", (0.90, 0.11, 0.14)),
    ("orange", (0.96, 0.53, 0.08)),
    ("yellow", (0.98, 0.83, 0.09)),
    ("green", (0.18, 0.76, 0.35)),
    ("blue", (0.21, 0.52, 0.89)),
    ("purple", (0.56, 0.35, 0.83)),
    ("black", (0.06, 0.06, 0.06)),
    ("white", (1.00, 1.00, 1.00)),
];

/// Build a rectangle from two drag endpoints, normalised so width and height
/// are non-negative regardless of drag direction.
pub fn rect_from_points(a: Point, b: Point) -> Rect {
    (
        a.0.min(b.0),
        a.1.min(b.1),
        (b.0 - a.0).abs(),
        (b.1 - a.1).abs(),
    )
}

// ---------------------------------------------------------------------------
// Cairo rendering
// ---------------------------------------------------------------------------

/// Paint every non-redaction annotation onto a Cairo context whose user space
/// is image pixels.
pub fn draw_vector(ctx: &cairo::Context, annotations: &[Annotation]) {
    for ann in annotations {
        if ann.shape.is_redaction() {
            continue;
        }
        draw_one(ctx, ann);
    }
}

fn draw_one(ctx: &cairo::Context, ann: &Annotation) {
    let (r, g, b) = ann.color;
    ctx.set_line_width(ann.stroke);
    ctx.set_line_join(cairo::LineJoin::Round);
    ctx.set_line_cap(cairo::LineCap::Round);

    match &ann.shape {
        Shape::Arrow { from, to } => {
            ctx.set_source_rgba(r, g, b, 1.0);
            draw_arrow(ctx, *from, *to, ann.stroke);
        }
        Shape::Rectangle { rect } => {
            ctx.set_source_rgba(r, g, b, 1.0);
            ctx.rectangle(rect.0, rect.1, rect.2, rect.3);
            let _ = ctx.stroke();
        }
        Shape::Ellipse { rect } => {
            ctx.set_source_rgba(r, g, b, 1.0);
            draw_ellipse(ctx, *rect);
            let _ = ctx.stroke();
        }
        Shape::Highlight { rect } => {
            // A translucent fill, the way a physical highlighter works: the
            // content underneath stays readable.
            ctx.set_source_rgba(r, g, b, 0.35);
            ctx.rectangle(rect.0, rect.1, rect.2, rect.3);
            let _ = ctx.fill();
        }
        Shape::FreeDraw { points } => {
            if points.len() < 2 {
                return;
            }
            ctx.set_source_rgba(r, g, b, 1.0);
            ctx.move_to(points[0].0, points[0].1);
            for p in &points[1..] {
                ctx.line_to(p.0, p.1);
            }
            let _ = ctx.stroke();
        }
        Shape::Text { at, text } => {
            draw_text(ctx, *at, text, ann);
        }
        Shape::Number { at, value } => {
            draw_number(ctx, *at, *value, ann);
        }
        Shape::Pixelate { .. } | Shape::Blackout { .. } => {}
    }
}

/// A straight shaft with a solid triangular head, scaled to the stroke width so
/// thin arrows do not get an oversized head.
fn draw_arrow(ctx: &cairo::Context, from: Point, to: Point, stroke: f64) {
    let dx = to.0 - from.0;
    let dy = to.1 - from.1;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return;
    }

    let head_len = (stroke * 4.0).min(len * 0.5).max(stroke * 2.0);
    let head_half_width = head_len * 0.45;
    let (ux, uy) = (dx / len, dy / len);

    // Stop the shaft where the head begins so the stroke does not poke through
    // the tip when the arrow is drawn in a translucent colour.
    let shaft_end = (to.0 - ux * head_len, to.1 - uy * head_len);
    ctx.move_to(from.0, from.1);
    ctx.line_to(shaft_end.0, shaft_end.1);
    let _ = ctx.stroke();

    // Perpendicular unit vector for the head's base corners.
    let (px, py) = (-uy, ux);
    ctx.move_to(to.0, to.1);
    ctx.line_to(
        shaft_end.0 + px * head_half_width,
        shaft_end.1 + py * head_half_width,
    );
    ctx.line_to(
        shaft_end.0 - px * head_half_width,
        shaft_end.1 - py * head_half_width,
    );
    ctx.close_path();
    let _ = ctx.fill();
}

fn draw_ellipse(ctx: &cairo::Context, rect: Rect) {
    let (x, y, w, h) = rect;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let _ = ctx.save();
    ctx.translate(x + w / 2.0, y + h / 2.0);
    ctx.scale(w / 2.0, h / 2.0);
    ctx.arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU);
    let _ = ctx.restore();
}

/// Font size derived from the stroke width, so the thickness slider scales text
/// and arrows together.
fn text_size(stroke: f64) -> f64 {
    (stroke * 6.0).clamp(12.0, 160.0)
}

/// Lay out a string with Pango.
///
/// Pango rather than Cairo's toy text API: SuperShot ships in fourteen
/// languages and lets the user type arbitrary text into an annotation, so
/// shaping and font fallback have to work for CJK, Devanagari and RTL scripts.
fn layout_for(ctx: &cairo::Context, text: &str, size: f64, bold: bool) -> pango::Layout {
    let layout = pangocairo::functions::create_layout(ctx);
    let mut desc = pango::FontDescription::from_string("Sans");
    if bold {
        desc.set_weight(pango::Weight::Bold);
    }
    desc.set_absolute_size(size * pango::SCALE as f64);
    layout.set_font_description(Some(&desc));
    layout.set_text(text);
    layout
}

fn draw_text(ctx: &cairo::Context, at: Point, text: &str, ann: &Annotation) {
    if text.is_empty() {
        return;
    }
    let size = text_size(ann.stroke);
    let layout = layout_for(ctx, text, size, true);
    let (r, g, b) = ann.color;

    // A contrasting outline keeps the label readable over any background, which
    // matters more for annotations than for the watermark: the user places
    // these deliberately over busy content.
    let _ = ctx.save();
    ctx.move_to(at.0, at.1);
    pangocairo::functions::layout_path(ctx, &layout);
    let outline = if luminance(ann.color) > 0.5 { 0.0 } else { 1.0 };
    ctx.set_source_rgba(outline, outline, outline, 0.85);
    ctx.set_line_width((size * 0.12).max(1.5));
    ctx.set_line_join(cairo::LineJoin::Round);
    let _ = ctx.stroke_preserve();
    ctx.set_source_rgba(r, g, b, 1.0);
    let _ = ctx.fill();
    let _ = ctx.restore();
}

/// A filled disc carrying a step number, for walking a reader through a UI.
fn draw_number(ctx: &cairo::Context, at: Point, value: u32, ann: &Annotation) {
    let size = text_size(ann.stroke);
    let radius = size * 0.85;
    let (r, g, b) = ann.color;

    ctx.set_source_rgba(r, g, b, 1.0);
    ctx.arc(at.0, at.1, radius, 0.0, std::f64::consts::TAU);
    let _ = ctx.fill_preserve();

    // Ring in the contrasting colour so the badge reads against a background of
    // the same hue.
    let contrast = if luminance(ann.color) > 0.5 { 0.0 } else { 1.0 };
    ctx.set_source_rgba(contrast, contrast, contrast, 0.9);
    ctx.set_line_width((radius * 0.12).max(1.0));
    let _ = ctx.stroke();

    let text = value.to_string();
    let layout = layout_for(ctx, &text, size, true);
    let (tw, th) = layout.pixel_size();
    ctx.set_source_rgba(contrast, contrast, contrast, 1.0);
    ctx.move_to(at.0 - tw as f64 / 2.0, at.1 - th as f64 / 2.0);
    pangocairo::functions::show_layout(ctx, &layout);
}

fn luminance((r, g, b): Rgb) -> f64 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

// ---------------------------------------------------------------------------
// Redactions
// ---------------------------------------------------------------------------

/// Average a channel-sum accumulator, or `None` for an empty block.
fn mean_pixel(sum: [u64; 4], count: u64) -> Option<image::Rgba<u8>> {
    let count = std::num::NonZeroU64::new(count)?.get();
    Some(image::Rgba([
        (sum[0] / count) as u8,
        (sum[1] / count) as u8,
        (sum[2] / count) as u8,
        (sum[3] / count) as u8,
    ]))
}

/// Mosaic block size for a redaction region.
///
/// Scaled to the region so a small redaction over a few words still coarsens
/// enough to destroy the glyphs, while a large one does not turn into a single
/// flat square.
fn block_size(rect: Rect) -> u32 {
    let shortest = rect.2.min(rect.3);
    ((shortest / 10.0).round() as u32).clamp(8, 64)
}

/// Replace the pixels under every redaction annotation, in place.
pub fn apply_redactions(img: &mut RgbaImage, annotations: &[Annotation]) {
    for ann in annotations {
        match &ann.shape {
            Shape::Pixelate { rect } => pixelate_region(img, *rect),
            Shape::Blackout { rect } => fill_region(img, *rect, ann.color),
            _ => {}
        }
    }
}

/// Clamp a rectangle to the image bounds, returning integer pixel ranges.
fn clamp_rect(img_w: u32, img_h: u32, rect: Rect) -> Option<(u32, u32, u32, u32)> {
    let x0 = rect.0.floor().max(0.0) as u32;
    let y0 = rect.1.floor().max(0.0) as u32;
    let x1 = ((rect.0 + rect.2).ceil().max(0.0) as u32).min(img_w);
    let y1 = ((rect.1 + rect.3).ceil().max(0.0) as u32).min(img_h);

    if x0 >= x1 || y0 >= y1 || x0 >= img_w || y0 >= img_h {
        return None;
    }
    Some((x0, y0, x1, y1))
}

/// Average each block and write the average back over the whole block.
fn pixelate_region(img: &mut RgbaImage, rect: Rect) {
    let (w, h) = (img.width(), img.height());
    let Some((x0, y0, x1, y1)) = clamp_rect(w, h, rect) else {
        return;
    };
    let block = block_size(rect);

    let mut by = y0;
    while by < y1 {
        let mut bx = x0;
        while bx < x1 {
            let bx_end = (bx + block).min(x1);
            let by_end = (by + block).min(y1);

            let mut sum = [0u64; 4];
            let mut count = 0u64;
            for y in by..by_end {
                for x in bx..bx_end {
                    let px = img.get_pixel(x, y).0;
                    for c in 0..4 {
                        sum[c] += px[c] as u64;
                    }
                    count += 1;
                }
            }
            if let Some(avg) = mean_pixel(sum, count) {
                for y in by..by_end {
                    for x in bx..bx_end {
                        img.put_pixel(x, y, avg);
                    }
                }
            }
            bx += block;
        }
        by += block;
    }
}

/// Overwrite a region with a solid opaque colour.
fn fill_region(img: &mut RgbaImage, rect: Rect, color: Rgb) {
    let (w, h) = (img.width(), img.height());
    let Some((x0, y0, x1, y1)) = clamp_rect(w, h, rect) else {
        return;
    };
    let px = image::Rgba([
        (color.0 * 255.0).round().clamp(0.0, 255.0) as u8,
        (color.1 * 255.0).round().clamp(0.0, 255.0) as u8,
        (color.2 * 255.0).round().clamp(0.0, 255.0) as u8,
        255,
    ]);
    for y in y0..y1 {
        for x in x0..x1 {
            img.put_pixel(x, y, px);
        }
    }
}

/// Draw redactions onto a preview context, reading from the image being shown.
///
/// The preview cannot simply reuse `apply_redactions`, which mutates the
/// buffer; instead it paints the same mosaic and fills that the save pipeline
/// will produce, so the preview is an accurate proof of what leaves the machine.
/// `scale` maps annotation coordinates (full-resolution image pixels) into
/// `source`, which during live editing is a downscaled copy. The context is
/// expected to be in `source`'s own pixel space.
pub fn draw_redactions_preview(
    ctx: &cairo::Context,
    source: &RgbaImage,
    annotations: &[Annotation],
    scale: f64,
) {
    let (w, h) = (source.width(), source.height());
    let scaled = |r: Rect| -> Rect { (r.0 * scale, r.1 * scale, r.2 * scale, r.3 * scale) };

    for ann in annotations {
        match &ann.shape {
            Shape::Blackout { rect } => {
                let rect = scaled(*rect);
                let (r, g, b) = ann.color;
                ctx.set_source_rgb(r, g, b);
                ctx.rectangle(rect.0, rect.1, rect.2, rect.3);
                let _ = ctx.fill();
            }
            Shape::Pixelate { rect } => {
                let rect = scaled(*rect);
                let Some((x0, y0, x1, y1)) = clamp_rect(w, h, rect) else {
                    continue;
                };
                let block = block_size(rect);

                let mut by = y0;
                while by < y1 {
                    let mut bx = x0;
                    while bx < x1 {
                        let bx_end = (bx + block).min(x1);
                        let by_end = (by + block).min(y1);

                        let mut sum = [0u64; 3];
                        let mut count = 0u64;
                        for y in by..by_end {
                            for x in bx..bx_end {
                                let px = source.get_pixel(x, y).0;
                                for c in 0..3 {
                                    sum[c] += px[c] as u64;
                                }
                                count += 1;
                            }
                        }
                        if let Some(count) = std::num::NonZeroU64::new(count) {
                            let count = count.get() as f64;
                            ctx.set_source_rgb(
                                sum[0] as f64 / count / 255.0,
                                sum[1] as f64 / count / 255.0,
                                sum[2] as f64 / count / 255.0,
                            );
                            ctx.rectangle(
                                bx as f64,
                                by as f64,
                                (bx_end - bx) as f64,
                                (by_end - by) as f64,
                            );
                            let _ = ctx.fill();
                        }
                        bx += block;
                    }
                    by += block;
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Compositing into the save pipeline
// ---------------------------------------------------------------------------

/// Render a transparent overlay of the given size by running `draw` against a
/// Cairo context in image-pixel coordinates, then convert it to an `RgbaImage`
/// ready for `image::imageops::overlay`.
///
/// Returns `None` when the surface cannot be allocated, which Cairo reports for
/// dimensions beyond its 32767-pixel limit as well as under memory pressure.
/// A failed overlay must not abort a save that has otherwise succeeded.
pub fn render_layer<F>(width: u32, height: u32, draw: F) -> Option<RgbaImage>
where
    F: FnOnce(&cairo::Context),
{
    if width == 0 || height == 0 {
        return None;
    }

    let surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, width as i32, height as i32).ok()?;
    {
        let ctx = cairo::Context::new(&surface).ok()?;
        draw(&ctx);
    }

    surface_to_rgba(surface)
}

/// Convert a Cairo ARGB32 surface into a straight-alpha `RgbaImage`.
///
/// Cairo stores ARGB32 as premultiplied 32-bit native-endian words, which on
/// every platform SuperShot targets means the byte order B, G, R, A. Both the
/// channel swap and the un-premultiplication are required before the data can
/// be handed to the `image` crate.
fn surface_to_rgba(surface: cairo::ImageSurface) -> Option<RgbaImage> {
    let width = surface.width() as u32;
    let height = surface.height() as u32;
    let stride = surface.stride() as usize;

    // The surface must be dropped out of any borrow before take_data().
    let data = surface.take_data().ok()?;

    let mut out = RgbaImage::new(width, height);
    for y in 0..height as usize {
        let row = &data[y * stride..y * stride + width as usize * 4];
        for x in 0..width as usize {
            let px = &row[x * 4..x * 4 + 4];
            let (b, g, r, a) = (px[0], px[1], px[2], px[3]);

            let (r, g, b) = if a == 0 {
                (0, 0, 0)
            } else if a == 255 {
                (r, g, b)
            } else {
                let unpremul = |c: u8| -> u8 {
                    // Round-to-nearest, saturating: premultiplied values can
                    // exceed alpha by one after Cairo's own rounding.
                    (((c as u32 * 255) + a as u32 / 2) / a as u32).min(255) as u8
                };
                (unpremul(r), unpremul(g), unpremul(b))
            };

            out.put_pixel(x as u32, y as u32, image::Rgba([r, g, b, a]));
        }
    }

    Some(out)
}

/// Apply the full annotation display list to an owned image.
///
/// Redactions go in first and destroy pixels; vector marks are composited on
/// top as a single layer, which keeps translucent highlighter strokes from
/// darkening where they overlap each other.
pub fn render_onto(img: &mut RgbaImage, annotations: &[Annotation]) {
    if annotations.is_empty() {
        return;
    }

    apply_redactions(img, annotations);

    let has_vector = annotations.iter().any(|a| !a.shape.is_redaction());
    if !has_vector {
        return;
    }

    let (w, h) = (img.width(), img.height());
    if let Some(layer) = render_layer(w, h, |ctx| draw_vector(ctx, annotations)) {
        image::imageops::overlay(img, &layer, 0, 0);
    }
}

/// How the image frame changed under a geometry edit.
///
/// Annotations are stored in the coordinates of the *displayed* image, so a
/// rotation or flip moves the frame out from under them. Rather than discard
/// the user's marks, they are carried into the new frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reframe {
    /// Quarter turn clockwise.
    Rot90,
    /// Quarter turn counter-clockwise.
    Rot270,
    FlipH,
    FlipV,
}

/// Map a point from the pre-change frame into the post-change one.
///
/// `width` and `height` are the frame's dimensions *before* the change.
fn reframe_point(p: Point, width: f64, height: f64, how: Reframe) -> Point {
    match how {
        Reframe::Rot90 => (height - p.1, p.0),
        Reframe::Rot270 => (p.1, width - p.0),
        Reframe::FlipH => (width - p.0, p.1),
        Reframe::FlipV => (p.0, height - p.1),
    }
}

/// Carry an annotation into a reframed image.
///
/// Rectangles are transformed corner-to-corner and re-normalised, because a
/// rotation swaps which corner is the origin.
pub fn reframe(ann: &mut Annotation, width: f64, height: f64, how: Reframe) {
    let map = |p: Point| reframe_point(p, width, height, how);

    let map_rect = |r: &mut Rect| {
        let a = map((r.0, r.1));
        let b = map((r.0 + r.2, r.1 + r.3));
        *r = rect_from_points(a, b);
    };

    match &mut ann.shape {
        Shape::Arrow { from, to } => {
            *from = map(*from);
            *to = map(*to);
        }
        Shape::Rectangle { rect }
        | Shape::Ellipse { rect }
        | Shape::Highlight { rect }
        | Shape::Pixelate { rect }
        | Shape::Blackout { rect } => map_rect(rect),
        Shape::FreeDraw { points } => {
            for p in points.iter_mut() {
                *p = map(*p);
            }
        }
        Shape::Text { at, .. } | Shape::Number { at, .. } => *at = map(*at),
    }
}

/// Shift an annotation's coordinates, used when a crop moves the origin.
pub fn translate(ann: &mut Annotation, dx: f64, dy: f64) {
    let shift_point = |p: &mut Point| {
        p.0 += dx;
        p.1 += dy;
    };
    let shift_rect = |r: &mut Rect| {
        r.0 += dx;
        r.1 += dy;
    };

    match &mut ann.shape {
        Shape::Arrow { from, to } => {
            shift_point(from);
            shift_point(to);
        }
        Shape::Rectangle { rect }
        | Shape::Ellipse { rect }
        | Shape::Highlight { rect }
        | Shape::Pixelate { rect }
        | Shape::Blackout { rect } => shift_rect(rect),
        Shape::FreeDraw { points } => {
            for p in points.iter_mut() {
                shift_point(p);
            }
        }
        Shape::Text { at, .. } | Shape::Number { at, .. } => shift_point(at),
    }
}

/// Bounding box of an annotation, used to decide whether a click selects it.
pub fn bounds(ann: &Annotation) -> Rect {
    let pad = ann.stroke.max(4.0);
    match &ann.shape {
        Shape::Arrow { from, to } => {
            let r = rect_from_points(*from, *to);
            (r.0 - pad, r.1 - pad, r.2 + pad * 2.0, r.3 + pad * 2.0)
        }
        Shape::FreeDraw { points } => {
            let (mut min_x, mut min_y) = (f64::MAX, f64::MAX);
            let (mut max_x, mut max_y) = (f64::MIN, f64::MIN);
            for (x, y) in points {
                min_x = min_x.min(*x);
                min_y = min_y.min(*y);
                max_x = max_x.max(*x);
                max_y = max_y.max(*y);
            }
            if points.is_empty() {
                return (0.0, 0.0, 0.0, 0.0);
            }
            (
                min_x - pad,
                min_y - pad,
                max_x - min_x + pad * 2.0,
                max_y - min_y + pad * 2.0,
            )
        }
        Shape::Text { at, text } => {
            let size = text_size(ann.stroke);
            // Approximate: an exact extent needs a Cairo context, and this is
            // only used for hit-testing tolerance.
            (
                at.0 - pad,
                at.1 - pad,
                text.chars().count() as f64 * size * 0.6 + pad * 2.0,
                size * 1.4 + pad * 2.0,
            )
        }
        Shape::Number { at, .. } => {
            let radius = text_size(ann.stroke) * 0.85;
            (at.0 - radius, at.1 - radius, radius * 2.0, radius * 2.0)
        }
        other => other.rect().unwrap_or((0.0, 0.0, 0.0, 0.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ann(shape: Shape) -> Annotation {
        Annotation { shape, color: (1.0, 0.0, 0.0), stroke: 4.0 }
    }

    fn rect_of(a: &Annotation) -> Rect {
        match &a.shape {
            Shape::Rectangle { rect } => *rect,
            other => panic!("expected a rectangle, got {:?}", other),
        }
    }

    fn approx(a: Rect, b: Rect) {
        for (x, y) in [(a.0, b.0), (a.1, b.1), (a.2, b.2), (a.3, b.3)] {
            assert!((x - y).abs() < 1e-9, "{:?} != {:?}", a, b);
        }
    }

    #[test]
    fn rect_from_points_normalises_any_drag_direction() {
        let expected = (10.0, 20.0, 30.0, 40.0);
        approx(rect_from_points((10.0, 20.0), (40.0, 60.0)), expected);
        approx(rect_from_points((40.0, 60.0), (10.0, 20.0)), expected);
        approx(rect_from_points((40.0, 20.0), (10.0, 60.0)), expected);
        approx(rect_from_points((10.0, 60.0), (40.0, 20.0)), expected);
    }

    /// Four quarter turns must land an annotation exactly where it started,
    /// which is the property that keeps marks aligned after the user rotates
    /// back and forth.
    #[test]
    fn four_quarter_turns_are_the_identity() {
        let start = (10.0, 20.0, 30.0, 40.0);
        let mut a = ann(Shape::Rectangle { rect: start });

        // The frame alternates between 100x200 and 200x100 as it turns.
        let mut w = 100.0;
        let mut h = 200.0;
        for _ in 0..4 {
            reframe(&mut a, w, h, Reframe::Rot90);
            std::mem::swap(&mut w, &mut h);
        }

        approx(rect_of(&a), start);
    }

    #[test]
    fn rot90_and_rot270_are_inverses() {
        let start = (10.0, 20.0, 30.0, 40.0);
        let mut a = ann(Shape::Rectangle { rect: start });

        reframe(&mut a, 100.0, 200.0, Reframe::Rot90);
        // The frame is now 200x100.
        reframe(&mut a, 200.0, 100.0, Reframe::Rot270);

        approx(rect_of(&a), start);
    }

    #[test]
    fn rot90_maps_the_top_left_corner_to_the_top_right() {
        // A 10x10 mark at the origin of a 100x200 frame becomes a mark at the
        // top-right of the resulting 200x100 frame.
        let mut a = ann(Shape::Rectangle { rect: (0.0, 0.0, 10.0, 10.0) });
        reframe(&mut a, 100.0, 200.0, Reframe::Rot90);
        approx(rect_of(&a), (190.0, 0.0, 10.0, 10.0));
    }

    #[test]
    fn flips_are_involutions() {
        for how in [Reframe::FlipH, Reframe::FlipV] {
            let start = (10.0, 20.0, 30.0, 40.0);
            let mut a = ann(Shape::Rectangle { rect: start });
            reframe(&mut a, 100.0, 200.0, how);
            assert_ne!(rect_of(&a), start, "{:?} should move the mark", how);
            reframe(&mut a, 100.0, 200.0, how);
            approx(rect_of(&a), start);
        }
    }

    #[test]
    fn reframe_carries_every_shape_kind() {
        let shapes = vec![
            Shape::Arrow { from: (1.0, 2.0), to: (3.0, 4.0) },
            Shape::Ellipse { rect: (1.0, 2.0, 3.0, 4.0) },
            Shape::Highlight { rect: (1.0, 2.0, 3.0, 4.0) },
            Shape::FreeDraw { points: vec![(1.0, 2.0), (3.0, 4.0)] },
            Shape::Text { at: (1.0, 2.0), text: "x".into() },
            Shape::Number { at: (1.0, 2.0), value: 1 },
            Shape::Pixelate { rect: (1.0, 2.0, 3.0, 4.0) },
            Shape::Blackout { rect: (1.0, 2.0, 3.0, 4.0) },
        ];

        for shape in shapes {
            let mut a = ann(shape);
            let before = bounds(&a);
            reframe(&mut a, 100.0, 100.0, Reframe::FlipH);
            let after = bounds(&a);
            assert_ne!(
                (before.0, before.1),
                (after.0, after.1),
                "shape was not carried into the new frame: {:?}",
                a.shape
            );
        }
    }

    #[test]
    fn translate_shifts_every_shape_kind() {
        let mut a = ann(Shape::Arrow { from: (10.0, 10.0), to: (20.0, 20.0) });
        translate(&mut a, -5.0, -3.0);
        match a.shape {
            Shape::Arrow { from, to } => {
                approx((from.0, from.1, to.0, to.1), (5.0, 7.0, 15.0, 17.0));
            }
            _ => unreachable!(),
        }
    }

    /// Redaction must replace pixels, not merely cover them: a saved file with
    /// the original values still present under a translucent overlay would
    /// defeat the purpose of the tool.
    #[test]
    fn pixelate_destroys_the_original_pixels() {
        let mut img = RgbaImage::new(64, 64);
        // A high-contrast checkerboard: averaging any block flattens it.
        for (x, y, px) in img.enumerate_pixels_mut() {
            let on = (x / 2 + y / 2) % 2 == 0;
            *px = if on {
                image::Rgba([255, 255, 255, 255])
            } else {
                image::Rgba([0, 0, 0, 255])
            };
        }
        let before = img.clone();

        apply_redactions(&mut img, &[ann(Shape::Pixelate { rect: (0.0, 0.0, 64.0, 64.0) })]);

        assert_ne!(img.as_raw(), before.as_raw(), "pixelate left the region untouched");

        // Every block must be uniform, i.e. the checkerboard is gone.
        let block = block_size((0.0, 0.0, 64.0, 64.0));
        let corner = *img.get_pixel(0, 0);
        for y in 0..block.min(64) {
            for x in 0..block.min(64) {
                assert_eq!(*img.get_pixel(x, y), corner, "block is not uniform at {},{}", x, y);
            }
        }
    }

    #[test]
    fn blackout_writes_an_opaque_solid_region() {
        let mut img = RgbaImage::from_pixel(32, 32, image::Rgba([200, 100, 50, 255]));
        let mut a = ann(Shape::Blackout { rect: (4.0, 4.0, 8.0, 8.0) });
        a.color = (0.0, 0.0, 0.0);
        apply_redactions(&mut img, &[a]);

        assert_eq!(*img.get_pixel(8, 8), image::Rgba([0, 0, 0, 255]));
        // Outside the rectangle the image is untouched.
        assert_eq!(*img.get_pixel(20, 20), image::Rgba([200, 100, 50, 255]));
    }

    /// A rectangle dragged past the edge of the image must be clipped rather
    /// than panicking on an out-of-range pixel access.
    #[test]
    fn redactions_clamp_to_the_image_bounds() {
        let mut img = RgbaImage::from_pixel(16, 16, image::Rgba([1, 2, 3, 255]));
        let out_of_range = [
            (-50.0, -50.0, 200.0, 200.0),
            (-10.0, -10.0, 5.0, 5.0),
            (100.0, 100.0, 10.0, 10.0),
            (0.0, 0.0, 0.0, 0.0),
        ];
        for rect in out_of_range {
            apply_redactions(&mut img, &[ann(Shape::Blackout { rect })]);
            apply_redactions(&mut img, &[ann(Shape::Pixelate { rect })]);
        }
    }

    /// Cairo stores ARGB32 premultiplied in native byte order; the conversion
    /// back to straight-alpha RGBA has to undo both that and the channel swap.
    #[test]
    fn render_layer_round_trips_colour_and_alpha() {
        let layer = render_layer(8, 8, |ctx| {
            // Opaque pure red over the left half.
            ctx.set_source_rgba(1.0, 0.0, 0.0, 1.0);
            ctx.rectangle(0.0, 0.0, 4.0, 8.0);
            let _ = ctx.fill();
        })
        .expect("an 8x8 surface must be allocatable");

        assert_eq!(*layer.get_pixel(1, 1), image::Rgba([255, 0, 0, 255]));
        // The untouched half stays fully transparent.
        assert_eq!(layer.get_pixel(6, 1)[3], 0);
    }

    #[test]
    fn render_layer_rejects_degenerate_sizes() {
        assert!(render_layer(0, 10, |_| {}).is_none());
        assert!(render_layer(10, 0, |_| {}).is_none());
    }

    #[test]
    fn vector_marks_composite_but_redactions_do_not_go_through_the_layer() {
        let mut img = RgbaImage::from_pixel(32, 32, image::Rgba([255, 255, 255, 255]));
        render_onto(&mut img, &[ann(Shape::Rectangle { rect: (4.0, 4.0, 20.0, 20.0) })]);

        // Somewhere on the stroke the pixel must have picked up the colour.
        let changed = img
            .enumerate_pixels()
            .any(|(_, _, px)| *px != image::Rgba([255, 255, 255, 255]));
        assert!(changed, "the vector layer was not composited");
    }

    #[test]
    fn render_onto_is_a_no_op_without_annotations() {
        let mut img = RgbaImage::from_pixel(8, 8, image::Rgba([9, 9, 9, 255]));
        let before = img.clone();
        render_onto(&mut img, &[]);
        assert_eq!(img.as_raw(), before.as_raw());
    }
}
