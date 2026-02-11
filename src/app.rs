// SuperShot - Application object
// Copyright (c) 2026 axpnet <axp@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Defines the GLib/GTK application subclass using the Adwaita framework.
// Responsible for application lifecycle management and window creation.

use libadwaita as adw;
use gtk4 as gtk;
use gtk4::{gio, glib};

mod imp {
    use super::*;
    use gtk4::subclass::prelude::*;
    use libadwaita::subclass::prelude::*;

    #[derive(Default)]
    pub struct SuperShotApp;

    #[glib::object_subclass]
    impl ObjectSubclass for SuperShotApp {
        const NAME: &'static str = "SuperShotApp";
        type Type = super::SuperShotApp;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for SuperShotApp {}

    impl ApplicationImpl for SuperShotApp {
        /// Called when the application is activated (first launch or re-focus).
        /// Registers the custom icon search path and creates the main window.
        fn activate(&self) {
            self.parent_activate();
            let app = self.obj();

            // Register custom icon path so GTK finds supershot-capture-symbolic.
            // The installed path (/usr/share/icons) is already in the default
            // search path; this adds the development data/ directory.
            if let Some(display) = gtk4::gdk::Display::default() {
                let icon_theme = gtk::IconTheme::for_display(&display);
                icon_theme.add_search_path("data/icons");
            }

            let window = crate::window::SuperShotWindow::new(&app);
            gtk4::prelude::GtkWindowExt::present(&window);
        }
    }

    impl GtkApplicationImpl for SuperShotApp {}
    impl AdwApplicationImpl for SuperShotApp {}
}

glib::wrapper! {
    pub struct SuperShotApp(ObjectSubclass<imp::SuperShotApp>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl SuperShotApp {
    /// Construct a new application instance with the registered application ID.
    pub fn new() -> Self {
        glib::Object::builder()
            .property("application-id", crate::config::APP_ID)
            .property("flags", gio::ApplicationFlags::default())
            .build()
    }
}
