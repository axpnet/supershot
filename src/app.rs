// SuperShot - Application object
// Copyright (c) 2026 axpnet <https://github.com/axpnet>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Defines the GLib/GTK application subclass using the Adwaita framework.
// Responsible for application lifecycle management and window creation.

use libadwaita as adw;
use gtk4 as gtk;
use gtk4::{gio, glib};

mod imp {
    use super::*;
    use gtk4::prelude::*;
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
        fn startup(&self) {
            self.parent_startup();
            let app = self.obj();

            // Action: open a screenshot file in the default image viewer.
            // Activated by clicking the desktop notification after capture.
            let open_action = gio::SimpleAction::new("open-screenshot", Some(&String::static_variant_type()));
            open_action.connect_activate(|_, param| {
                if let Some(path) = param.and_then(|p| p.get::<String>()) {
                    let uri = format!("file://{}", path);
                    if let Err(e) = gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE) {
                        eprintln!("Failed to open screenshot: {}", e);
                    }
                }
            });
            app.add_action(&open_action);

            // Action: trigger screenshot capture via keyboard shortcut.
            let capture_action = gio::SimpleAction::new("capture", None);
            capture_action.connect_activate(|_, _| {
                if let Some(app) = gio::Application::default() {
                    if let Some(win) = app.downcast_ref::<gtk::Application>()
                        .and_then(|a| a.active_window())
                    {
                        if let Some(sw) = win.downcast_ref::<crate::window::SuperShotWindow>() {
                            sw.start_capture_flow();
                        }
                    }
                }
            });
            app.add_action(&capture_action);
            app.set_accels_for_action("app.capture", &["<Control>Return"]);
        }

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
