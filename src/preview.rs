// SuperShot - Preview and editing window
// Copyright (c) 2026 axpnet <https://github.com/axpnet>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Displays a captured screenshot with a full editing sidebar:
// rotate, flip, brightness, contrast, blur, sharpen, grayscale, invert.
// Shows image resolution and crop dimensions. Edits are applied live
// via the `image` crate and rendered with Cairo.

use gtk4::prelude::*;
use gtk4::{glib, gio};
use libadwaita as adw;
use libadwaita::prelude::*;
use glib::subclass::types::ObjectSubclassIsExt;
use std::cell::{Cell, RefCell};
use crate::capture::{self, CaptureOptions};
use crate::editing::EditState;
use crate::window::SuperShotWindow;

const MIN_ZOOM: f64 = 0.25;
const MAX_ZOOM: f64 = 4.0;
const ZOOM_STEP: f64 = 0.25;

mod imp {
    use super::*;
    use gtk4::subclass::prelude::*;
    use libadwaita::subclass::prelude::*;

    pub struct PreviewWindow {
        // Canvas
        pub drawing_area: gtk4::DrawingArea,
        pub scrolled: gtk4::ScrolledWindow,

        // Header buttons
        pub save_btn: gtk4::Button,
        pub discard_btn: gtk4::Button,
        pub zoom_in_btn: gtk4::Button,
        pub zoom_out_btn: gtk4::Button,
        pub zoom_label: gtk4::Label,

        // Resolution info
        pub res_label: gtk4::Label,
        pub crop_label: gtk4::Label,
        pub crop_apply_btn: gtk4::Button,
        pub crop_undo_btn: gtk4::Button,

        // Undo crop: stores pre-crop state
        pub pre_crop_pixbuf: RefCell<Option<gdk_pixbuf::Pixbuf>>,
        pub pre_crop_edit_state: RefCell<Option<EditState>>,

        // Edit controls
        pub brightness_scale: gtk4::Scale,
        pub contrast_scale: gtk4::Scale,
        pub blur_scale: gtk4::Scale,
        pub sharpen_scale: gtk4::Scale,
        pub grayscale_switch: gtk4::Switch,
        pub invert_switch: gtk4::Switch,

        // Image state
        pub pixbuf: RefCell<Option<gdk_pixbuf::Pixbuf>>,
        pub edited_pixbuf: RefCell<Option<gdk_pixbuf::Pixbuf>>,
        pub edit_state: RefCell<EditState>,

        // Zoom
        pub zoom: Cell<f64>,

        // Crop selection in drawing-area coordinates
        pub crop_start: Cell<(f64, f64)>,
        pub crop_end: Cell<(f64, f64)>,
        pub crop_active: Cell<bool>,

        // Capture context
        pub options: RefCell<CaptureOptions>,
        pub uri: RefCell<String>,
        pub main_window: RefCell<Option<SuperShotWindow>>,
        pub hold_guard: RefCell<Option<gio::ApplicationHoldGuard>>,

        // Suppresses edit recalc during programmatic widget resets
        pub suppress_edits: Cell<bool>,
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
                res_label: gtk4::Label::new(None),
                crop_label: gtk4::Label::new(None),
                crop_apply_btn: gtk4::Button::with_label("Apply Crop"),
                crop_undo_btn: gtk4::Button::with_label("Undo Crop"),
                pre_crop_pixbuf: RefCell::new(None),
                pre_crop_edit_state: RefCell::new(None),
                brightness_scale: gtk4::Scale::new(gtk4::Orientation::Horizontal, Some(&gtk4::Adjustment::new(0.0, -100.0, 100.0, 1.0, 10.0, 0.0))),
                contrast_scale: gtk4::Scale::new(gtk4::Orientation::Horizontal, Some(&gtk4::Adjustment::new(0.0, -100.0, 100.0, 1.0, 10.0, 0.0))),
                blur_scale: gtk4::Scale::new(gtk4::Orientation::Horizontal, Some(&gtk4::Adjustment::new(0.0, 0.0, 10.0, 0.5, 1.0, 0.0))),
                sharpen_scale: gtk4::Scale::new(gtk4::Orientation::Horizontal, Some(&gtk4::Adjustment::new(0.0, 0.0, 10.0, 0.5, 1.0, 0.0))),
                grayscale_switch: gtk4::Switch::new(),
                invert_switch: gtk4::Switch::new(),
                pixbuf: RefCell::new(None),
                edited_pixbuf: RefCell::new(None),
                edit_state: RefCell::new(EditState::default()),
                zoom: Cell::new(1.0),
                crop_start: Cell::new((0.0, 0.0)),
                crop_end: Cell::new((0.0, 0.0)),
                crop_active: Cell::new(false),
                options: RefCell::new(CaptureOptions::default()),
                uri: RefCell::new(String::new()),
                main_window: RefCell::new(None),
                hold_guard: RefCell::new(None),
                suppress_edits: Cell::new(false),
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

            // --- Header bar ---
            let header = adw::HeaderBar::new();

            self.discard_btn.set_label("Discard");
            self.discard_btn.add_css_class("destructive-action");
            header.pack_start(&self.discard_btn);

            self.save_btn.set_label("Save");
            self.save_btn.add_css_class("suggested-action");
            header.pack_end(&self.save_btn);

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

            // Edit toggle button in header
            let edit_toggle = gtk4::ToggleButton::new();
            edit_toggle.set_icon_name("document-edit-symbolic");
            edit_toggle.set_tooltip_text(Some("Edit Tools"));
            edit_toggle.add_css_class("flat");
            header.pack_end(&edit_toggle);

            // --- Edit sidebar (hidden by default) ---
            let sidebar = self.build_sidebar(&obj);
            let revealer = gtk4::Revealer::new();
            revealer.set_transition_type(gtk4::RevealerTransitionType::SlideRight);
            revealer.set_transition_duration(200);
            revealer.set_reveal_child(false);
            revealer.set_child(Some(&sidebar));

            // Toggle sidebar visibility
            let rev_ref = revealer.clone();
            edit_toggle.connect_toggled(move |btn| {
                rev_ref.set_reveal_child(btn.is_active());
            });

            // --- Drawing area ---
            self.scrolled.set_hexpand(true);
            self.scrolled.set_vexpand(true);
            self.scrolled.set_child(Some(&self.drawing_area));

            // --- Layout: [revealer(sidebar)] | image ---
            let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
            hbox.append(&revealer);
            hbox.append(&self.scrolled);

            let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
            vbox.append(&header);
            vbox.append(&hbox);
            obj.set_content(Some(&vbox));
            obj.set_default_size(1000, 650);
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
                        win.update_crop_label();
                    }
                }
            });
            let weak_end = obj.downgrade();
            drag.connect_drag_end(move |_, _, _| {
                if let Some(win) = weak_end.upgrade() {
                    win.update_crop_label();
                }
            });
            self.drawing_area.add_controller(drag);

            // --- Click inside crop selection to apply ---
            let click = gtk4::GestureClick::new();
            let weak_click = obj.downgrade();
            click.connect_released(move |_, _, x, y| {
                if let Some(win) = weak_click.upgrade() {
                    let imp = win.imp();
                    if imp.crop_active.get() {
                        let (sx, sy) = imp.crop_start.get();
                        let (ex, ey) = imp.crop_end.get();
                        let rx = sx.min(ex);
                        let ry = sy.min(ey);
                        let rw = (ex - sx).abs();
                        let rh = (ey - sy).abs();
                        if rw > 2.0 && rh > 2.0
                            && x >= rx && x <= rx + rw
                            && y >= ry && y <= ry + rh
                        {
                            win.apply_crop();
                        }
                    }
                }
            });
            self.drawing_area.add_controller(click);

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

            // --- Save / Discard / Close ---
            let weak_save = obj.downgrade();
            self.save_btn.connect_clicked(move |_| {
                if let Some(win) = weak_save.upgrade() {
                    win.do_save();
                }
            });
            let weak_discard = obj.downgrade();
            self.discard_btn.connect_clicked(move |_| {
                if let Some(win) = weak_discard.upgrade() {
                    win.do_discard();
                }
            });
            let weak_close = obj.downgrade();
            obj.connect_close_request(move |_| {
                if let Some(win) = weak_close.upgrade() {
                    win.do_discard();
                }
                glib::Propagation::Stop
            });
        }
    }

    impl PreviewWindow {
        /// Build the left editing sidebar with all controls.
        fn build_sidebar(&self, obj: &super::PreviewWindow) -> gtk4::ScrolledWindow {
            let sidebar_box = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
            sidebar_box.set_margin_top(8);
            sidebar_box.set_margin_bottom(8);
            sidebar_box.set_margin_start(8);
            sidebar_box.set_margin_end(8);
            sidebar_box.set_hexpand(false);

            // --- Resolution info ---
            self.res_label.add_css_class("caption");
            self.res_label.add_css_class("dim-label");
            self.res_label.set_halign(gtk4::Align::Start);
            sidebar_box.append(&self.res_label);

            self.crop_label.add_css_class("caption");
            self.crop_label.add_css_class("dim-label");
            self.crop_label.set_halign(gtk4::Align::Start);
            self.crop_label.set_visible(false);
            sidebar_box.append(&self.crop_label);

            let crop_btn_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
            crop_btn_box.set_halign(gtk4::Align::Start);

            self.crop_apply_btn.add_css_class("suggested-action");
            self.crop_apply_btn.set_visible(false);
            let weak = obj.downgrade();
            self.crop_apply_btn.connect_clicked(move |_| {
                if let Some(win) = weak.upgrade() { win.apply_crop(); }
            });
            crop_btn_box.append(&self.crop_apply_btn);

            self.crop_undo_btn.add_css_class("flat");
            self.crop_undo_btn.set_visible(false);
            let weak = obj.downgrade();
            self.crop_undo_btn.connect_clicked(move |_| {
                if let Some(win) = weak.upgrade() { win.undo_crop(); }
            });
            crop_btn_box.append(&self.crop_undo_btn);

            sidebar_box.append(&crop_btn_box);

            sidebar_box.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

            // --- Geometry: Rotate & Flip ---
            let geo_label = gtk4::Label::new(Some("Geometry"));
            geo_label.add_css_class("heading");
            geo_label.set_halign(gtk4::Align::Start);
            sidebar_box.append(&geo_label);

            let geo_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
            geo_box.set_halign(gtk4::Align::Center);

            let rotate_ccw = gtk4::Button::from_icon_name("object-rotate-left-symbolic");
            rotate_ccw.set_tooltip_text(Some("Rotate Left"));
            rotate_ccw.add_css_class("flat");

            let rotate_cw = gtk4::Button::from_icon_name("object-rotate-right-symbolic");
            rotate_cw.set_tooltip_text(Some("Rotate Right"));
            rotate_cw.add_css_class("flat");

            let flip_h = gtk4::Button::from_icon_name("object-flip-horizontal-symbolic");
            flip_h.set_tooltip_text(Some("Flip Horizontal"));
            flip_h.add_css_class("flat");

            let flip_v = gtk4::Button::from_icon_name("object-flip-vertical-symbolic");
            flip_v.set_tooltip_text(Some("Flip Vertical"));
            flip_v.add_css_class("flat");

            geo_box.append(&rotate_ccw);
            geo_box.append(&rotate_cw);
            geo_box.append(&flip_h);
            geo_box.append(&flip_v);
            sidebar_box.append(&geo_box);

            // Connect geometry buttons
            let weak = obj.downgrade();
            rotate_ccw.connect_clicked(move |_| {
                if let Some(win) = weak.upgrade() { win.rotate(-90); }
            });
            let weak = obj.downgrade();
            rotate_cw.connect_clicked(move |_| {
                if let Some(win) = weak.upgrade() { win.rotate(90); }
            });
            let weak = obj.downgrade();
            flip_h.connect_clicked(move |_| {
                if let Some(win) = weak.upgrade() { win.toggle_flip_h(); }
            });
            let weak = obj.downgrade();
            flip_v.connect_clicked(move |_| {
                if let Some(win) = weak.upgrade() { win.toggle_flip_v(); }
            });

            sidebar_box.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

            // --- Adjustments ---
            let adj_label = gtk4::Label::new(Some("Adjustments"));
            adj_label.add_css_class("heading");
            adj_label.set_halign(gtk4::Align::Start);
            sidebar_box.append(&adj_label);

            self.add_slider_row(&sidebar_box, "Brightness", &self.brightness_scale, obj);
            self.add_slider_row(&sidebar_box, "Contrast", &self.contrast_scale, obj);

            sidebar_box.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

            // --- Effects ---
            let fx_label = gtk4::Label::new(Some("Effects"));
            fx_label.add_css_class("heading");
            fx_label.set_halign(gtk4::Align::Start);
            sidebar_box.append(&fx_label);

            self.add_slider_row(&sidebar_box, "Blur", &self.blur_scale, obj);
            self.add_slider_row(&sidebar_box, "Sharpen", &self.sharpen_scale, obj);

            // Grayscale row
            let gray_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
            let gray_lbl = gtk4::Label::new(Some("Grayscale"));
            gray_lbl.set_hexpand(true);
            gray_lbl.set_halign(gtk4::Align::Start);
            gray_row.append(&gray_lbl);
            gray_row.append(&self.grayscale_switch);
            sidebar_box.append(&gray_row);

            // Invert row
            let inv_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
            let inv_lbl = gtk4::Label::new(Some("Invert"));
            inv_lbl.set_hexpand(true);
            inv_lbl.set_halign(gtk4::Align::Start);
            inv_row.append(&inv_lbl);
            inv_row.append(&self.invert_switch);
            sidebar_box.append(&inv_row);

            // Connect switches
            let weak = obj.downgrade();
            self.grayscale_switch.connect_state_set(move |_, state| {
                if let Some(win) = weak.upgrade() {
                    if !win.imp().suppress_edits.get() {
                        win.imp().edit_state.borrow_mut().grayscale = state;
                        win.recalc_preview();
                    }
                }
                glib::Propagation::Proceed
            });
            let weak = obj.downgrade();
            self.invert_switch.connect_state_set(move |_, state| {
                if let Some(win) = weak.upgrade() {
                    if !win.imp().suppress_edits.get() {
                        win.imp().edit_state.borrow_mut().invert = state;
                        win.recalc_preview();
                    }
                }
                glib::Propagation::Proceed
            });

            sidebar_box.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

            // --- Reset button ---
            let reset_btn = gtk4::Button::with_label("Reset All");
            reset_btn.add_css_class("flat");
            reset_btn.set_halign(gtk4::Align::Center);
            let weak = obj.downgrade();
            reset_btn.connect_clicked(move |_| {
                if let Some(win) = weak.upgrade() { win.reset_edits(); }
            });
            sidebar_box.append(&reset_btn);

            // Wrap in scrolled window for small screens
            let scrolled = gtk4::ScrolledWindow::new();
            scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
            scrolled.set_child(Some(&sidebar_box));
            scrolled.set_width_request(200);
            scrolled
        }

        /// Helper to add a labeled slider row and connect its value-changed signal.
        fn add_slider_row(
            &self,
            parent: &gtk4::Box,
            label: &str,
            scale: &gtk4::Scale,
            obj: &super::PreviewWindow,
        ) {
            let row = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
            let lbl = gtk4::Label::new(Some(label));
            lbl.set_halign(gtk4::Align::Start);
            lbl.add_css_class("caption");
            row.append(&lbl);

            scale.set_draw_value(true);
            scale.set_value_pos(gtk4::PositionType::Right);
            scale.set_hexpand(true);
            row.append(scale);
            parent.append(&row);

            let label_owned = label.to_string();
            let weak = obj.downgrade();
            scale.connect_value_changed(move |s| {
                if let Some(win) = weak.upgrade() {
                    if win.imp().suppress_edits.get() { return; }
                    let val = s.value();
                    let mut es = win.imp().edit_state.borrow_mut();
                    match label_owned.as_str() {
                        "Brightness" => es.brightness = val as i32,
                        "Contrast" => es.contrast = val as i32,
                        "Blur" => es.blur = val as f32,
                        "Sharpen" => es.sharpen = val as f32,
                        _ => {}
                    }
                    drop(es);
                    win.recalc_preview();
                }
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

        if let Some(path) = gio::File::for_uri(&uri).path() {
            if let Ok(pixbuf) = gdk_pixbuf::Pixbuf::from_file(&path) {
                let iw = pixbuf.width();
                let ih = pixbuf.height();
                let header_h = 47;
                let max_w = 1920;
                let max_h = 1080;
                if iw > max_w || ih > max_h {
                    win.set_default_size(max_w, max_h);
                    win.set_maximized(true);
                } else {
                    win.set_default_size(
                        iw.max(600),
                        (ih + header_h).max(400),
                    );
                }

                win.imp().res_label.set_label(
                    &format!("{} × {} px", iw, ih),
                );
                win.imp().pixbuf.replace(Some(pixbuf.clone()));
                win.imp().edited_pixbuf.replace(Some(pixbuf));
                win.update_drawing_area_size();
            }
        }

        win.imp().uri.replace(uri);
        win.imp().options.replace(options);
        win.imp().main_window.replace(Some(main_window.clone()));
        win.imp().hold_guard.replace(hold);

        win.present();
    }

    // --- Edit actions ---

    fn rotate(&self, degrees: i32) {
        let imp = self.imp();
        let mut es = imp.edit_state.borrow_mut();
        // rem_euclid handles negative degrees correctly without the +360 hack
        // and never overflows for any i32 input.
        es.rotation = (es.rotation as i32 + degrees).rem_euclid(360) as u32;
        drop(es);
        imp.crop_active.set(false);
        self.recalc_preview();
    }

    fn toggle_flip_h(&self) {
        let imp = self.imp();
        imp.edit_state.borrow_mut().flip_h ^= true;
        self.recalc_preview();
    }

    fn toggle_flip_v(&self) {
        let imp = self.imp();
        imp.edit_state.borrow_mut().flip_v ^= true;
        self.recalc_preview();
    }

    fn reset_edits(&self) {
        let imp = self.imp();
        imp.suppress_edits.set(true);
        *imp.edit_state.borrow_mut() = EditState::default();
        imp.brightness_scale.set_value(0.0);
        imp.contrast_scale.set_value(0.0);
        imp.blur_scale.set_value(0.0);
        imp.sharpen_scale.set_value(0.0);
        imp.grayscale_switch.set_active(false);
        imp.invert_switch.set_active(false);
        imp.crop_active.set(false);
        imp.suppress_edits.set(false);
        self.recalc_preview();
    }

    /// Recalculate the edited preview pixbuf from the original.
    fn recalc_preview(&self) {
        let imp = self.imp();
        let original = imp.pixbuf.borrow();
        if let Some(pb) = original.as_ref() {
            let edits = imp.edit_state.borrow().clone();
            let edited = crate::editing::apply_preview_edits(pb, &edits);
            // Update resolution label with edited dimensions
            imp.res_label.set_label(
                &format!("{} × {} px", edited.width(), edited.height()),
            );
            imp.edited_pixbuf.replace(Some(edited));
            self.update_drawing_area_size();
            imp.drawing_area.queue_draw();
        }
    }

    /// Update the crop dimension label and apply button visibility.
    fn update_crop_label(&self) {
        let imp = self.imp();
        if imp.crop_active.get() {
            let (sx, sy) = imp.crop_start.get();
            let (ex, ey) = imp.crop_end.get();
            let (ix1, iy1) = self.screen_to_image(sx, sy);
            let (ix2, iy2) = self.screen_to_image(ex, ey);
            let w = (ix1 - ix2).unsigned_abs();
            let h = (iy1 - iy2).unsigned_abs();
            if w > 0 && h > 0 {
                imp.crop_label.set_label(&format!("Selection: {} × {} px", w, h));
                imp.crop_label.set_visible(true);
                imp.crop_apply_btn.set_visible(true);
            }
        } else {
            imp.crop_label.set_visible(false);
            imp.crop_apply_btn.set_visible(false);
        }
    }

    /// Resize the drawing area to match edited image dimensions at current zoom.
    fn update_drawing_area_size(&self) {
        let imp = self.imp();
        let pixbuf = imp.edited_pixbuf.borrow();
        if let Some(pb) = pixbuf.as_ref() {
            let zoom = imp.zoom.get();
            let w = (pb.width() as f64 * zoom) as i32;
            let h = (pb.height() as f64 * zoom) as i32;
            imp.drawing_area.set_content_width(w);
            imp.drawing_area.set_content_height(h);
        }
    }

    fn adjust_zoom(&self, delta: f64) {
        let imp = self.imp();
        let new_zoom = (imp.zoom.get() + delta).clamp(MIN_ZOOM, MAX_ZOOM);
        imp.zoom.set(new_zoom);
        imp.zoom_label.set_label(&format!("{}%", (new_zoom * 100.0).round() as i32));
        imp.zoom_out_btn.set_sensitive(new_zoom > MIN_ZOOM);
        imp.zoom_in_btn.set_sensitive(new_zoom < MAX_ZOOM);
        imp.crop_active.set(false);
        self.update_crop_label();
        self.update_drawing_area_size();
        imp.drawing_area.queue_draw();
    }

    fn screen_to_image(&self, sx: f64, sy: f64) -> (i32, i32) {
        let imp = self.imp();
        let zoom = imp.zoom.get();
        let ix = (sx / zoom) as i32;
        let iy = (sy / zoom) as i32;
        let pixbuf = imp.edited_pixbuf.borrow();
        let (max_w, max_h) = match pixbuf.as_ref() {
            Some(pb) => (pb.width(), pb.height()),
            None => return (0, 0),
        };
        (ix.clamp(0, max_w), iy.clamp(0, max_h))
    }

    /// Cairo draw callback.
    fn draw(&self, ctx: &cairo::Context, _width: i32, _height: i32) {
        let imp = self.imp();
        let pixbuf = imp.edited_pixbuf.borrow();
        let pb = match pixbuf.as_ref() {
            Some(pb) => pb,
            None => return,
        };

        let zoom = imp.zoom.get();

        // Paint the zoomed edited image.
        let _ = ctx.save();
        ctx.scale(zoom, zoom);
        gtk4::gdk::prelude::GdkCairoContextExt::set_source_pixbuf(ctx, pb, 0.0, 0.0);
        let _ = ctx.paint();
        let _ = ctx.restore();

        // Watermark overlay
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

        // Crop overlay
        if imp.crop_active.get() {
            let (sx, sy) = imp.crop_start.get();
            let (ex, ey) = imp.crop_end.get();
            let rx = sx.min(ex);
            let ry = sy.min(ey);
            let rw = (ex - sx).abs();
            let rh = (ey - sy).abs();

            if rw > 2.0 && rh > 2.0 {
                // Dim outside
                ctx.set_source_rgba(0.0, 0.0, 0.0, 0.4);
                let _ = ctx.paint();

                // Redraw inside crop (un-dimmed)
                let _ = ctx.save();
                ctx.rectangle(rx, ry, rw, rh);
                ctx.clip();
                ctx.scale(zoom, zoom);
                gtk4::gdk::prelude::GdkCairoContextExt::set_source_pixbuf(ctx, pb, 0.0, 0.0);
                let _ = ctx.paint();

                let options = imp.options.borrow();
                if options.watermark {
                    let iw = pb.width() as f64;
                    let ih = pb.height() as f64;
                    let _ = capture::draw_watermark_overlay(ctx, iw, ih, &options);
                }
                drop(options);
                let _ = ctx.restore();

                // Dashed border
                ctx.set_source_rgba(1.0, 1.0, 1.0, 0.9);
                ctx.set_dash(&[6.0, 3.0], 0.0);
                ctx.set_line_width(2.0);
                ctx.rectangle(rx, ry, rw, rh);
                let _ = ctx.stroke();

                // Crop dimensions badge
                let (ix1, iy1) = self.screen_to_image(sx, sy);
                let (ix2, iy2) = self.screen_to_image(ex, ey);
                let cw = (ix1 - ix2).unsigned_abs();
                let ch = (iy1 - iy2).unsigned_abs();
                if cw > 0 && ch > 0 {
                    let badge = format!("{} × {}", cw, ch);
                    ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
                    ctx.set_font_size(12.0);
                    let ext = match ctx.text_extents(&badge) {
                        Ok(e) => e,
                        Err(_) => return,
                    };
                    let bx = rx + (rw - ext.width()) / 2.0;
                    let by = ry + rh + 16.0;

                    // Badge background
                    ctx.set_source_rgba(0.0, 0.0, 0.0, 0.7);
                    ctx.rectangle(bx - 6.0, by - ext.height() - 4.0, ext.width() + 12.0, ext.height() + 8.0);
                    let _ = ctx.fill();

                    // Badge text
                    ctx.set_source_rgba(1.0, 1.0, 1.0, 1.0);
                    ctx.move_to(bx, by);
                    let _ = ctx.show_text(&badge);
                }
            }
        }
    }

    /// Apply the current crop selection to the preview, baking in all edits.
    fn apply_crop(&self) {
        let imp = self.imp();
        if !imp.crop_active.get() { return; }

        let (sx, sy) = imp.crop_start.get();
        let (ex, ey) = imp.crop_end.get();
        let (ix1, iy1) = self.screen_to_image(sx, sy);
        let (ix2, iy2) = self.screen_to_image(ex, ey);
        let x = ix1.min(ix2);
        let y = iy1.min(iy2);
        let w = (ix1 - ix2).abs();
        let h = (iy1 - iy2).abs();
        if w <= 0 || h <= 0 { return; }

        // Apply current edits to get the edited image, then crop it
        let edited = imp.edited_pixbuf.borrow();
        if let Some(pb) = edited.as_ref() {
            let pw = pb.width();
            let ph = pb.height();
            if pw <= 0 || ph <= 0 { return; }
            // Clamp crop origin to the pixbuf rect; cw/ch saturating-sub
            // guarantees new_subpixbuf cannot be called with an out-of-range
            // rectangle (which panics in gdk-pixbuf).
            let cx = x.clamp(0, pw - 1);
            let cy = y.clamp(0, ph - 1);
            let cw = w.min(pw - cx);
            let ch = h.min(ph - cy);
            if cw <= 0 || ch <= 0 { return; }

            // Save pre-crop state for undo
            imp.pre_crop_pixbuf.replace(imp.pixbuf.borrow().clone());
            imp.pre_crop_edit_state.replace(Some(imp.edit_state.borrow().clone()));

            let cropped_pb = pb.new_subpixbuf(cx, cy, cw, ch);
            drop(edited);
            // Bake: cropped edited image becomes the new original
            imp.pixbuf.replace(Some(cropped_pb.clone()));
            imp.edited_pixbuf.replace(Some(cropped_pb));
            // Reset edits since they're baked in
            imp.suppress_edits.set(true);
            *imp.edit_state.borrow_mut() = EditState::default();
            imp.brightness_scale.set_value(0.0);
            imp.contrast_scale.set_value(0.0);
            imp.blur_scale.set_value(0.0);
            imp.sharpen_scale.set_value(0.0);
            imp.grayscale_switch.set_active(false);
            imp.invert_switch.set_active(false);
            imp.suppress_edits.set(false);
            // Clear crop, show undo
            imp.crop_active.set(false);
            self.update_crop_label();
            imp.crop_undo_btn.set_visible(true);
            // Update display
            imp.res_label.set_label(
                &format!("{} × {} px",
                    imp.edited_pixbuf.borrow().as_ref().map_or(0, |p| p.width()),
                    imp.edited_pixbuf.borrow().as_ref().map_or(0, |p| p.height())),
            );
            self.update_drawing_area_size();
            imp.drawing_area.queue_draw();
        }
    }

    /// Undo the last crop and restore the pre-crop state.
    fn undo_crop(&self) {
        let imp = self.imp();
        let pre_pb = imp.pre_crop_pixbuf.borrow_mut().take();
        let pre_es = imp.pre_crop_edit_state.borrow_mut().take();
        if let (Some(pb), Some(es)) = (pre_pb, pre_es) {
            // Restore original pixbuf and edit state
            imp.pixbuf.replace(Some(pb));
            imp.suppress_edits.set(true);
            imp.brightness_scale.set_value(es.brightness as f64);
            imp.contrast_scale.set_value(es.contrast as f64);
            imp.blur_scale.set_value(es.blur as f64);
            imp.sharpen_scale.set_value(es.sharpen as f64);
            imp.grayscale_switch.set_active(es.grayscale);
            imp.invert_switch.set_active(es.invert);
            *imp.edit_state.borrow_mut() = es;
            imp.suppress_edits.set(false);
            imp.crop_active.set(false);
            imp.crop_undo_btn.set_visible(false);
            self.update_crop_label();
            self.recalc_preview();
        }
    }

    /// Save the edited + optionally cropped screenshot.
    fn do_save(&self) {
        let imp = self.imp();
        let edits = imp.edit_state.borrow().clone();
        let uri = imp.uri.borrow().clone();
        let options = imp.options.borrow().clone();
        let main_window = imp.main_window.borrow().clone();
        let hold = imp.hold_guard.borrow_mut().take();

        // Build crop rectangle in edited-image pixel coordinates.
        let crop = if imp.crop_active.get() {
            let (sx, sy) = imp.crop_start.get();
            let (ex, ey) = imp.crop_end.get();
            let (ix1, iy1) = self.screen_to_image(sx, sy);
            let (ix2, iy2) = self.screen_to_image(ex, ey);
            let x = ix1.min(ix2);
            let y = iy1.min(iy2);
            let w = (ix1 - ix2).abs();
            let h = (iy1 - iy2).abs();
            if w > 0 && h > 0 { Some((x, y, w, h)) } else { None }
        } else {
            None
        };

        self.destroy();

        match capture::process_and_save_edited(&uri, &options, &edits, crop) {
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

    fn do_discard(&self) {
        let imp = self.imp();
        let main_window = imp.main_window.borrow().clone();
        let hold = imp.hold_guard.borrow_mut().take();

        // Clean up the portal's temp file so it won't be reused on next capture.
        let uri = imp.uri.borrow().clone();
        if let Some(path) = gtk4::gio::File::for_uri(&uri).path() {
            let _ = std::fs::remove_file(&path);
        }

        self.destroy();

        if let Some(mw) = main_window {
            mw.set_capture_sensitive(true);
            gtk4::prelude::WidgetExt::set_visible(&mw, true);
            gtk4::prelude::GtkWindowExt::present(&mw);
        }
        drop(hold);
    }
}
