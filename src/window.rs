// SuperShot - Main application window
// Copyright (c) 2026 axpnet <axp@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Implements the primary user interface as an AdwApplicationWindow subclass
// using GTK4 composite templates. The window provides a delay selector and
// a capture button. All other capture options (mode, clipboard, sound) are
// handled by the XDG Desktop Portal. The delay preference is persisted
// through GSettings when the compiled schema is available.
//
// The countdown label (hidden by default) is activated during delayed captures
// to provide visual feedback of the remaining seconds before capture.

use libadwaita as adw;
use libadwaita::prelude::*;
use gtk4::{glib, gio, CompositeTemplate};
use glib::subclass::types::ObjectSubclassIsExt;
use gettextrs::gettext;

mod imp {
    use super::*;
    use gtk4::subclass::prelude::*;
    use libadwaita::subclass::prelude::*;
    use gtk4 as gtk;

    #[derive(Default, CompositeTemplate)]
    #[template(string = r#"
    <interface>
      <template class="SuperShotWindow" parent="AdwApplicationWindow">
        <property name="title">SuperShot</property>
        <property name="default-width">320</property>
        <property name="default-height">370</property>
        <property name="content">
          <object class="GtkBox">
            <property name="orientation">vertical</property>

            <child>
              <object class="AdwHeaderBar" />
            </child>

            <child>
              <object class="AdwPreferencesPage">
                <child>
                  <object class="AdwPreferencesGroup">
                    <property name="title" translatable="yes">Capture Settings</property>

                    <child>
                        <object class="AdwComboRow" id="delay_row">
                            <property name="title" translatable="yes">Delay</property>
                            <property name="model">
                                <object class="GtkStringList">
                                    <items>
                                        <item translatable="yes">None</item>
                                        <item translatable="yes">3 seconds</item>
                                        <item translatable="yes">5 seconds</item>
                                        <item translatable="yes">10 seconds</item>
                                    </items>
                                </object>
                            </property>
                        </object>
                    </child>

                  </object>
                </child>

                <child>
                    <object class="AdwPreferencesGroup">
                        <child>
                            <object class="GtkLabel" id="countdown_label">
                                <property name="visible">false</property>
                                <property name="halign">center</property>
                                <property name="valign">center</property>
                                <property name="margin-top">8</property>
                                <property name="margin-bottom">4</property>
                                <style>
                                    <class name="title-2"/>
                                </style>
                            </object>
                        </child>
                        <child>
                            <object class="GtkButton" id="capture_btn">
                                <property name="width-request">80</property>
                                <property name="height-request">80</property>
                                <property name="halign">center</property>
                                <property name="margin-top">8</property>
                                <property name="margin-bottom">20</property>
                                <property name="tooltip-text" translatable="yes">Take screenshot</property>
                                <property name="child">
                                    <object class="GtkImage">
                                        <property name="icon-name">supershot-capture-symbolic</property>
                                        <property name="pixel-size">50</property>
                                    </object>
                                </property>
                                <style>
                                    <class name="suggested-action"/>
                                    <class name="circular"/>
                                </style>
                            </object>
                        </child>
                    </object>
                </child>

              </object>
            </child>
          </object>
        </property>
      </template>
    </interface>
    "#)]
    /// Internal state for the main window.
    ///
    /// Template children are bound to the inline XML template via the
    /// `#[template_child]` attribute. The `settings` field is lazily
    /// initialized during `constructed()` if a compiled GSettings schema
    /// is found on the system.
    pub struct SuperShotWindow {
        #[template_child]
        pub capture_btn: TemplateChild<gtk::Button>,

        #[template_child]
        pub delay_row: TemplateChild<adw::ComboRow>,

        #[template_child]
        pub countdown_label: TemplateChild<gtk::Label>,

        pub settings: std::cell::OnceCell<gio::Settings>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SuperShotWindow {
        const NAME: &'static str = "SuperShotWindow";
        type Type = super::SuperShotWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for SuperShotWindow {
        /// Post-construction initialization.
        ///
        /// Attempts to load the GSettings schema and, if successful, establishes
        /// a bidirectional binding between the delay key and the combo row.
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            if let Some(settings) = super::SuperShotWindow::try_load_settings() {
                settings.bind("delay", &*self.delay_row, "selected").build();
                let _ = self.settings.set(settings);
            }

            self.capture_btn.connect_clicked(glib::clone!(
                #[weak]
                obj,
                move |_| {
                    obj.start_capture_flow();
                }
            ));
        }
    }
    impl WidgetImpl for SuperShotWindow {}
    impl WindowImpl for SuperShotWindow {}
    impl ApplicationWindowImpl for SuperShotWindow {}
    impl AdwApplicationWindowImpl for SuperShotWindow {}
}

glib::wrapper! {
    pub struct SuperShotWindow(ObjectSubclass<imp::SuperShotWindow>)
        @extends gtk4::Widget, gtk4::Window, gtk4::ApplicationWindow, adw::ApplicationWindow,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget,
                    gtk4::Native, gtk4::Root, gtk4::ShortcutManager,
                    gtk4::gio::ActionGroup, gtk4::gio::ActionMap;
}

impl SuperShotWindow {
    /// Create a new window attached to the given application instance.
    pub fn new(app: &super::app::SuperShotApp) -> Self {
        glib::Object::builder()
            .property("application", app)
            .build()
    }

    /// Attempt to load the GSettings schema from the system or user schema directories.
    ///
    /// Returns `None` if the schema is not installed, which allows the application
    /// to function without GSettings (using widget defaults instead). During
    /// development, the `build.rs` script installs the schema automatically
    /// into `~/.local/share/glib-2.0/schemas/`.
    fn try_load_settings() -> Option<gio::Settings> {
        let schema_source = gio::SettingsSchemaSource::default()?;
        let schema = schema_source.lookup(crate::config::APP_ID, true)?;
        Some(gio::Settings::new_full(&schema, None::<&gio::SettingsBackend>, None))
    }

    /// Enable or disable the capture button.
    ///
    /// Exposed as a public method so that `capture.rs` can guard against
    /// concurrent capture requests without reaching into the private `imp` module.
    pub fn set_capture_sensitive(&self, sensitive: bool) {
        self.imp().capture_btn.set_sensitive(sensitive);
    }

    /// Display the countdown overlay with the remaining seconds.
    /// Disables the capture button to prevent concurrent capture requests.
    pub fn show_countdown(&self, seconds: u32) {
        let imp = self.imp();
        imp.countdown_label.set_label(&gettext("Capturing in %u\u{2026}").replace("%u", &seconds.to_string()));
        imp.countdown_label.set_visible(true);
        imp.capture_btn.set_sensitive(false);
    }

    /// Hide the countdown overlay and re-enable the capture button.
    pub fn hide_countdown(&self) {
        let imp = self.imp();
        imp.countdown_label.set_visible(false);
        imp.capture_btn.set_sensitive(true);
    }

    /// Read current widget state and initiate the capture pipeline.
    ///
    /// Maps the delay combo box index to seconds (0=None, 1=3s, 2=5s, 3=10s)
    /// and delegates to `capture::start_capture()` which handles countdown,
    /// window hiding, portal invocation, file saving, clipboard copy, and
    /// notification.
    pub fn start_capture_flow(&self) {
        use crate::capture;

        let imp = self.imp();

        let delay_idx = imp.delay_row.selected();
        let delay = match delay_idx {
            1 => 3,
            2 => 5,
            3 => 10,
            _ => 0,
        };

        capture::start_capture(self, delay);
    }
}
