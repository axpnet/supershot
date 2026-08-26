// SuperShot - Preview, annotation and editing window
// Copyright (c) 2026 axpnet <https://github.com/axpnet>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Presents a captured screenshot with an annotation toolbar (arrow, rectangle,
// ellipse, highlighter, free draw, text, numbered step, pixelate, blackout),
// a crop tool, and an adjustments sidebar (rotate, flip, brightness, contrast,
// blur, sharpen, grayscale, invert).
//
// Model
// -----
// The window owns `source`: the full-resolution image as it currently stands,
// with every applied crop already baked in. It is the single source of truth
// for saving. Annotations are held as a display list in `source` pixel
// coordinates, and `edits` holds the non-destructive adjustments layered on
// top.
//
// This replaces an arrangement in which "Apply Crop" baked the crop into an
// in-memory pixbuf, reset the edit state and cleared the crop flag, while
// saving independently re-read the *original file* from disk and re-applied the
// (now empty) edit state. Cropping and then saving therefore wrote the full
// uncropped image and silently discarded every adjustment made before the crop.
//
// Rendering
// ---------
// Live editing runs against `preview_base`, a copy of `source` bounded to
// `PREVIEW_MAX_EDGE`, and slider input is debounced. Recomputation happens on a
// worker thread; the main loop only uploads the finished frame. Saving always
// re-runs the same edits against the full-resolution `source`.

use gtk4::prelude::*;
use gtk4::{glib, gio};
use libadwaita as adw;
use libadwaita::prelude::*;
use glib::subclass::types::ObjectSubclassIsExt;
use gettextrs::gettext;
use std::cell::{Cell, RefCell};
use image::RgbaImage;

use crate::annotate::{self, Annotation, Shape, Tool};
use crate::capture::{self, CaptureOptions, SaveSource, TempCapture};
use crate::editing::EditState;
use crate::window::SuperShotWindow;

const MIN_ZOOM: f64 = 0.25;
const MAX_ZOOM: f64 = 4.0;
const ZOOM_STEP: f64 = 0.25;

/// Delay between the last slider movement and the recomputation it triggers.
///
/// Long enough that dragging a slider across its range schedules one render
/// rather than one per motion event, short enough to feel immediate.
const EDIT_DEBOUNCE_MS: u32 = 120;

/// Sidebar width bounds, in pixels, honoured while it is docked.
const SIDEBAR_MIN_WIDTH: f64 = 190.0;
const SIDEBAR_MAX_WIDTH: f64 = 260.0;

/// Window width below which the sidebar becomes an overlay.
const COLLAPSE_BELOW: &str = "max-width: 640px";

/// The smallest size the preview window can render at. Everything inside
/// adapts or scrolls, so this is the only hard floor.
const MIN_WINDOW_WIDTH: i32 = 360;
const MIN_WINDOW_HEIGHT: i32 = 400;

/// Minimum pointer travel, in widget pixels, before a press is treated as a
/// drag rather than a click. Below this a drag gesture and a click gesture both
/// fire for the same interaction.
const DRAG_THRESHOLD: f64 = 3.0;

/// A restorable point in the window's edit history.
#[derive(Clone)]
pub struct Snapshot {
    annotations: Vec<Annotation>,
    edits: EditState,
    /// Present only for operations that replace the underlying pixels, i.e.
    /// applying a crop. Annotation changes share the current `source`, so
    /// storing a full-resolution copy for each of them would be wasteful.
    source: Option<RgbaImage>,
}

/// Cap on stored crop snapshots, each of which holds a full-resolution image.
const MAX_IMAGE_SNAPSHOTS: usize = 4;

/// Everything the render pipeline needs, gathered from live window state.
struct SaveJob {
    source: SaveSource,
    options: CaptureOptions,
    edits: EditState,
    annotations: Vec<Annotation>,
    crop: Option<(i32, i32, i32, i32)>,
}

mod imp {
    use super::*;
    use gtk4::subclass::prelude::*;
    use libadwaita::subclass::prelude::*;

    pub struct PreviewWindow {
        // --- Canvas ---
        pub drawing_area: gtk4::DrawingArea,
        pub scrolled: gtk4::ScrolledWindow,

        // --- Header ---
        pub save_btn: gtk4::Button,
        pub discard_btn: gtk4::Button,
        pub copy_btn: gtk4::Button,
        pub zoom_in_btn: gtk4::Button,
        pub zoom_out_btn: gtk4::Button,
        pub zoom_label: gtk4::Label,
        pub spinner: gtk4::Spinner,
        pub sidebar_btn: gtk4::ToggleButton,
        pub header: RefCell<Option<adw::HeaderBar>>,
        pub status: RefCell<Option<gtk4::Box>>,
        pub split: RefCell<Option<adw::OverlaySplitView>>,

        // --- Annotation toolbar ---
        pub stroke_scale: gtk4::Scale,
        pub text_entry: gtk4::Entry,
        pub undo_btn: gtk4::Button,
        pub redo_btn: gtk4::Button,
        pub crop_apply_btn: gtk4::Button,

        // --- Status ---
        pub res_label: gtk4::Label,
        pub hint_label: gtk4::Label,

        // --- Adjustment sidebar ---
        pub brightness_scale: gtk4::Scale,
        pub contrast_scale: gtk4::Scale,
        pub blur_scale: gtk4::Scale,
        pub sharpen_scale: gtk4::Scale,
        pub grayscale_switch: gtk4::Switch,
        pub invert_switch: gtk4::Switch,

        // --- Image model ---
        /// Full-resolution image with all applied crops baked in.
        pub source: RefCell<RgbaImage>,
        /// Bounded-size copy of `source` used for live editing.
        pub preview_base: RefCell<RgbaImage>,
        /// preview_base dimensions / source dimensions.
        pub preview_scale: Cell<f64>,
        /// `preview_base` with the current edits applied, ready to blit.
        pub edited_surface: RefCell<Option<cairo::ImageSurface>>,
        /// `preview_base` with the current edits applied, for redaction sampling.
        pub edited_preview: RefCell<Option<RgbaImage>>,
        pub edits: RefCell<EditState>,

        // --- Annotations ---
        pub annotations: RefCell<Vec<Annotation>>,
        pub active_tool: Cell<Tool>,
        pub color_index: Cell<usize>,
        pub next_number: Cell<u32>,
        /// Annotation currently being dragged out, drawn but not yet committed.
        pub in_progress: RefCell<Option<Annotation>>,

        pub undo_stack: RefCell<Vec<Snapshot>>,
        pub redo_stack: RefCell<Vec<Snapshot>>,

        // --- Interaction ---
        pub zoom: Cell<f64>,
        pub drag_start: Cell<(f64, f64)>,
        pub drag_end: Cell<(f64, f64)>,
        pub dragging: Cell<bool>,
        pub drag_moved: Cell<bool>,
        pub crop_rect: Cell<Option<(f64, f64, f64, f64)>>,
        pub free_points: RefCell<Vec<(f64, f64)>>,

        /// Pending debounced recompute, cancelled when superseded.
        pub recalc_source: RefCell<Option<glib::SourceId>>,
        /// Suppresses edit recalculation during programmatic widget resets.
        pub suppress_edits: Cell<bool>,
        pub saving: Cell<bool>,

        // --- Capture context ---
        pub options: RefCell<CaptureOptions>,
        pub uri: RefCell<String>,
        pub temp: RefCell<Option<TempCapture>>,
        pub main_window: RefCell<Option<SuperShotWindow>>,
        pub hold_guard: RefCell<Option<gio::ApplicationHoldGuard>>,
    }

    impl Default for PreviewWindow {
        fn default() -> Self {
            let scale = |lo: f64, hi: f64, step: f64| {
                gtk4::Scale::new(
                    gtk4::Orientation::Horizontal,
                    Some(&gtk4::Adjustment::new(0.0, lo, hi, step, step * 10.0, 0.0)),
                )
            };

            Self {
                drawing_area: gtk4::DrawingArea::new(),
                scrolled: gtk4::ScrolledWindow::new(),
                save_btn: gtk4::Button::new(),
                discard_btn: gtk4::Button::new(),
                copy_btn: gtk4::Button::new(),
                zoom_in_btn: gtk4::Button::new(),
                zoom_out_btn: gtk4::Button::new(),
                zoom_label: gtk4::Label::new(Some("100%")),
                spinner: gtk4::Spinner::new(),
                sidebar_btn: gtk4::ToggleButton::new(),
                header: RefCell::new(None),
                status: RefCell::new(None),
                split: RefCell::new(None),
                stroke_scale: gtk4::Scale::new(
                    gtk4::Orientation::Horizontal,
                    Some(&gtk4::Adjustment::new(4.0, 1.0, 24.0, 1.0, 4.0, 0.0)),
                ),
                text_entry: gtk4::Entry::new(),
                undo_btn: gtk4::Button::new(),
                redo_btn: gtk4::Button::new(),
                crop_apply_btn: gtk4::Button::new(),
                res_label: gtk4::Label::new(None),
                hint_label: gtk4::Label::new(None),
                brightness_scale: scale(-100.0, 100.0, 1.0),
                contrast_scale: scale(-100.0, 100.0, 1.0),
                blur_scale: scale(0.0, 10.0, 0.5),
                sharpen_scale: scale(0.0, 10.0, 0.5),
                grayscale_switch: gtk4::Switch::new(),
                invert_switch: gtk4::Switch::new(),
                source: RefCell::new(RgbaImage::new(1, 1)),
                preview_base: RefCell::new(RgbaImage::new(1, 1)),
                preview_scale: Cell::new(1.0),
                edited_surface: RefCell::new(None),
                edited_preview: RefCell::new(None),
                edits: RefCell::new(EditState::default()),
                annotations: RefCell::new(Vec::new()),
                active_tool: Cell::new(Tool::Crop),
                color_index: Cell::new(0),
                next_number: Cell::new(1),
                in_progress: RefCell::new(None),
                undo_stack: RefCell::new(Vec::new()),
                redo_stack: RefCell::new(Vec::new()),
                zoom: Cell::new(1.0),
                drag_start: Cell::new((0.0, 0.0)),
                drag_end: Cell::new((0.0, 0.0)),
                dragging: Cell::new(false),
                drag_moved: Cell::new(false),
                crop_rect: Cell::new(None),
                free_points: RefCell::new(Vec::new()),
                recalc_source: RefCell::new(None),
                suppress_edits: Cell::new(false),
                saving: Cell::new(false),
                options: RefCell::new(CaptureOptions::default()),
                uri: RefCell::new(String::new()),
                temp: RefCell::new(None),
                main_window: RefCell::new(None),
                hold_guard: RefCell::new(None),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PreviewWindow {
        const NAME: &'static str = "SuperShotPreviewWindow";
        type Type = super::PreviewWindow;
        type ParentType = adw::Window;
    }

    impl ObjectImpl for PreviewWindow {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            let header = self.build_header(&obj);
            let sidebar = self.build_sidebar(&obj);
            let status = self.build_status();
            self.header.replace(Some(header.clone()));
            self.status.replace(Some(status.clone()));

            self.scrolled.set_hexpand(true);
            self.scrolled.set_vexpand(true);
            self.scrolled.set_child(Some(&self.drawing_area));
            // Keep the canvas pinned to the top-left so widget coordinates map
            // linearly onto image coordinates at any window size.
            self.drawing_area.set_halign(gtk4::Align::Start);
            self.drawing_area.set_valign(gtk4::Align::Start);

            // AdwOverlaySplitView rather than a plain box: the sidebar keeps a
            // natural width when there is room, and collapses into an overlay
            // when there is not, so neither the tools nor the canvas can impose
            // a minimum size on the window.
            let split = adw::OverlaySplitView::new();
            split.set_sidebar(Some(&sidebar));
            split.set_content(Some(&self.scrolled));
            split.set_min_sidebar_width(SIDEBAR_MIN_WIDTH);
            split.set_max_sidebar_width(SIDEBAR_MAX_WIDTH);
            split.set_sidebar_width_fraction(0.22);
            split.set_vexpand(true);
            self.split.replace(Some(split.clone()));

            // The toggle only appears once the sidebar is overlaid; while it is
            // docked there is nothing to reveal.
            self.sidebar_btn.set_icon_name("sidebar-show-symbolic");
            self.sidebar_btn.set_tooltip_text(Some(&gettext("Show tools")));
            self.sidebar_btn.add_css_class("flat");
            self.sidebar_btn.set_visible(false);
            header.pack_start(&self.sidebar_btn);
            split
                .bind_property("show-sidebar", &self.sidebar_btn, "active")
                .sync_create()
                .bidirectional()
                .build();
            split
                .bind_property("collapsed", &self.sidebar_btn, "visible")
                .sync_create()
                .build();

            let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
            vbox.append(&header);
            vbox.append(&split);
            vbox.append(&status);

            obj.set_content(Some(&vbox));
            obj.set_title(Some(&gettext("Preview")));
            // A window using breakpoints must declare the smallest size it can
            // render at. Kept deliberately small: everything below adapts, and
            // this is the only hard floor in the window.
            obj.set_size_request(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT);

            // Below this width the sidebar overlays the canvas instead of
            // sitting beside it.
            match adw::BreakpointCondition::parse(COLLAPSE_BELOW) {
                Ok(condition) => {
                    let breakpoint = adw::Breakpoint::new(condition);
                    breakpoint.add_setter(&split, "collapsed", Some(&true.into()));
                    obj.add_breakpoint(breakpoint);
                }
                Err(e) => eprintln!("preview: invalid breakpoint condition: {e}"),
            }

            self.connect_canvas(&obj);
            self.connect_shortcuts(&obj);

            let weak = obj.downgrade();
            self.drawing_area.set_draw_func(move |_area, ctx, w, h| {
                if let Some(win) = weak.upgrade() {
                    win.draw(ctx, w, h);
                }
            });

            let weak = obj.downgrade();
            obj.connect_close_request(move |_| {
                if let Some(win) = weak.upgrade() {
                    win.do_discard();
                }
                glib::Propagation::Stop
            });
        }
    }

    impl PreviewWindow {
        fn build_header(&self, obj: &super::PreviewWindow) -> adw::HeaderBar {
            let header = adw::HeaderBar::new();

            self.discard_btn.set_label(&gettext("Discard"));
            self.discard_btn.add_css_class("destructive-action");
            header.pack_start(&self.discard_btn);

            self.save_btn.set_label(&gettext("Save"));
            self.save_btn.add_css_class("suggested-action");
            header.pack_end(&self.save_btn);

            self.copy_btn.set_icon_name("edit-copy-symbolic");
            self.copy_btn.set_tooltip_text(Some(&gettext("Copy to clipboard")));
            self.copy_btn.add_css_class("flat");
            header.pack_end(&self.copy_btn);

            // The header carries only actions. Its title is suppressed because
            // AdwHeaderBar reserves symmetric space around one, which alone
            // accounted for most of the window's minimum width; the window
            // title is still shown by the compositor. Zoom lives in the status
            // bar, which ellipsizes.
            header.set_show_title(false);
            self.spinner.set_visible(false);
            header.pack_end(&self.spinner);

            let weak = obj.downgrade();
            self.save_btn.connect_clicked(move |_| {
                if let Some(w) = weak.upgrade() { w.do_save(); }
            });
            let weak = obj.downgrade();
            self.discard_btn.connect_clicked(move |_| {
                if let Some(w) = weak.upgrade() { w.do_discard(); }
            });
            let weak = obj.downgrade();
            self.copy_btn.connect_clicked(move |_| {
                if let Some(w) = weak.upgrade() { w.do_copy_only(); }
            });
            header
        }

        /// The tool sidebar.
        ///
        /// Laid out vertically rather than as a top toolbar: ten tools, eight
        /// colour swatches, a thickness slider and a text field in one row
        /// demanded a minimum window width of over 1600 px, which forced the
        /// window wider than most captures and left the canvas floating in
        /// empty space.
        ///
        /// Tools are labelled rather than iconised on purpose. There are no
        /// icon names in the freedesktop naming spec for "arrow annotation" or
        /// "pixelate", so any icon chosen here would resolve to a broken-image
        /// placeholder under the many icon themes shipped by KDE, XFCE, MATE
        /// and Cinnamon users. Short translated labels work everywhere.
        fn build_sidebar(&self, obj: &super::PreviewWindow) -> gtk4::ScrolledWindow {
            let column = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
            column.set_margin_top(10);
            column.set_margin_bottom(10);
            column.set_margin_start(10);
            column.set_margin_end(10);

            column.append(&heading(gettext("Tools")));
            column.append(&self.build_tool_grid(obj));

            let crop_caption = gtk4::Label::new(Some(&gettext("Apply Crop")));
            crop_caption.set_ellipsize(pango::EllipsizeMode::End);
            crop_caption.set_max_width_chars(14);
            self.crop_apply_btn.set_child(Some(&crop_caption));
            self.crop_apply_btn.set_tooltip_text(Some(&gettext("Apply Crop")));
            self.crop_apply_btn.add_css_class("suggested-action");
            self.crop_apply_btn.set_sensitive(false);
            let weak = obj.downgrade();
            self.crop_apply_btn.connect_clicked(move |_| {
                if let Some(w) = weak.upgrade() { w.apply_crop(); }
            });
            column.append(&self.crop_apply_btn);

            let history = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
            history.add_css_class("linked");
            history.set_homogeneous(true);
            self.undo_btn.set_icon_name("edit-undo-symbolic");
            self.undo_btn.set_tooltip_text(Some(&gettext("Undo")));
            self.undo_btn.set_sensitive(false);
            self.undo_btn.set_hexpand(true);
            self.redo_btn.set_icon_name("edit-redo-symbolic");
            self.redo_btn.set_tooltip_text(Some(&gettext("Redo")));
            self.redo_btn.set_sensitive(false);
            self.redo_btn.set_hexpand(true);
            history.append(&self.undo_btn);
            history.append(&self.redo_btn);
            column.append(&history);

            let weak = obj.downgrade();
            self.undo_btn.connect_clicked(move |_| {
                if let Some(w) = weak.upgrade() { w.undo(); }
            });
            let weak = obj.downgrade();
            self.redo_btn.connect_clicked(move |_| {
                if let Some(w) = weak.upgrade() { w.redo(); }
            });

            column.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

            column.append(&heading(gettext("Colour")));
            column.append(&self.build_colour_grid(obj));

            column.append(&heading(gettext("Thickness")));
            self.stroke_scale.set_draw_value(false);
            self.stroke_scale.set_hexpand(true);
            column.append(&self.stroke_scale);

            self.text_entry.set_placeholder_text(Some(&gettext("Label text")));
            self.text_entry.set_max_length(256);
            self.text_entry.set_sensitive(false);
            column.append(&self.text_entry);

            column.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

            // Adjustments are folded away by default: they are the less common
            // operation now that annotation is the point of this window.
            let expander = gtk4::Expander::new(None);
            expander.set_label_widget(Some(&heading(gettext("Adjustments"))));
            expander.set_child(Some(&self.build_adjustments(obj)));
            column.append(&expander);

            let scrolled = gtk4::ScrolledWindow::new();
            // Vertical scrolling only: the column is laid out to fit the
            // sidebar's width, and a short window must scroll the tools rather
            // than stretch the window to fit them.
            scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
            scrolled.set_child(Some(&column));
            // No width-request: the split view governs the width, and a request
            // here would become a hard minimum on the whole window.
            scrolled.set_hexpand(false);
            scrolled.set_vexpand(true);
            scrolled
        }

        fn build_tool_grid(&self, obj: &super::PreviewWindow) -> gtk4::Grid {
            let tools: &[(Tool, String, String)] = &[
                (Tool::Crop, gettext("Crop"), gettext("Drag a region, then click inside it to crop")),
                (Tool::Arrow, gettext("Arrow"), gettext("Drag to draw an arrow")),
                (Tool::Rectangle, gettext("Box"), gettext("Drag to draw a rectangle")),
                (Tool::Ellipse, gettext("Circle"), gettext("Drag to draw an ellipse")),
                (Tool::Highlighter, gettext("Marker"), gettext("Drag to highlight a region")),
                (Tool::FreeDraw, gettext("Pen"), gettext("Draw freehand")),
                (Tool::Text, gettext("Text"), gettext("Type in the field, then click to place the label")),
                (Tool::Number, gettext("Step"), gettext("Click to drop a numbered step marker")),
                (Tool::Pixelate, gettext("Pixelate"), gettext("Drag over sensitive data to destroy it")),
                (Tool::Blackout, gettext("Redact"), gettext("Drag to cover a region with solid colour")),
            ];

            let grid = gtk4::Grid::new();
            grid.set_row_spacing(4);
            grid.set_column_spacing(4);
            grid.set_column_homogeneous(true);

            let mut leader: Option<gtk4::ToggleButton> = None;

            for (index, (tool, label, tip)) in tools.iter().enumerate() {
                // An explicit ellipsizing label instead of `with_label`: the
                // button would otherwise demand the full width of its longest
                // translation, and the widest string in any of the fourteen
                // languages would set the sidebar's minimum width for everyone.
                let caption = gtk4::Label::new(Some(label));
                caption.set_ellipsize(pango::EllipsizeMode::End);
                caption.set_max_width_chars(8);

                let btn = gtk4::ToggleButton::new();
                btn.set_child(Some(&caption));
                btn.set_tooltip_text(Some(tip));
                btn.set_hexpand(true);

                match &leader {
                    None => {
                        btn.set_active(true);
                        leader = Some(btn.clone());
                    }
                    Some(first) => btn.set_group(Some(first)),
                }

                let weak = obj.downgrade();
                let tool = *tool;
                btn.connect_toggled(move |b| {
                    if b.is_active() {
                        if let Some(w) = weak.upgrade() {
                            w.set_tool(tool);
                        }
                    }
                });

                grid.attach(&btn, (index % 2) as i32, (index / 2) as i32, 1, 1);
            }

            grid
        }

        fn build_colour_grid(&self, obj: &super::PreviewWindow) -> gtk4::Grid {
            // One provider defines a class per palette entry. GTK 4.10 deprecated
            // per-widget style contexts, so the colours are declared once for
            // the display and applied by class name.
            install_swatch_styles();

            let grid = gtk4::Grid::new();
            grid.set_row_spacing(4);
            grid.set_column_spacing(4);
            grid.set_halign(gtk4::Align::Start);

            let mut leader: Option<gtk4::ToggleButton> = None;

            for (index, (name, _)) in annotate::PALETTE.iter().enumerate() {
                let btn = gtk4::ToggleButton::new();
                btn.set_size_request(28, 28);
                btn.set_tooltip_text(Some(name));
                btn.add_css_class("circular");
                btn.add_css_class(&format!("supershot-swatch-{}", index));

                match &leader {
                    None => {
                        btn.set_active(true);
                        leader = Some(btn.clone());
                    }
                    Some(first) => btn.set_group(Some(first)),
                }

                let weak = obj.downgrade();
                btn.connect_toggled(move |b| {
                    if b.is_active() {
                        if let Some(w) = weak.upgrade() {
                            w.imp().color_index.set(index);
                        }
                    }
                });

                grid.attach(&btn, (index % 4) as i32, (index / 4) as i32, 1, 1);
            }

            grid
        }

        fn build_status(&self) -> gtk4::Box {
            // Connected here because the buttons now live in this bar.
            let obj = self.obj();
            let weak = obj.downgrade();
            self.zoom_in_btn.connect_clicked(move |_| {
                if let Some(w) = weak.upgrade() { w.adjust_zoom(ZOOM_STEP); }
            });
            let weak = obj.downgrade();
            self.zoom_out_btn.connect_clicked(move |_| {
                if let Some(w) = weak.upgrade() { w.adjust_zoom(-ZOOM_STEP); }
            });

            let bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
            bar.set_margin_start(10);
            bar.set_margin_end(10);
            bar.set_margin_bottom(6);

            self.res_label.add_css_class("caption");
            self.res_label.add_css_class("dim-label");
            self.res_label.set_ellipsize(pango::EllipsizeMode::End);
            bar.append(&self.res_label);

            self.hint_label.add_css_class("caption");
            self.hint_label.add_css_class("dim-label");
            self.hint_label.set_hexpand(true);
            self.hint_label.set_halign(gtk4::Align::End);
            self.hint_label.set_ellipsize(pango::EllipsizeMode::End);
            bar.append(&self.hint_label);

            let zoom_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
            self.zoom_out_btn.set_icon_name("zoom-out-symbolic");
            self.zoom_out_btn.set_tooltip_text(Some(&gettext("Zoom out")));
            self.zoom_out_btn.add_css_class("flat");
            self.zoom_out_btn.add_css_class("circular");
            self.zoom_in_btn.set_icon_name("zoom-in-symbolic");
            self.zoom_in_btn.set_tooltip_text(Some(&gettext("Zoom in")));
            self.zoom_in_btn.add_css_class("flat");
            self.zoom_in_btn.add_css_class("circular");
            self.zoom_label.add_css_class("caption");
            self.zoom_label.add_css_class("numeric");
            self.zoom_label.set_width_chars(5);
            zoom_box.append(&self.zoom_out_btn);
            zoom_box.append(&self.zoom_label);
            zoom_box.append(&self.zoom_in_btn);
            bar.append(&zoom_box);

            bar
        }

        fn build_adjustments(&self, obj: &super::PreviewWindow) -> gtk4::Box {
            let box_ = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
            box_.set_margin_top(8);

            let geo = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
            geo.set_halign(gtk4::Align::Center);

            let make = |icon: &str, tip: String| {
                let b = gtk4::Button::from_icon_name(icon);
                b.set_tooltip_text(Some(&tip));
                b.add_css_class("flat");
                b
            };
            let rotate_ccw = make("object-rotate-left-symbolic", gettext("Rotate left"));
            let rotate_cw = make("object-rotate-right-symbolic", gettext("Rotate right"));
            let flip_h = make("object-flip-horizontal-symbolic", gettext("Flip horizontally"));
            let flip_v = make("object-flip-vertical-symbolic", gettext("Flip vertically"));

            for b in [&rotate_ccw, &rotate_cw, &flip_h, &flip_v] {
                geo.append(b);
            }
            box_.append(&geo);

            let weak = obj.downgrade();
            rotate_ccw.connect_clicked(move |_| {
                if let Some(w) = weak.upgrade() { w.rotate(-90); }
            });
            let weak = obj.downgrade();
            rotate_cw.connect_clicked(move |_| {
                if let Some(w) = weak.upgrade() { w.rotate(90); }
            });
            let weak = obj.downgrade();
            flip_h.connect_clicked(move |_| {
                if let Some(w) = weak.upgrade() { w.toggle_flip(true); }
            });
            let weak = obj.downgrade();
            flip_v.connect_clicked(move |_| {
                if let Some(w) = weak.upgrade() { w.toggle_flip(false); }
            });

            // The slider is identified by an Adjustment field rather than by its
            // label text. Dispatching on the visible English string, as the
            // previous implementation did, breaks the moment the label is
            // translated — which is exactly what this release does.
            self.slider_row(&box_, gettext("Brightness"), &self.brightness_scale, obj, Field::Brightness);
            self.slider_row(&box_, gettext("Contrast"), &self.contrast_scale, obj, Field::Contrast);
            self.slider_row(&box_, gettext("Blur"), &self.blur_scale, obj, Field::Blur);
            self.slider_row(&box_, gettext("Sharpen"), &self.sharpen_scale, obj, Field::Sharpen);

            let switch_row = |label: String, sw: &gtk4::Switch| {
                let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
                let l = gtk4::Label::new(Some(&label));
                l.set_hexpand(true);
                l.set_halign(gtk4::Align::Start);
                row.append(&l);
                sw.set_halign(gtk4::Align::End);
                row.append(sw);
                row
            };
            box_.append(&switch_row(gettext("Grayscale"), &self.grayscale_switch));
            box_.append(&switch_row(gettext("Invert"), &self.invert_switch));

            let weak = obj.downgrade();
            self.grayscale_switch.connect_state_set(move |_, state| {
                if let Some(w) = weak.upgrade() {
                    if !w.imp().suppress_edits.get() {
                        w.imp().edits.borrow_mut().grayscale = state;
                        w.schedule_recalc();
                    }
                }
                glib::Propagation::Proceed
            });
            let weak = obj.downgrade();
            self.invert_switch.connect_state_set(move |_, state| {
                if let Some(w) = weak.upgrade() {
                    if !w.imp().suppress_edits.get() {
                        w.imp().edits.borrow_mut().invert = state;
                        w.schedule_recalc();
                    }
                }
                glib::Propagation::Proceed
            });

            let reset = gtk4::Button::with_label(&gettext("Reset Adjustments"));
            reset.add_css_class("flat");
            let weak = obj.downgrade();
            reset.connect_clicked(move |_| {
                if let Some(w) = weak.upgrade() { w.reset_edits(); }
            });
            box_.append(&reset);

            box_
        }

        fn slider_row(
            &self,
            parent: &gtk4::Box,
            label: String,
            scale: &gtk4::Scale,
            obj: &super::PreviewWindow,
            field: Field,
        ) {
            let row = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
            let l = gtk4::Label::new(Some(&label));
            l.set_halign(gtk4::Align::Start);
            l.add_css_class("caption");
            row.append(&l);

            scale.set_draw_value(true);
            scale.set_value_pos(gtk4::PositionType::Right);
            scale.set_hexpand(true);
            row.append(scale);
            parent.append(&row);

            let weak = obj.downgrade();
            scale.connect_value_changed(move |s| {
                let Some(w) = weak.upgrade() else { return };
                if w.imp().suppress_edits.get() {
                    return;
                }
                let val = s.value();
                {
                    let mut e = w.imp().edits.borrow_mut();
                    match field {
                        Field::Brightness => e.brightness = val as i32,
                        Field::Contrast => e.contrast = val as i32,
                        Field::Blur => e.blur = val as f32,
                        Field::Sharpen => e.sharpen = val as f32,
                    }
                }
                w.schedule_recalc();
            });
        }

        fn connect_canvas(&self, obj: &super::PreviewWindow) {
            let drag = gtk4::GestureDrag::new();

            let weak = obj.downgrade();
            drag.connect_drag_begin(move |_, x, y| {
                if let Some(w) = weak.upgrade() {
                    w.on_drag_begin(x, y);
                }
            });
            let weak = obj.downgrade();
            drag.connect_drag_update(move |g, dx, dy| {
                if let Some(w) = weak.upgrade() {
                    if let Some((sx, sy)) = g.start_point() {
                        w.on_drag_update(sx + dx, sy + dy, dx.hypot(dy));
                    }
                }
            });
            let weak = obj.downgrade();
            drag.connect_drag_end(move |g, dx, dy| {
                if let Some(w) = weak.upgrade() {
                    if let Some((sx, sy)) = g.start_point() {
                        w.on_drag_end(sx + dx, sy + dy);
                    }
                }
            });
            self.drawing_area.add_controller(drag);

            let click = gtk4::GestureClick::new();
            let weak = obj.downgrade();
            click.connect_released(move |_, _, x, y| {
                if let Some(w) = weak.upgrade() {
                    w.on_click(x, y);
                }
            });
            self.drawing_area.add_controller(click);
        }

        fn connect_shortcuts(&self, obj: &super::PreviewWindow) {
            let keys = gtk4::EventControllerKey::new();
            let weak = obj.downgrade();
            keys.connect_key_pressed(move |_, key, _, state| {
                let Some(w) = weak.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                let ctrl = state.contains(gtk4::gdk::ModifierType::CONTROL_MASK);
                let shift = state.contains(gtk4::gdk::ModifierType::SHIFT_MASK);

                match key {
                    gtk4::gdk::Key::z if ctrl && !shift => { w.undo(); }
                    gtk4::gdk::Key::z if ctrl && shift => { w.redo(); }
                    gtk4::gdk::Key::y if ctrl => { w.redo(); }
                    gtk4::gdk::Key::s if ctrl => { w.do_save(); }
                    gtk4::gdk::Key::c if ctrl => { w.do_copy_only(); }
                    gtk4::gdk::Key::Escape => { w.cancel_pending(); }
                    gtk4::gdk::Key::Return if ctrl => { w.do_save(); }
                    _ => return glib::Propagation::Proceed,
                }
                glib::Propagation::Stop
            });
            obj.add_controller(keys);
        }
    }

    /// A small bold section label for the sidebar.
    fn heading(text: String) -> gtk4::Label {
        let label = gtk4::Label::new(Some(&text));
        label.add_css_class("heading");
        label.set_halign(gtk4::Align::Start);
        label
    }

    /// Which `EditState` field a slider drives.
    #[derive(Clone, Copy)]
    pub enum Field {
        Brightness,
        Contrast,
        Blur,
        Sharpen,
    }

    impl WidgetImpl for PreviewWindow {}
    impl WindowImpl for PreviewWindow {}
    impl AdwWindowImpl for PreviewWindow {}
}

glib::wrapper! {
    pub struct PreviewWindow(ObjectSubclass<imp::PreviewWindow>)
        @extends gtk4::Widget, gtk4::Window, adw::Window,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget,
                    gtk4::Native, gtk4::Root, gtk4::ShortcutManager;
}

impl PreviewWindow {
    /// Create and present the preview window for a captured screenshot.
    pub fn present_for(
        uri: String,
        temp: Option<TempCapture>,
        options: CaptureOptions,
        main_window: SuperShotWindow,
        hold: Option<gio::ApplicationHoldGuard>,
    ) {
        let win: Self = glib::Object::new();

        match crate::editing::load(&uri) {
            Ok(img) => win.set_source(img, true),
            Err(e) => {
                // Nothing to preview; report it through the main window rather
                // than presenting an empty canvas the user cannot act on.
                capture::show_error_dialog(
                    &main_window,
                    &gettext("Preview Failed"),
                    &format!("{}\n\n{}", gettext("The capture could not be opened."), e),
                );
                main_window.set_capture_sensitive(true);
                gtk4::prelude::WidgetExt::set_visible(&main_window, true);
                gtk4::prelude::GtkWindowExt::present(&main_window);
                drop(hold);
                return;
            }
        }

        let imp = win.imp();
        imp.uri.replace(uri);
        imp.temp.replace(temp);
        imp.options.replace(options);
        imp.main_window.replace(Some(main_window));
        imp.hold_guard.replace(hold);

        win.update_hint(&gettext("Pick a tool, mark up the shot, then Save or Copy."));
        win.present();

        if let Some(spec) = std::env::var_os("SUPERSHOT_DEBUG_LAYOUT") {
            // "WIDTHxHEIGHT" resizes the window before reporting, so the
            // adaptive behaviour can be observed at a chosen size.
            if let Some((w_str, h_str)) = spec.to_string_lossy().split_once('x') {
                if let (Ok(w), Ok(h)) = (w_str.trim().parse(), h_str.trim().parse()) {
                    win.set_default_size(w, h);
                    win.set_maximized(false);
                }
            }
            let w = win.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(1200), move || {
                w.dump_layout();
                if let Some(app) = gio::Application::default() {
                    app.quit();
                }
            });
        }
    }

    /// Report the geometry of the canvas chain.
    ///
    /// Enabled with `SUPERSHOT_DEBUG_LAYOUT`; set it to `WIDTHxHEIGHT` to
    /// resize the window first and observe how the layout adapts.
    fn dump_layout(&self) {
        let imp = self.imp();
        let root = self.clone().upcast::<gtk4::Widget>();

        let show = |name: &str, w: &gtk4::Widget| {
            let origin = w
                .compute_bounds(&root)
                .map(|b| (b.x().round() as i32, b.y().round() as i32))
                .unwrap_or((-1, -1));
            let (min, nat, _, _) = w.measure(gtk4::Orientation::Horizontal, -1);
            println!(
                "{:<28} x={:<5} y={:<5} w={:<5} h={:<5}  width(min={} nat={})",
                name, origin.0, origin.1, w.width(), w.height(), min, nat
            );
        };

        println!("--- layout ---");
        show("window", &root);
        if let Some(h) = imp.header.borrow().as_ref() {
            show("header", h.upcast_ref::<gtk4::Widget>());
        }
        if let Some(b) = imp.status.borrow().as_ref() {
            show("status", b.upcast_ref::<gtk4::Widget>());
        }
        show("sidebar canvas", imp.scrolled.upcast_ref::<gtk4::Widget>());
        show("drawing_area", imp.drawing_area.upcast_ref::<gtk4::Widget>());

        if let Some(split) = imp.split.borrow().as_ref() {
            println!(
                "split collapsed={}  show_sidebar={}  sidebar_btn visible={}",
                split.is_collapsed(),
                split.shows_sidebar(),
                imp.sidebar_btn.is_visible()
            );
            if let Some(sidebar) = split.sidebar() {
                show("sidebar", &sidebar);
            }
        }
        println!("--------------");
    }

    /// Install a new full-resolution image, rebuilding the preview pyramid.
    fn set_source(&self, img: RgbaImage, size_window: bool) {
        let imp = self.imp();
        let (w, h) = (img.width(), img.height());

        let (base, scale) = crate::editing::downscale_for_preview(&img);
        imp.preview_scale.set(scale);
        imp.preview_base.replace(base);
        imp.source.replace(img);

        if size_window {
            // Fit the window to the capture, but never larger than a common
            // laptop panel; oversized captures open maximised instead.
            if w > 1920 || h > 1080 {
                self.set_default_size(1400, 900);
                self.set_maximized(true);
            } else {
                self.set_default_size(
                    (w as i32 + 260).clamp(760, 1600),
                    (h as i32 + 190).clamp(520, 1000),
                );
            }
        }

        self.recalc_now();
    }

    // --- History -----------------------------------------------------------

    fn snapshot(&self, include_source: bool) -> Snapshot {
        let imp = self.imp();
        Snapshot {
            annotations: imp.annotations.borrow().clone(),
            edits: imp.edits.borrow().clone(),
            source: include_source.then(|| imp.source.borrow().clone()),
        }
    }

    fn push_undo(&self, include_source: bool) {
        let imp = self.imp();
        let mut stack = imp.undo_stack.borrow_mut();
        stack.push(self.snapshot(include_source));

        // Bound the memory held by full-resolution crop snapshots by dropping
        // the oldest one that carries an image, rather than truncating history
        // wholesale and losing cheap annotation steps with it.
        while stack.iter().filter(|s| s.source.is_some()).count() > MAX_IMAGE_SNAPSHOTS {
            if let Some(pos) = stack.iter().position(|s| s.source.is_some()) {
                stack[pos].source = None;
            } else {
                break;
            }
        }
        drop(stack);

        imp.redo_stack.borrow_mut().clear();
        self.update_history_buttons();
    }

    fn restore(&self, snap: Snapshot) {
        let imp = self.imp();
        imp.annotations.replace(snap.annotations);
        imp.in_progress.replace(None);
        imp.crop_rect.set(None);

        let edits = snap.edits.clone();
        self.apply_edit_state_to_widgets(&edits);
        imp.edits.replace(edits);

        match snap.source {
            Some(img) => self.set_source(img, false),
            None => self.recalc_now(),
        }
        self.update_history_buttons();
        self.update_status();
    }

    fn undo(&self) {
        let imp = self.imp();
        let Some(snap) = imp.undo_stack.borrow_mut().pop() else {
            return;
        };
        // A crop snapshot restores an earlier image, so the state we are leaving
        // has to carry the current image for redo to be able to return to it.
        let include_source = snap.source.is_some();
        imp.redo_stack.borrow_mut().push(self.snapshot(include_source));
        self.restore(snap);
    }

    fn redo(&self) {
        let imp = self.imp();
        let Some(snap) = imp.redo_stack.borrow_mut().pop() else {
            return;
        };
        let include_source = snap.source.is_some();
        imp.undo_stack.borrow_mut().push(self.snapshot(include_source));
        self.restore(snap);
    }

    fn update_history_buttons(&self) {
        let imp = self.imp();
        imp.undo_btn.set_sensitive(!imp.undo_stack.borrow().is_empty());
        imp.redo_btn.set_sensitive(!imp.redo_stack.borrow().is_empty());
    }

    // --- Edits -------------------------------------------------------------

    fn apply_edit_state_to_widgets(&self, edits: &EditState) {
        let imp = self.imp();
        imp.suppress_edits.set(true);
        imp.brightness_scale.set_value(edits.brightness as f64);
        imp.contrast_scale.set_value(edits.contrast as f64);
        imp.blur_scale.set_value(edits.blur as f64);
        imp.sharpen_scale.set_value(edits.sharpen as f64);
        imp.grayscale_switch.set_active(edits.grayscale);
        imp.invert_switch.set_active(edits.invert);
        imp.suppress_edits.set(false);
    }

    /// Rotate by a quarter turn, carrying the annotations with the image.
    ///
    /// `apply_edits` rotates first and flips afterwards, so when exactly one
    /// flip is active the reflection conjugates the rotation and reverses its
    /// apparent direction on screen. The annotations have to follow what the
    /// user sees, not the raw rotation value.
    fn rotate(&self, degrees: i32) {
        self.push_undo(false);
        let imp = self.imp();

        let (w, h) = self.output_size();
        let (flip_h, flip_v) = {
            let e = imp.edits.borrow();
            (e.flip_h, e.flip_v)
        };

        let clockwise = degrees > 0;
        let apparent_clockwise = if flip_h == flip_v { clockwise } else { !clockwise };
        let how = if apparent_clockwise {
            annotate::Reframe::Rot90
        } else {
            annotate::Reframe::Rot270
        };

        for ann in imp.annotations.borrow_mut().iter_mut() {
            annotate::reframe(ann, w as f64, h as f64, how);
        }

        {
            let mut e = imp.edits.borrow_mut();
            // rem_euclid handles negative degrees without a +360 correction and
            // cannot overflow for any i32 input.
            e.rotation = (e.rotation as i32 + degrees).rem_euclid(360) as u32;
        }

        imp.crop_rect.set(None);
        imp.crop_apply_btn.set_sensitive(false);
        self.recalc_now();
        self.update_status();
    }

    /// Mirror the image, carrying the annotations with it.
    ///
    /// Toggling a flip reflects the displayed frame along that axis regardless
    /// of the rotation in force, because the two flips commute and the rotation
    /// cancels out between the old and new frames.
    fn toggle_flip(&self, horizontal: bool) {
        self.push_undo(false);
        let imp = self.imp();

        let (w, h) = self.output_size();
        let how = if horizontal {
            annotate::Reframe::FlipH
        } else {
            annotate::Reframe::FlipV
        };
        for ann in imp.annotations.borrow_mut().iter_mut() {
            annotate::reframe(ann, w as f64, h as f64, how);
        }

        {
            let mut e = imp.edits.borrow_mut();
            if horizontal {
                e.flip_h ^= true;
            } else {
                e.flip_v ^= true;
            }
        }

        imp.crop_rect.set(None);
        imp.crop_apply_btn.set_sensitive(false);
        self.recalc_now();
    }

    fn reset_edits(&self) {
        self.push_undo(false);
        let imp = self.imp();
        let defaults = EditState::default();
        self.apply_edit_state_to_widgets(&defaults);
        imp.edits.replace(defaults);
        imp.crop_rect.set(None);
        self.recalc_now();
    }

    /// Queue a recomputation, coalescing rapid slider input into one render.
    fn schedule_recalc(&self) {
        let imp = self.imp();
        if let Some(id) = imp.recalc_source.borrow_mut().take() {
            id.remove();
        }

        let weak = self.downgrade();
        let id = glib::timeout_add_local_once(
            std::time::Duration::from_millis(EDIT_DEBOUNCE_MS as u64),
            move || {
                if let Some(w) = weak.upgrade() {
                    w.imp().recalc_source.replace(None);
                    w.recalc_now();
                }
            },
        );
        imp.recalc_source.replace(Some(id));
    }

    /// Recompute the edited preview and its Cairo surface immediately.
    fn recalc_now(&self) {
        let imp = self.imp();
        let base = imp.preview_base.borrow().clone();
        let edits = imp.edits.borrow().clone();

        let edited = crate::editing::apply_edits_rgba(&base, &edits);
        let surface = crate::editing::rgba_to_surface(&edited);

        imp.edited_preview.replace(Some(edited));
        imp.edited_surface.replace(surface);

        self.update_drawing_area_size();
        imp.drawing_area.queue_draw();
        self.update_status();
    }

    /// Dimensions of the image as it will be saved, accounting for rotation.
    fn output_size(&self) -> (u32, u32) {
        let imp = self.imp();
        let src = imp.source.borrow();
        let (w, h) = (src.width(), src.height());
        if imp.edits.borrow().changes_geometry() {
            (h, w)
        } else {
            (w, h)
        }
    }

    fn update_drawing_area_size(&self) {
        let imp = self.imp();
        let (w, h) = self.output_size();
        let zoom = imp.zoom.get();
        imp.drawing_area.set_content_width((w as f64 * zoom).round() as i32);
        imp.drawing_area.set_content_height((h as f64 * zoom).round() as i32);
    }

    fn adjust_zoom(&self, delta: f64) {
        let imp = self.imp();
        let zoom = (imp.zoom.get() + delta).clamp(MIN_ZOOM, MAX_ZOOM);
        imp.zoom.set(zoom);
        imp.zoom_label.set_label(&format!("{}%", (zoom * 100.0).round() as i32));
        imp.zoom_out_btn.set_sensitive(zoom > MIN_ZOOM);
        imp.zoom_in_btn.set_sensitive(zoom < MAX_ZOOM);
        self.update_drawing_area_size();
        imp.drawing_area.queue_draw();
    }

    // --- Coordinates -------------------------------------------------------

    /// Map widget coordinates to output-image pixel coordinates.
    fn to_image(&self, x: f64, y: f64) -> (f64, f64) {
        let imp = self.imp();
        let zoom = imp.zoom.get();
        let (w, h) = self.output_size();
        (
            (x / zoom).clamp(0.0, w as f64),
            (y / zoom).clamp(0.0, h as f64),
        )
    }

    // --- Tools -------------------------------------------------------------

    fn set_tool(&self, tool: Tool) {
        let imp = self.imp();
        imp.active_tool.set(tool);
        imp.in_progress.replace(None);
        if tool != Tool::Crop {
            imp.crop_rect.set(None);
            imp.crop_apply_btn.set_sensitive(false);
        }
        imp.text_entry.set_sensitive(tool == Tool::Text);

        let hint = match tool {
            Tool::Crop => gettext("Drag a region, then click inside it or press Apply Crop."),
            Tool::Text => gettext("Type the label, then click on the image to place it."),
            Tool::Number => gettext("Click to drop the next numbered marker."),
            Tool::Pixelate => gettext("Drag over sensitive data. Pixels are destroyed, not covered."),
            Tool::Blackout => gettext("Drag to cover a region with solid colour."),
            _ => gettext("Drag on the image to draw."),
        };
        self.update_hint(&hint);
        imp.drawing_area.queue_draw();
    }

    fn current_color(&self) -> annotate::Rgb {
        let idx = self.imp().color_index.get();
        annotate::PALETTE
            .get(idx)
            .map(|(_, c)| *c)
            .unwrap_or((0.9, 0.11, 0.14))
    }

    /// Stroke width in output-image pixels.
    ///
    /// The toolbar value is in screen-ish units; scaling it by the image's own
    /// size keeps a 4-pixel stroke looking the same on a 4K capture as on a
    /// 720p one.
    fn current_stroke(&self) -> f64 {
        let imp = self.imp();
        let (_, h) = self.output_size();
        let factor = (h as f64 / 900.0).clamp(0.6, 4.0);
        imp.stroke_scale.value() * factor
    }

    fn cancel_pending(&self) {
        let imp = self.imp();
        imp.in_progress.replace(None);
        imp.crop_rect.set(None);
        imp.dragging.set(false);
        imp.free_points.borrow_mut().clear();
        imp.crop_apply_btn.set_sensitive(false);
        imp.drawing_area.queue_draw();
        self.update_status();
    }

    // --- Gestures ----------------------------------------------------------

    fn on_drag_begin(&self, x: f64, y: f64) {
        let imp = self.imp();
        if imp.saving.get() {
            return;
        }
        let p = self.to_image(x, y);
        imp.drag_start.set(p);
        imp.drag_end.set(p);
        imp.dragging.set(true);
        imp.drag_moved.set(false);

        if imp.active_tool.get() == Tool::FreeDraw {
            let mut pts = imp.free_points.borrow_mut();
            pts.clear();
            pts.push(p);
        }
        imp.drawing_area.queue_draw();
    }

    fn on_drag_update(&self, x: f64, y: f64, travel: f64) {
        let imp = self.imp();
        if !imp.dragging.get() {
            return;
        }
        if travel > DRAG_THRESHOLD {
            imp.drag_moved.set(true);
        }
        let p = self.to_image(x, y);
        imp.drag_end.set(p);

        if imp.active_tool.get() == Tool::FreeDraw {
            imp.free_points.borrow_mut().push(p);
        }

        self.rebuild_in_progress();
        imp.drawing_area.queue_draw();
        self.update_status();
    }

    fn on_drag_end(&self, x: f64, y: f64) {
        let imp = self.imp();
        if !imp.dragging.get() {
            return;
        }
        imp.dragging.set(false);
        let p = self.to_image(x, y);
        imp.drag_end.set(p);

        let tool = imp.active_tool.get();

        if tool == Tool::Crop {
            let (sx, sy) = imp.drag_start.get();
            let rect = annotate::rect_from_points((sx, sy), p);
            if rect.2 > 2.0 && rect.3 > 2.0 {
                imp.crop_rect.set(Some(rect));
                imp.crop_apply_btn.set_sensitive(true);
            } else {
                imp.crop_rect.set(None);
                imp.crop_apply_btn.set_sensitive(false);
            }
            imp.free_points.borrow_mut().clear();
            imp.drawing_area.queue_draw();
            self.update_status();
            return;
        }

        if tool.is_click_placed() {
            imp.in_progress.replace(None);
            imp.free_points.borrow_mut().clear();
            return;
        }

        self.rebuild_in_progress();
        if let Some(ann) = imp.in_progress.borrow_mut().take() {
            if self.is_meaningful(&ann) {
                self.push_undo(false);
                imp.annotations.borrow_mut().push(ann);
            }
        }
        imp.free_points.borrow_mut().clear();
        imp.drawing_area.queue_draw();
        self.update_status();
    }

    fn on_click(&self, x: f64, y: f64) {
        let imp = self.imp();
        if imp.saving.get() || imp.drag_moved.get() {
            imp.drag_moved.set(false);
            return;
        }

        let p = self.to_image(x, y);
        match imp.active_tool.get() {
            Tool::Crop => {
                // Clicking inside an existing selection commits it, matching the
                // gesture users already know from the previous release.
                if let Some(rect) = imp.crop_rect.get() {
                    if p.0 >= rect.0
                        && p.0 <= rect.0 + rect.2
                        && p.1 >= rect.1
                        && p.1 <= rect.1 + rect.3
                    {
                        self.apply_crop();
                    }
                }
            }
            Tool::Text => {
                let text = imp.text_entry.text().to_string();
                if text.is_empty() {
                    self.update_hint(&gettext("Type the label text first, then click to place it."));
                    return;
                }
                self.push_undo(false);
                imp.annotations.borrow_mut().push(Annotation {
                    shape: Shape::Text { at: p, text },
                    color: self.current_color(),
                    stroke: self.current_stroke(),
                });
                imp.drawing_area.queue_draw();
                self.update_status();
            }
            Tool::Number => {
                let value = imp.next_number.get();
                self.push_undo(false);
                imp.annotations.borrow_mut().push(Annotation {
                    shape: Shape::Number { at: p, value },
                    color: self.current_color(),
                    stroke: self.current_stroke(),
                });
                imp.next_number.set(value + 1);
                imp.drawing_area.queue_draw();
                self.update_status();
            }
            _ => {}
        }
    }

    /// Rebuild the annotation currently being dragged out from the live
    /// endpoints, so the preview shows exactly what committing would produce.
    fn rebuild_in_progress(&self) {
        let imp = self.imp();
        let tool = imp.active_tool.get();
        if tool == Tool::Crop || tool.is_click_placed() {
            imp.in_progress.replace(None);
            return;
        }

        let start = imp.drag_start.get();
        let end = imp.drag_end.get();
        let rect = annotate::rect_from_points(start, end);

        let shape = match tool {
            Tool::Arrow => Shape::Arrow { from: start, to: end },
            Tool::Rectangle => Shape::Rectangle { rect },
            Tool::Ellipse => Shape::Ellipse { rect },
            Tool::Highlighter => Shape::Highlight { rect },
            Tool::Pixelate => Shape::Pixelate { rect },
            Tool::Blackout => Shape::Blackout { rect },
            Tool::FreeDraw => Shape::FreeDraw {
                points: imp.free_points.borrow().clone(),
            },
            _ => return,
        };

        imp.in_progress.replace(Some(Annotation {
            shape,
            color: self.current_color(),
            stroke: self.current_stroke(),
        }));
    }

    /// Reject degenerate marks produced by a stray click-drag of a few pixels.
    fn is_meaningful(&self, ann: &Annotation) -> bool {
        match &ann.shape {
            Shape::FreeDraw { points } => points.len() >= 2,
            Shape::Arrow { from, to } => (to.0 - from.0).hypot(to.1 - from.1) > 4.0,
            _ => {
                let b = annotate::bounds(ann);
                b.2 > 3.0 && b.3 > 3.0
            }
        }
    }

    // --- Crop --------------------------------------------------------------

    /// Bake the pending crop into `source`.
    ///
    /// Edits are applied to the full-resolution image first, because the crop
    /// rectangle was drawn against the edited view; the resulting image becomes
    /// the new `source` and the edit state is reset because it is now baked in.
    /// Annotations are translated into the new origin and dropped if they fall
    /// entirely outside the retained region.
    fn apply_crop(&self) {
        let imp = self.imp();
        let Some((x, y, w, h)) = imp.crop_rect.get() else {
            return;
        };
        if w < 1.0 || h < 1.0 {
            return;
        }

        self.push_undo(true);

        let edits = imp.edits.borrow().clone();
        let flattened = crate::editing::apply_edits_rgba(&imp.source.borrow(), &edits);
        let cropped = crate::editing::crop_rgba(
            flattened,
            x.round() as i32,
            y.round() as i32,
            w.round() as i32,
            h.round() as i32,
        );

        // Shift annotations into the cropped frame and discard the ones that no
        // longer intersect it.
        {
            let (cw, ch) = (cropped.width() as f64, cropped.height() as f64);
            let mut anns = imp.annotations.borrow_mut();
            anns.retain_mut(|ann| {
                annotate::translate(ann, -x, -y);
                let b = annotate::bounds(ann);
                b.0 + b.2 > 0.0 && b.1 + b.3 > 0.0 && b.0 < cw && b.1 < ch
            });
        }

        let defaults = EditState::default();
        self.apply_edit_state_to_widgets(&defaults);
        imp.edits.replace(defaults);
        imp.crop_rect.set(None);
        imp.crop_apply_btn.set_sensitive(false);

        self.set_source(cropped, false);
        self.update_status();
    }

    // --- Status ------------------------------------------------------------

    fn update_status(&self) {
        let imp = self.imp();
        let (w, h) = self.output_size();
        let count = imp.annotations.borrow().len();

        let mut text = gettext("__W__ × __H__ px")
            .replace("__W__", &w.to_string())
            .replace("__H__", &h.to_string());

        if let Some((_, _, cw, ch)) = imp.crop_rect.get() {
            text.push_str("  ·  ");
            text.push_str(
                &gettext("Selection __W__ × __H__")
                    .replace("__W__", &(cw.round() as u32).to_string())
                    .replace("__H__", &(ch.round() as u32).to_string()),
            );
        }
        if count > 0 {
            text.push_str("  ·  ");
            text.push_str(&gettext("__N__ annotations").replace("__N__", &count.to_string()));
        }

        imp.res_label.set_label(&text);
    }

    fn update_hint(&self, hint: &str) {
        self.imp().hint_label.set_label(hint);
    }

    // --- Drawing -----------------------------------------------------------

    fn draw(&self, ctx: &cairo::Context, _width: i32, _height: i32) {
        let imp = self.imp();
        let zoom = imp.zoom.get();
        let scale = imp.preview_scale.get();

        // User space becomes output-image pixels.
        let _ = ctx.save();
        ctx.scale(zoom, zoom);

        // Blit the edited preview, expanded from its reduced resolution.
        if let Some(surface) = imp.edited_surface.borrow().as_ref() {
            let _ = ctx.save();
            ctx.scale(1.0 / scale, 1.0 / scale);
            let pattern = cairo::SurfacePattern::create(surface);
            // Smooth the upscale from the reduced-resolution preview; the
            // default Nearest filter makes the canvas look far worse than the
            // file that will actually be written.
            pattern.set_filter(cairo::Filter::Good);
            let _ = ctx.set_source(&pattern);
            let _ = ctx.paint();
            let _ = ctx.restore();
        }

        // Redactions are sampled from the reduced-resolution edited image, so
        // they are drawn in that image's own space with the annotation
        // coordinates scaled to match.
        let committed = imp.annotations.borrow();
        if let Some(edited) = imp.edited_preview.borrow().as_ref() {
            let mut all: Vec<Annotation> = committed.clone();
            if let Some(pending) = imp.in_progress.borrow().as_ref() {
                all.push(pending.clone());
            }
            let _ = ctx.save();
            ctx.scale(1.0 / scale, 1.0 / scale);
            annotate::draw_redactions_preview(ctx, edited, &all, scale);
            let _ = ctx.restore();
        }

        annotate::draw_vector(ctx, &committed);
        if let Some(pending) = imp.in_progress.borrow().as_ref() {
            annotate::draw_vector(ctx, std::slice::from_ref(pending));
        }
        drop(committed);

        // Watermark last, so it sits above annotations exactly as it will in
        // the saved file.
        let options = imp.options.borrow();
        if options.watermark {
            let (w, h) = self.output_size();
            capture::draw_watermark_overlay(ctx, w as f64, h as f64, &options);
        }
        drop(options);

        let _ = ctx.restore();

        // Crop overlay is drawn in widget space so its outline stays one pixel
        // wide at every zoom level.
        if let Some((x, y, w, h)) = imp.crop_rect.get() {
            let (rx, ry, rw, rh) = (x * zoom, y * zoom, w * zoom, h * zoom);

            ctx.set_source_rgba(0.0, 0.0, 0.0, 0.45);
            ctx.set_fill_rule(cairo::FillRule::EvenOdd);
            let (aw, ah) = (
                imp.drawing_area.content_width() as f64,
                imp.drawing_area.content_height() as f64,
            );
            ctx.rectangle(0.0, 0.0, aw, ah);
            ctx.rectangle(rx, ry, rw, rh);
            let _ = ctx.fill();
            ctx.set_fill_rule(cairo::FillRule::Winding);

            ctx.set_source_rgba(1.0, 1.0, 1.0, 0.95);
            ctx.set_dash(&[6.0, 3.0], 0.0);
            ctx.set_line_width(1.5);
            ctx.rectangle(rx, ry, rw, rh);
            let _ = ctx.stroke();
            ctx.set_dash(&[], 0.0);

            self.draw_badge(
                ctx,
                rx + rw / 2.0,
                ry + rh + 18.0,
                &format!("{} × {}", w.round() as u32, h.round() as u32),
            );
        }
    }

    fn draw_badge(&self, ctx: &cairo::Context, cx: f64, cy: f64, text: &str) {
        let layout = pangocairo::functions::create_layout(ctx);
        let mut desc = pango::FontDescription::from_string("Sans Bold");
        desc.set_absolute_size(12.0 * pango::SCALE as f64);
        layout.set_font_description(Some(&desc));
        layout.set_text(text);

        let (tw, th) = layout.pixel_size();
        let (tw, th) = (tw as f64, th as f64);
        let (bx, by) = (cx - tw / 2.0, cy - th / 2.0);

        ctx.set_source_rgba(0.0, 0.0, 0.0, 0.75);
        ctx.rectangle(bx - 6.0, by - 3.0, tw + 12.0, th + 6.0);
        let _ = ctx.fill();

        ctx.set_source_rgb(1.0, 1.0, 1.0);
        ctx.move_to(bx, by);
        pangocairo::functions::show_layout(ctx, &layout);
    }

    // --- Save / discard ----------------------------------------------------

    /// Everything needed to render the final image, taken from live state.
    fn save_inputs(&self) -> SaveJob {
        let imp = self.imp();

        // The image is taken from `source`, not re-read from disk, so any crop
        // already baked in is preserved along with the edits layered on it.
        let source = SaveSource::Image {
            image: imp.source.borrow().clone(),
            original: crate::editing::source_path(&imp.uri.borrow()),
        };

        let crop = imp.crop_rect.get().map(|(x, y, w, h)| {
            (
                x.round() as i32,
                y.round() as i32,
                w.round() as i32,
                h.round() as i32,
            )
        });

        SaveJob {
            source,
            options: imp.options.borrow().clone(),
            edits: imp.edits.borrow().clone(),
            annotations: imp.annotations.borrow().clone(),
            crop,
        }
    }

    fn set_saving(&self, saving: bool) {
        let imp = self.imp();
        imp.saving.set(saving);
        imp.spinner.set_visible(saving);
        if saving {
            imp.spinner.start();
        } else {
            imp.spinner.stop();
        }
        imp.save_btn.set_sensitive(!saving);
        imp.copy_btn.set_sensitive(!saving);
        imp.discard_btn.set_sensitive(!saving);
    }

    fn do_save(&self) {
        let imp = self.imp();
        if imp.saving.get() {
            return;
        }
        self.set_saving(true);
        self.update_hint(&gettext("Saving…"));

        let job = self.save_inputs();
        let win = self.clone();

        // The window stays up until the save completes. The previous
        // implementation destroyed it first and then encoded synchronously,
        // leaving the user staring at nothing while the main loop was blocked.
        capture::save_async(job.source, job.options, job.edits, job.annotations, job.crop, move |result| {
            let imp = win.imp();
            let main_window = imp.main_window.borrow().clone();

            match result {
                Ok(dest_path) => {
                    if let Some(mw) = &main_window {
                        if let Err(e) = capture::copy_to_clipboard(mw, &dest_path) {
                            eprintln!("Clipboard error: {}", e);
                        }
                    }
                    capture::send_notification(&dest_path);
                    win.finish(main_window);
                }
                Err(e) => {
                    win.set_saving(false);
                    win.update_hint(&gettext("Save failed."));
                    if let Some(mw) = &main_window {
                        capture::show_error_dialog(
                            mw,
                            &gettext("Save Error"),
                            &format!("{}\n\n{}", gettext("Failed to save the screenshot."), e),
                        );
                    }
                }
            }
        });
    }

    /// Render the marked-up image to the clipboard without writing a file.
    ///
    /// The common reason to annotate a screenshot is to paste it straight into
    /// a chat or an issue tracker; requiring a file on disk first is friction
    /// with no purpose.
    fn do_copy_only(&self) {
        let imp = self.imp();
        if imp.saving.get() {
            return;
        }
        self.set_saving(true);
        self.update_hint(&gettext("Copying…"));

        let job = self.save_inputs();
        let win = self.clone();

        capture::render_async(job.source, job.options, job.edits, job.annotations, job.crop, move |result| {
            win.set_saving(false);
            match result {
                Ok(img) => match capture::copy_image_to_clipboard(win.upcast_ref::<gtk4::Widget>(), &img) {
                    Ok(()) => win.update_hint(&gettext("Copied to clipboard.")),
                    Err(e) => {
                        win.update_hint(&gettext("Clipboard unavailable."));
                        eprintln!("Clipboard error: {}", e);
                    }
                },
                Err(e) => {
                    win.update_hint(&gettext("Render failed."));
                    eprintln!("Render error: {}", e);
                }
            }
        });
    }

    fn do_discard(&self) {
        let imp = self.imp();
        if imp.saving.get() {
            return;
        }

        // Remove the capture the portal or the CLI tool produced, so a
        // discarded shot leaves nothing behind.
        if let Some(path) = crate::editing::source_path(&imp.uri.borrow()) {
            let _ = std::fs::remove_file(path);
        }

        let main_window = imp.main_window.borrow().clone();
        self.finish(main_window);
    }

    /// Tear down the preview and hand control back to the main window.
    fn finish(&self, main_window: Option<SuperShotWindow>) {
        let imp = self.imp();
        if let Some(id) = imp.recalc_source.borrow_mut().take() {
            id.remove();
        }
        let hold = imp.hold_guard.borrow_mut().take();
        let temp = imp.temp.borrow_mut().take();

        self.destroy();

        if let Some(mw) = main_window {
            mw.set_capture_sensitive(true);
            mw.set_busy(false);
            gtk4::prelude::WidgetExt::set_visible(&mw, true);
            gtk4::prelude::GtkWindowExt::present(&mw);
        }

        drop(temp);
        drop(hold);
    }
}

/// Register one CSS class per palette colour for the toolbar swatches.
///
/// Installed once per process, guarded because the preview window can be
/// constructed many times over a session.
fn install_swatch_styles() {
    use std::sync::Once;
    static INSTALL: Once = Once::new();

    INSTALL.call_once(|| {
        let Some(display) = gtk4::gdk::Display::default() else {
            return;
        };

        let mut css = String::new();
        for (idx, (_, (r, g, b))) in annotate::PALETTE.iter().enumerate() {
            css.push_str(&format!(
                ".supershot-swatch-{} {{ background-image: none; background-color: rgb({},{},{}); }}\n",
                idx,
                (r * 255.0).round() as u8,
                (g * 255.0).round() as u8,
                (b * 255.0).round() as u8
            ));
        }

        let provider = gtk4::CssProvider::new();
        provider.load_from_string(&css);
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    });
}
