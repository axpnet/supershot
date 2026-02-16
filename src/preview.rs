// SuperShot - Preview and crop window
// Copyright (c) 2026 axpnet <https://github.com/axpnet>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Displays a captured screenshot in a separate window, allowing the user to
// optionally drag-select a crop region before saving. Uses a GtkDrawingArea
// inside a GtkScrolledWindow with zoom controls. The watermark is rendered
// as a live overlay in image coordinate space.

use gtk4::prelude::*;
use gtk4::{glib, gio};
use libadwaita as adw;
use libadwaita::prelude::*;
use glib::subclass::types::ObjectSubclassIsExt;
use std::cell::{Cell, RefCell};
use crate::capture::{self, CaptureOptions};
use crate::window::SuperShotWindow;

const MIN_ZOOM: f64 = 0.25;
const MAX_ZOOM: f64 = 4.0;
const ZOOM_STEP: f64 = 0.25;

mod imp {
    use super::*;
    use gtk4::subclass::prelude::*;
    use libadwaita::subclass::prelude::*;

    pub struct PreviewWindow {
        pub drawing_area: gtk4::DrawingArea,
        pub scrolled: gtk4::ScrolledWindow,
        pub save_btn: gtk4::Button,
        pub discard_btn: gtk4::Button,
        pub zoom_in_btn: gtk4::Button,
        pub zoom_out_btn: gtk4::Button,
        pub zoom_label: gtk4::Label,

        // Image state.
        pub pixbuf: RefCell<Option<gdk_pixbuf::Pixbuf>>,

        // Zoom level: 1.0 = original pixel size.
        pub zoom: Cell<f64>,

        // Crop selection in drawing-area coordinates.
        pub crop_start: Cell<(f64, f64)>,
        pub crop_end: Cell<(f64, f64)>,
        pub crop_active: Cell<bool>,

        // Capture context transferred from the main pipeline.
        pub options: RefCell<CaptureOptions>,
        pub uri: RefCell<String>,
        pub main_window: RefCell<Option<SuperShotWindow>>,
        pub hold_guard: RefCell<Option<gio::ApplicationHoldGuard>>,
    }

    impl Default for PreviewWindow {
        fn default() -> Self {
            Self {
                drawing_area: gtk4::DrawingArea::new(),
                scrolled: gtk4::ScrolledWindow::new(),
                save_btn: gtk4::Button::new(),
                discard_btn: gtk4::Button::new(),
                zoom_in_btn: gtk4::Button::new(),
                zoom_out_btn: gtk4::Button::new(),
                zoom_label: gtk4::Label::new(Some("100%")),
                pixbuf: RefCell::new(None),
                zoom: Cell::new(1.0),
                crop_start: Cell::new((0.0, 0.0)),
                crop_end: Cell::new((0.0, 0.0)),
                crop_active: Cell::new(false),
                options: RefCell::new(CaptureOptions::default()),
                uri: RefCell::new(String::new()),
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

            let header = adw::HeaderBar::new();

            self.discard_btn.set_label("Discard");
            self.discard_btn.add_css_class("destructive-action");
            header.pack_start(&self.discard_btn);

            self.save_btn.set_label("Save");
            self.save_btn.add_css_class("suggested-action");
            header.pack_end(&self.save_btn);

            // Zoom controls in the header center.
            let zoom_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
            self.zoom_out_btn.set_icon_name("zoom-out-symbolic");
            self.zoom_out_btn.add_css_class("flat");
            self.zoom_in_btn.set_icon_name("zoom-in-symbolic");
            self.zoom_in_btn.add_css_class("flat");
            self.zoom_label.set_width_chars(5);
            zoom_box.append(&self.zoom_out_btn);
            zoom_box.append(&self.zoom_label);
            zoom_box.append(&self.zoom_in_btn);
            header.set_title_widget(Some(&zoom_box));

            // Drawing area inside a scrolled window.
            self.scrolled.set_hexpand(true);
            self.scrolled.set_vexpand(true);
            self.scrolled.set_child(Some(&self.drawing_area));

            let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
            vbox.append(&header);
            vbox.append(&self.scrolled);
            obj.set_content(Some(&vbox));
            obj.set_default_size(800, 600);
            obj.set_title(Some("Preview"));

            // --- Drawing function ---
            let weak = obj.downgrade();
            self.drawing_area.set_draw_func(move |_area, ctx, width, height| {
                if let Some(win) = weak.upgrade() {
                    win.draw(ctx, width, height);
                }
            });

            // --- Crop gesture ---
            let drag = gtk4::GestureDrag::new();
            let weak_drag = obj.downgrade();
            drag.connect_drag_begin(move |_, x, y| {
                if let Some(win) = weak_drag.upgrade() {
                    let imp = win.imp();
                    imp.crop_start.set((x, y));
                    imp.crop_end.set((x, y));
                    imp.crop_active.set(true);
                    imp.drawing_area.queue_draw();
                }
            });
            let weak_update = obj.downgrade();
            drag.connect_drag_update(move |gesture, off_x, off_y| {
                if let Some(win) = weak_update.upgrade() {
                    if let Some((sx, sy)) = gesture.start_point() {
                        let imp = win.imp();
                        imp.crop_end.set((sx + off_x, sy + off_y));
                        imp.drawing_area.queue_draw();
                    }
                }
            });
            self.drawing_area.add_controller(drag);

            // --- Zoom buttons ---
            let weak_zin = obj.downgrade();
            self.zoom_in_btn.connect_clicked(move |_| {
                if let Some(win) = weak_zin.upgrade() {
                    win.adjust_zoom(super::ZOOM_STEP);
                }
            });
            let weak_zout = obj.downgrade();
            self.zoom_out_btn.connect_clicked(move |_| {
                if let Some(win) = weak_zout.upgrade() {
                    win.adjust_zoom(-super::ZOOM_STEP);
                }
            });

            // --- Save button ---
            let weak_save = obj.downgrade();
            self.save_btn.connect_clicked(move |_| {
                if let Some(win) = weak_save.upgrade() {
                    win.do_save();
                }
            });

            // --- Discard button ---
            let weak_discard = obj.downgrade();
            self.discard_btn.connect_clicked(move |_| {
                if let Some(win) = weak_discard.upgrade() {
                    win.do_discard();
                }
            });

            // --- Window close (WM X button) falls back to discard ---
            let weak_close = obj.downgrade();
            obj.connect_close_request(move |_| {
                if let Some(win) = weak_close.upgrade() {
                    win.do_discard();
                }
                glib::Propagation::Stop
            });
        }
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
        options: CaptureOptions,
        main_window: SuperShotWindow,
        hold: Option<gio::ApplicationHoldGuard>,
    ) {
        let win: Self = glib::Object::new();

        // Load the image.
        if let Some(path) = gio::File::for_uri(&uri).path() {
            if let Ok(pixbuf) = gdk_pixbuf::Pixbuf::from_file(&path) {
                // Set window size to image dimensions. For large images,
                // maximize the window so the scrolled view is usable.
                let iw = pixbuf.width();
                let ih = pixbuf.height();
                let header_h = 47;
                let max_w = 1920;
                let max_h = 1080;
                if iw > max_w || ih > max_h {
                    win.set_default_size(max_w, max_h);
                    win.set_maximized(true);
                } else {
                    win.set_default_size(iw.max(400), (ih + header_h).max(300));
                }

                win.imp().pixbuf.replace(Some(pixbuf));
                win.update_drawing_area_size();
            }
        }

        // Store capture context.
        win.imp().uri.replace(uri);
        win.imp().options.replace(options);
        win.imp().main_window.replace(Some(main_window.clone()));
        win.imp().hold_guard.replace(hold);

        win.set_transient_for(Some(&main_window));
        win.present();
    }

    /// Resize the drawing area to match image dimensions at the current zoom.
    fn update_drawing_area_size(&self) {
        let imp = self.imp();
        let pixbuf = imp.pixbuf.borrow();
        if let Some(pb) = pixbuf.as_ref() {
            let zoom = imp.zoom.get();
            let w = (pb.width() as f64 * zoom) as i32;
            let h = (pb.height() as f64 * zoom) as i32;
            imp.drawing_area.set_content_width(w);
            imp.drawing_area.set_content_height(h);
        }
    }

    /// Adjust zoom by delta, clamped to [MIN_ZOOM, MAX_ZOOM].
    fn adjust_zoom(&self, delta: f64) {
        let imp = self.imp();
        let new_zoom = (imp.zoom.get() + delta).clamp(MIN_ZOOM, MAX_ZOOM);
        imp.zoom.set(new_zoom);
        imp.zoom_label.set_label(&format!("{}%", (new_zoom * 100.0) as i32));
        imp.zoom_out_btn.set_sensitive(new_zoom > MIN_ZOOM);
        imp.zoom_in_btn.set_sensitive(new_zoom < MAX_ZOOM);

        // Reset crop on zoom change (coordinates become invalid).
        imp.crop_active.set(false);

        self.update_drawing_area_size();
        imp.drawing_area.queue_draw();
    }

    /// Convert drawing-area coordinates to image pixel coordinates.
    fn screen_to_image(&self, sx: f64, sy: f64) -> (i32, i32) {
        let imp = self.imp();
        let zoom = imp.zoom.get();
        let ix = (sx / zoom) as i32;
        let iy = (sy / zoom) as i32;

        // Clamp to image bounds.
        let pixbuf = imp.pixbuf.borrow();
        let (max_w, max_h) = match pixbuf.as_ref() {
            Some(pb) => (pb.width(), pb.height()),
            None => return (0, 0),
        };
        (ix.clamp(0, max_w), iy.clamp(0, max_h))
    }

    /// Cairo draw callback for the drawing area.
    fn draw(&self, ctx: &cairo::Context, _width: i32, _height: i32) {
        let imp = self.imp();
        let pixbuf = imp.pixbuf.borrow();
        let pb = match pixbuf.as_ref() {
            Some(pb) => pb,
            None => return,
        };

        let zoom = imp.zoom.get();

        // Paint the zoomed image.
        let _ = ctx.save();
        ctx.scale(zoom, zoom);
        gtk4::gdk::prelude::GdkCairoContextExt::set_source_pixbuf(ctx, pb, 0.0, 0.0);
        let _ = ctx.paint();
        let _ = ctx.restore();

        // Draw watermark preview overlay (in image coordinate space).
        let options = imp.options.borrow();
        if options.watermark {
            let iw = pb.width() as f64;
            let ih = pb.height() as f64;
            let _ = ctx.save();
            ctx.scale(zoom, zoom);
            let _ = capture::draw_watermark_overlay(ctx, iw, ih, &options);
            let _ = ctx.restore();
        }
        drop(options);

        // Draw crop overlay if active.
        if imp.crop_active.get() {
            let (sx, sy) = imp.crop_start.get();
            let (ex, ey) = imp.crop_end.get();
            let rx = sx.min(ex);
            let ry = sy.min(ey);
            let rw = (ex - sx).abs();
            let rh = (ey - sy).abs();

            if rw > 2.0 && rh > 2.0 {
                // Dim the area outside the selection.
                ctx.set_source_rgba(0.0, 0.0, 0.0, 0.4);
                let _ = ctx.paint();

                // Redraw the image inside the crop rectangle (un-dimmed).
                let _ = ctx.save();
                ctx.rectangle(rx, ry, rw, rh);
                ctx.clip();
                ctx.scale(zoom, zoom);
                gtk4::gdk::prelude::GdkCairoContextExt::set_source_pixbuf(ctx, pb, 0.0, 0.0);
                let _ = ctx.paint();

                // Re-draw watermark inside crop region too.
                let options = imp.options.borrow();
                if options.watermark {
                    let iw = pb.width() as f64;
                    let ih = pb.height() as f64;
                    let _ = capture::draw_watermark_overlay(ctx, iw, ih, &options);
                }
                drop(options);
                let _ = ctx.restore();

                // Dashed white border around the selection.
                ctx.set_source_rgba(1.0, 1.0, 1.0, 0.9);
                ctx.set_dash(&[6.0, 3.0], 0.0);
                ctx.set_line_width(2.0);
                ctx.rectangle(rx, ry, rw, rh);
                let _ = ctx.stroke();
            }
        }
    }

    /// Save the (optionally cropped) screenshot and return to the main window.
    fn do_save(&self) {
        let imp = self.imp();

        // Build crop rectangle in image pixel coordinates.
        let mut options = imp.options.borrow().clone();
        if imp.crop_active.get() {
            let (sx, sy) = imp.crop_start.get();
            let (ex, ey) = imp.crop_end.get();
            let (ix1, iy1) = self.screen_to_image(sx, sy);
            let (ix2, iy2) = self.screen_to_image(ex, ey);
            let x = ix1.min(ix2);
            let y = iy1.min(iy2);
            let w = (ix1 - ix2).abs();
            let h = (iy1 - iy2).abs();
            if w > 0 && h > 0 {
                options.crop = Some((x, y, w, h));
            }
        }

        let uri = imp.uri.borrow().clone();
        let main_window = imp.main_window.borrow().clone();
        let hold = imp.hold_guard.borrow_mut().take();

        self.destroy();

        match capture::process_and_save(&uri, &options) {
            Ok(dest_path) => {
                if let Some(ref mw) = main_window {
                    if let Err(e) = capture::copy_to_clipboard(mw, &dest_path) {
                        eprintln!("Clipboard error: {}", e);
                    }
                }
                capture::send_notification(&dest_path.to_string_lossy());
            }
            Err(e) => {
                if let Some(ref mw) = main_window {
                    capture::show_error_dialog(mw, "Save Error",
                        &format!("Failed to save screenshot:\n{}", e));
                }
            }
        }

        if let Some(mw) = main_window {
            mw.set_capture_sensitive(true);
            gtk4::prelude::WidgetExt::set_visible(&mw, true);
            gtk4::prelude::GtkWindowExt::present(&mw);
        }
        drop(hold);
    }

    /// Discard the capture and return to the main window.
    fn do_discard(&self) {
        let imp = self.imp();
        let main_window = imp.main_window.borrow().clone();
        let hold = imp.hold_guard.borrow_mut().take();

        self.destroy();

        if let Some(mw) = main_window {
            mw.set_capture_sensitive(true);
            gtk4::prelude::WidgetExt::set_visible(&mw, true);
            gtk4::prelude::GtkWindowExt::present(&mw);
        }
        drop(hold);
    }
}
