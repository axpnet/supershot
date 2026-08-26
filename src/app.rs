// SuperShot - Application object
// Copyright (c) 2026 axpnet <https://github.com/axpnet>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Defines the GLib/GTK application subclass using the Adwaita framework.
// Responsible for application lifecycle management, the actions reachable from
// desktop notifications and keyboard shortcuts, and window creation.

use libadwaita as adw;
use gtk4 as gtk;
use gtk4::{gio, glib};

mod imp {
    use super::*;
    // The Adwaita prelude re-exports the GTK one and additionally brings in
    // AdwDialogExt, AdwActionRowExt and friends, which the About and shortcuts
    // dialogs need.
    use libadwaita::prelude::*;
    use gtk4::subclass::prelude::*;
    use libadwaita::subclass::prelude::*;

    /// Application state.
    ///
    /// `headless` suppresses window creation for `--now`. Without it the
    /// default `activate` handler builds the main window even in headless
    /// mode, so a scripted capture flashes the GUI on screen and then leaves
    /// it behind — visible in the screenshot the user was trying to take.
    #[derive(Default)]
    pub struct SuperShotApp {
        pub headless: std::cell::Cell<bool>,
        /// Set by `--edit`: open this image in the annotation editor rather
        /// than capturing a new one.
        pub edit_target: std::cell::RefCell<Option<std::path::PathBuf>>,
    }

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

            // Action: open a screenshot in the default image viewer. Activated
            // by clicking the desktop notification posted after a capture.
            let open_action = gio::SimpleAction::new(
                "open-screenshot",
                Some(&String::static_variant_type()),
            );
            open_action.connect_activate(|_, param| {
                if let Some(path) = param.and_then(|p| p.get::<String>()) {
                    launch_uri(&gio::File::for_path(&path));
                }
            });
            app.add_action(&open_action);

            // Action: reveal the screenshot's directory in the file manager.
            let folder_action =
                gio::SimpleAction::new("open-folder", Some(&String::static_variant_type()));
            folder_action.connect_activate(|_, param| {
                if let Some(path) = param.and_then(|p| p.get::<String>()) {
                    let file = gio::File::for_path(&path);
                    match file.parent() {
                        Some(parent) => launch_uri(&parent),
                        None => launch_uri(&file),
                    }
                }
            });
            app.add_action(&folder_action);

            // Action: trigger a capture from the keyboard shortcut.
            let capture_action = gio::SimpleAction::new("capture", None);
            capture_action.connect_activate(|_, _| {
                if let Some(win) = gio::Application::default()
                    .and_then(|a| a.downcast::<gtk::Application>().ok())
                    .and_then(|a| a.active_window())
                {
                    if let Some(sw) = win.downcast_ref::<crate::window::SuperShotWindow>() {
                        sw.start_capture_flow();
                    }
                }
            });
            app.add_action(&capture_action);
            app.set_accels_for_action("app.capture", &["<Control>Return"]);

            // Action: the About dialog. Beyond credits it is where a user can
            // read off the running version and the session details a bug report
            // needs — display server, desktop, portal availability, packaging
            // channel — which for a tool that has to work across every Linux
            // desktop is the difference between a reproducible report and a
            // guess.
            let about_action = gio::SimpleAction::new("about", None);
            about_action.connect_activate(|_, _| show_about());
            app.add_action(&about_action);

            let shortcuts_action = gio::SimpleAction::new("shortcuts", None);
            shortcuts_action.connect_activate(|_, _| show_shortcuts());
            app.add_action(&shortcuts_action);
            app.set_accels_for_action("app.shortcuts", &["<Control>question"]);

            let quit_action = gio::SimpleAction::new("quit", None);
            quit_action.connect_activate(|_, _| {
                if let Some(app) = gio::Application::default() {
                    app.quit();
                }
            });
            app.add_action(&quit_action);
            app.set_accels_for_action("app.quit", &["<Control>q"]);
        }

        /// Called when the application is activated (first launch or re-focus).
        /// Registers the custom icon search path and creates the main window.
        fn activate(&self) {
            self.parent_activate();
            let app = self.obj();

            if self.headless.get() {
                return;
            }

            if let Some(display) = gtk4::gdk::Display::default() {
                let icon_theme = gtk::IconTheme::for_display(&display);
                // Installed prefixes are already on the default search path;
                // these entries cover running straight from a source checkout
                // and from a relocatable bundle such as an AppImage.
                icon_theme.add_search_path("data/icons");
                if let Some(prefix) = install_prefix() {
                    icon_theme.add_search_path(prefix.join("share/icons"));
                }
            }

            let window = crate::window::SuperShotWindow::new(&app);

            if let Some(path) = self.edit_target.borrow_mut().take() {
                // The main window is built but not presented: it backs the
                // editor's clipboard and settings, and becomes visible only if
                // the user keeps working after saving.
                let uri = gio::File::for_path(&path).uri().to_string();
                crate::preview::PreviewWindow::present_for(
                    uri,
                    None,
                    window.capture_options(),
                    window,
                    None,
                );
                return;
            }

            gtk4::prelude::GtkWindowExt::present(&window);
        }
    }

    /// Open a GFile with the user's default handler.
    ///
    /// The URI is produced by GIO rather than by concatenating "file://" with
    /// the path, which fails for any path containing a space, a '#', a '%' or a
    /// non-ASCII character — routine for a localized Pictures directory or a
    /// user-chosen folder.
    fn launch_uri(file: &gio::File) {
        let uri = file.uri();
        if let Err(e) = gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE) {
            eprintln!("Failed to open {}: {}", uri, e);
        }
    }

    /// Present the About dialog anchored to the active window.
    fn show_about() {
        use gettextrs::gettext;

        let about = adw::AboutDialog::builder()
            .application_name("SuperShot")
            .application_icon(crate::config::APP_ID)
            .developer_name("axpnet")
            .version(crate::config::VERSION)
            .website("https://github.com/axpnet/supershot")
            .issue_url("https://github.com/axpnet/supershot/issues")
            .license_type(gtk::License::Gpl30)
            .copyright("© 2026 axpnet")
            .comments(gettext("Capture, annotate and share screenshots."))
            .debug_info(crate::capture::session_report())
            .debug_info_filename("supershot-debug-info.txt")
            .build();

        // TRANSLATORS: replace with your own name and contact to be credited
        // in the About dialog of your language's build.
        let translators = gettext("translator-credits");
        if translators != "translator-credits" {
            about.set_translator_credits(&translators);
        }

        about.present(active_window().as_ref());
    }

    /// Present the keyboard shortcuts window.
    fn show_shortcuts() {
        use gettextrs::gettext;

        // Section titles are wrapped in gettext at their literal call site:
        // xgettext cannot extract a string passed through a variable, so
        // `gettext(*section)` would have left these permanently untranslated.
        let rows: &[(String, &[(&str, String)])] = &[
            (
                gettext("Main Window"),
                &[
                    ("<Control>Return", gettext("Take screenshot")),
                    ("<Control>question", gettext("Keyboard shortcuts")),
                    ("<Control>q", gettext("Quit")),
                ],
            ),
            (
                gettext("Preview"),
                &[
                    ("<Control>s", gettext("Save")),
                    ("<Control>c", gettext("Copy to clipboard")),
                    ("<Control>z", gettext("Undo")),
                    ("<Control><Shift>z", gettext("Redo")),
                    ("Escape", gettext("Cancel the pending selection")),
                ],
            ),
        ];

        let page = gtk::Box::new(gtk::Orientation::Vertical, 18);
        page.set_margin_top(18);
        page.set_margin_bottom(18);
        page.set_margin_start(18);
        page.set_margin_end(18);

        for (section, entries) in rows {
            let heading = gtk::Label::new(Some(section));
            heading.add_css_class("heading");
            heading.set_halign(gtk::Align::Start);
            page.append(&heading);

            let list = gtk::ListBox::new();
            list.add_css_class("boxed-list");
            list.set_selection_mode(gtk::SelectionMode::None);

            for (accel, label) in *entries {
                let row = adw::ActionRow::builder().title(label).build();
                let shortcut = gtk::ShortcutLabel::new(accel);
                shortcut.set_valign(gtk::Align::Center);
                row.add_suffix(&shortcut);
                list.append(&row);
            }
            page.append(&list);
        }

        let dialog = adw::Dialog::new();
        dialog.set_title(&gettext("Keyboard Shortcuts"));
        dialog.set_content_width(420);

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        toolbar.set_content(Some(&page));
        dialog.set_child(Some(&toolbar));

        dialog.present(active_window().as_ref());
    }

    fn active_window() -> Option<gtk::Window> {
        gio::Application::default()
            .and_then(|a| a.downcast::<gtk::Application>().ok())
            .and_then(|a| a.active_window())
    }

    /// Installation prefix derived from the running executable.
    fn install_prefix() -> Option<std::path::PathBuf> {
        std::env::current_exe()
            .ok()?
            .parent()?
            .parent()
            .map(|p| p.to_path_buf())
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

    /// Suppress main-window creation, for `--now`.
    pub fn set_headless(&self, headless: bool) {
        use gtk4::subclass::prelude::ObjectSubclassIsExt;
        self.imp().headless.set(headless);
    }

    /// Open this image in the annotation editor on activation, for `--edit`.
    pub fn set_edit_target(&self, path: Option<std::path::PathBuf>) {
        use gtk4::subclass::prelude::ObjectSubclassIsExt;
        self.imp().edit_target.replace(path);
    }
}
