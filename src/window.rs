// SuperShot - Main application window
// Copyright (c) 2026 axpnet <https://github.com/axpnet>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Implements the primary user interface as an AdwApplicationWindow subclass
// using GTK4 composite templates. The window uses a tabbed layout with
// AdwViewStack: the "Shot" tab holds capture mode, delay, preview toggle and
// the capture button; the "Settings" tab holds watermark, output format and
// save location.
//
// During delayed captures the countdown is displayed inside the capture
// button, replacing the icon with the remaining seconds; while a capture is
// being processed the same button shows a spinner.

use libadwaita as adw;
use libadwaita::prelude::*;
use gtk4::{glib, gio, CompositeTemplate};
use glib::subclass::types::ObjectSubclassIsExt;
use gettextrs::gettext;
use std::path::PathBuf;

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
        <property name="default-width">360</property>
        <!-- Tall enough for the About dialog to open inside it: libadwaita
             presents dialogs within their parent, and a shorter window makes it
             fall back to a clipped bottom sheet. -->
        <property name="default-height">620</property>
        <property name="content">
          <object class="GtkBox">
            <property name="orientation">vertical</property>

            <child>
              <object class="AdwHeaderBar">
                <child type="end">
                  <object class="GtkMenuButton" id="menu_btn">
                    <property name="icon-name">open-menu-symbolic</property>
                    <property name="tooltip-text" translatable="yes">Main Menu</property>
                    <property name="primary">true</property>
                    <property name="menu-model">primary_menu</property>
                  </object>
                </child>
              </object>
            </child>

            <child>
              <object class="AdwViewSwitcher">
                <property name="stack">view_stack</property>
                <property name="policy">wide</property>
                <property name="margin-top">10</property>
                <property name="margin-bottom">4</property>
                <property name="margin-start">9</property>
                <property name="margin-end">9</property>
              </object>
            </child>

            <child>
              <object class="AdwViewStack" id="view_stack">

                <child>
                  <object class="AdwViewStackPage">
                    <property name="name">capture</property>
                    <property name="title" translatable="yes">Shot</property>
                    <property name="icon-name">camera-photo-symbolic</property>
                    <property name="child">
                      <object class="AdwPreferencesPage">

                        <child>
                          <object class="AdwPreferencesGroup">
                            <child>
                              <object class="AdwComboRow" id="mode_row">
                                <property name="title" translatable="yes">Capture</property>
                                <property name="subtitle" translatable="yes">What to take a picture of</property>
                                <property name="model">
                                  <object class="GtkStringList">
                                    <items>
                                      <item translatable="yes">Ask me</item>
                                      <item translatable="yes">Area</item>
                                      <item translatable="yes">Window</item>
                                      <item translatable="yes">Whole screen</item>
                                    </items>
                                  </object>
                                </property>
                              </object>
                            </child>
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
                            <child>
                              <object class="AdwSwitchRow" id="preview_row">
                                <property name="title" translatable="yes">Preview</property>
                                <property name="subtitle" translatable="yes">Annotate and edit before saving</property>
                              </object>
                            </child>
                          </object>
                        </child>

                        <child>
                          <object class="AdwPreferencesGroup">
                            <child>
                              <object class="GtkBox">
                                <property name="orientation">vertical</property>
                                <property name="halign">center</property>
                                <property name="spacing">6</property>
                                <child>
                                  <object class="GtkButton" id="capture_btn">
                                    <property name="width-request">80</property>
                                    <property name="height-request">80</property>
                                    <property name="halign">center</property>
                                    <property name="margin-top">36</property>
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
                                <child>
                                  <object class="GtkLabel">
                                    <property name="label">Ctrl+Enter</property>
                                    <property name="halign">center</property>
                                    <property name="margin-bottom">10</property>
                                    <style>
                                      <class name="dim-label"/>
                                      <class name="caption"/>
                                    </style>
                                  </object>
                                </child>
                              </object>
                            </child>
                          </object>
                        </child>

                      </object>
                    </property>
                  </object>
                </child>

                <child>
                  <object class="AdwViewStackPage">
                    <property name="name">settings</property>
                    <property name="title" translatable="yes">Settings</property>
                    <property name="icon-name">emblem-system-symbolic</property>
                    <property name="child">
                      <object class="AdwPreferencesPage">

                        <child>
                          <object class="AdwPreferencesGroup">
                            <property name="title" translatable="yes">Watermark</property>
                            <child>
                              <object class="AdwSwitchRow" id="watermark_row">
                                <property name="title" translatable="yes">Enabled</property>
                                <property name="subtitle" translatable="yes">Overlay text on screenshot</property>
                              </object>
                            </child>
                            <child>
                              <object class="AdwComboRow" id="watermark_format_row">
                                <property name="title" translatable="yes">Date Format</property>
                                <property name="model">
                                  <object class="GtkStringList">
                                    <items>
                                      <item>2026-02-16 14:30:25</item>
                                      <item>16/02/2026 14:30</item>
                                      <item>Feb 16, 2026 14:30</item>
                                      <item>2026-02-16</item>
                                      <item>14:30:25</item>
                                    </items>
                                  </object>
                                </property>
                              </object>
                            </child>
                            <child>
                              <object class="AdwEntryRow" id="watermark_text_row">
                                <property name="title" translatable="yes">Custom Text</property>
                              </object>
                            </child>
                            <child>
                              <object class="AdwComboRow" id="watermark_position_row">
                                <property name="title" translatable="yes">Position</property>
                                <property name="model">
                                  <object class="GtkStringList">
                                    <items>
                                      <item translatable="yes">Bottom Right</item>
                                      <item translatable="yes">Bottom Left</item>
                                      <item translatable="yes">Top Right</item>
                                      <item translatable="yes">Top Left</item>
                                    </items>
                                  </object>
                                </property>
                              </object>
                            </child>
                            <child>
                              <object class="AdwComboRow" id="watermark_color_row">
                                <property name="title" translatable="yes">Color</property>
                                <property name="model">
                                  <object class="GtkStringList">
                                    <items>
                                      <item translatable="yes">White</item>
                                      <item translatable="yes">Black</item>
                                    </items>
                                  </object>
                                </property>
                              </object>
                            </child>
                          </object>
                        </child>

                        <child>
                          <object class="AdwPreferencesGroup">
                            <property name="title" translatable="yes">Output</property>
                            <child>
                              <object class="AdwComboRow" id="format_row">
                                <property name="title" translatable="yes">Format</property>
                                <property name="model">
                                  <object class="GtkStringList">
                                    <items>
                                      <item>PNG</item>
                                      <item>JPEG</item>
                                    </items>
                                  </object>
                                </property>
                              </object>
                            </child>
                            <child>
                              <object class="AdwSpinRow" id="quality_row">
                                <property name="title" translatable="yes">JPEG Quality</property>
                                <property name="subtitle" translatable="yes">Higher values keep more detail</property>
                                <property name="adjustment">
                                  <object class="GtkAdjustment">
                                    <property name="lower">1</property>
                                    <property name="upper">100</property>
                                    <property name="step-increment">1</property>
                                    <property name="page-increment">10</property>
                                    <property name="value">90</property>
                                  </object>
                                </property>
                              </object>
                            </child>
                            <child>
                              <object class="AdwActionRow" id="save_dir_row">
                                <property name="title" translatable="yes">Save Location</property>
                                <property name="subtitle">~/Pictures/Screenshots/</property>
                                <property name="activatable">true</property>
                                <child type="suffix">
                                  <object class="GtkImage">
                                    <property name="icon-name">folder-symbolic</property>
                                  </object>
                                </child>
                              </object>
                            </child>
                            <child>
                              <object class="AdwActionRow" id="open_folder_row">
                                <property name="title" translatable="yes">Open Screenshots Folder</property>
                                <property name="activatable">true</property>
                                <child type="suffix">
                                  <object class="GtkImage">
                                    <property name="icon-name">folder-open-symbolic</property>
                                  </object>
                                </child>
                              </object>
                            </child>
                          </object>
                        </child>

                      </object>
                    </property>
                  </object>
                </child>

              </object>
            </child>

          </object>
        </property>
      </template>

      <menu id="primary_menu">
        <section>
          <item>
            <attribute name="label" translatable="yes">_Keyboard Shortcuts</attribute>
            <attribute name="action">app.shortcuts</attribute>
          </item>
          <item>
            <attribute name="label" translatable="yes">_About SuperShot</attribute>
            <attribute name="action">app.about</attribute>
          </item>
        </section>
      </menu>
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
        pub menu_btn: TemplateChild<gtk::MenuButton>,

        #[template_child]
        pub mode_row: TemplateChild<adw::ComboRow>,

        #[template_child]
        pub delay_row: TemplateChild<adw::ComboRow>,

        #[template_child]
        pub format_row: TemplateChild<adw::ComboRow>,

        #[template_child]
        pub quality_row: TemplateChild<adw::SpinRow>,

        #[template_child]
        pub watermark_row: TemplateChild<adw::SwitchRow>,

        #[template_child]
        pub watermark_format_row: TemplateChild<adw::ComboRow>,

        #[template_child]
        pub watermark_text_row: TemplateChild<adw::EntryRow>,

        #[template_child]
        pub watermark_position_row: TemplateChild<adw::ComboRow>,

        #[template_child]
        pub watermark_color_row: TemplateChild<adw::ComboRow>,

        #[template_child]
        pub preview_row: TemplateChild<adw::SwitchRow>,

        #[template_child]
        pub save_dir_row: TemplateChild<adw::ActionRow>,

        #[template_child]
        pub open_folder_row: TemplateChild<adw::ActionRow>,

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
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            // Widen the tab buttons. The provider is installed for the display
            // rather than the widget because AdwViewSwitcher builds its buttons
            // internally, and only one window is ever constructed.
            let css = gtk::CssProvider::new();
            css.load_from_string(
                "viewswitcher button { padding: 10px 14px; margin: 0 5px; border-radius: 12px; }",
            );
            if let Some(display) = gtk::gdk::Display::default() {
                gtk::style_context_add_provider_for_display(
                    &display,
                    &css,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
            }

            if let Some(settings) = super::SuperShotWindow::try_load_settings() {
                settings.bind("capture-mode", &*self.mode_row, "selected").build();
                settings.bind("delay", &*self.delay_row, "selected").build();
                settings.bind("format", &*self.format_row, "selected").build();
                settings.bind("jpeg-quality", &*self.quality_row, "value").build();
                settings.bind("watermark", &*self.watermark_row, "active").build();
                settings.bind("watermark-format", &*self.watermark_format_row, "selected").build();
                settings.bind("watermark-position", &*self.watermark_position_row, "selected").build();
                settings.bind("watermark-color", &*self.watermark_color_row, "selected").build();
                settings.bind("preview", &*self.preview_row, "active").build();

                let wm_text: String = settings.string("watermark-text").into();
                if !wm_text.is_empty() {
                    self.watermark_text_row.set_text(&wm_text);
                }

                let dir: String = settings.string("save-directory").into();
                if !dir.is_empty() {
                    self.save_dir_row.set_subtitle(&dir);
                }

                let _ = self.settings.set(settings);
            }

            // The quality row is only meaningful for JPEG output.
            let sync_quality = {
                let format_row = self.format_row.clone();
                let quality_row = self.quality_row.clone();
                move || quality_row.set_sensitive(format_row.selected() == 1)
            };
            sync_quality();
            self.format_row.connect_selected_notify({
                let sync = sync_quality.clone();
                move |_| sync()
            });

            self.capture_btn.connect_clicked(glib::clone!(
                #[weak]
                obj,
                move |_| obj.start_capture_flow()
            ));

            // Persist custom watermark text on edit. Clamp length to prevent
            // unbounded text from causing pathological layout during render.
            self.watermark_text_row.connect_changed(glib::clone!(
                #[weak]
                obj,
                move |row| {
                    if let Some(settings) = obj.imp().settings.get() {
                        let mut text = row.text().to_string();
                        if text.chars().count() > 256 {
                            text = text.chars().take(256).collect();
                        }
                        let _ = settings.set_string("watermark-text", &text);
                    }
                }
            ));

            self.save_dir_row.connect_activated(glib::clone!(
                #[weak]
                obj,
                move |_| obj.open_folder_chooser()
            ));

            self.open_folder_row.connect_activated(glib::clone!(
                #[weak]
                obj,
                move |_| obj.open_screenshots_folder()
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
        glib::Object::builder().property("application", app).build()
    }

    /// Attempt to load the GSettings schema from the system or user schema directories.
    ///
    /// Returns `None` if the schema is not installed, which allows the
    /// application to function with widget defaults instead of crashing — the
    /// behaviour `gio::Settings::new()` would produce.
    fn try_load_settings() -> Option<gio::Settings> {
        let schema_source = gio::SettingsSchemaSource::default()?;
        let schema = schema_source.lookup(crate::config::APP_ID, true)?;
        Some(gio::Settings::new_full(&schema, None::<&gio::SettingsBackend>, None))
    }

    /// Enable or disable the capture button.
    pub fn set_capture_sensitive(&self, sensitive: bool) {
        self.imp().capture_btn.set_sensitive(sensitive);
    }

    /// Show or clear a working indicator while a capture is being processed.
    ///
    /// Rendering and encoding now happen on a worker thread, so the window
    /// stays responsive; without a visible indicator that would read as the
    /// capture having silently failed.
    pub fn set_busy(&self, busy: bool) {
        let imp = self.imp();
        if busy {
            let spinner = gtk4::Spinner::new();
            spinner.set_size_request(40, 40);
            spinner.start();
            imp.capture_btn.set_child(Some(&spinner));
            imp.capture_btn.set_sensitive(false);
        } else {
            self.restore_capture_icon();
        }
    }

    fn restore_capture_icon(&self) {
        let icon = gtk4::Image::from_icon_name("supershot-capture-symbolic");
        icon.set_pixel_size(50);
        self.imp().capture_btn.set_child(Some(&icon));
    }

    /// Display the countdown number inside the capture button.
    pub fn show_countdown(&self, seconds: u32) {
        let imp = self.imp();
        let label = gtk4::Label::new(Some(&seconds.to_string()));
        label.add_css_class("title-1");
        imp.capture_btn.set_child(Some(&label));
        imp.capture_btn.set_sensitive(false);
    }

    /// Restore the capture button icon and re-enable it.
    pub fn hide_countdown(&self) {
        self.restore_capture_icon();
        self.imp().capture_btn.set_sensitive(true);
    }

    /// Return the configured save directory, or `None` for the default.
    fn save_directory(&self) -> Option<PathBuf> {
        self.imp().settings.get().and_then(|s| {
            let dir: String = s.string("save-directory").into();
            if dir.is_empty() {
                None
            } else {
                Some(PathBuf::from(dir))
            }
        })
    }

    /// Collect current widget state into a `CaptureOptions`.
    pub fn capture_options(&self) -> crate::capture::CaptureOptions {
        let imp = self.imp();
        crate::capture::CaptureOptions {
            format_idx: imp.format_row.selected(),
            jpeg_quality: imp.quality_row.value().round().clamp(1.0, 100.0) as u8,
            watermark: imp.watermark_row.is_active(),
            watermark_format: imp.watermark_format_row.selected(),
            watermark_text: imp.watermark_text_row.text().to_string(),
            watermark_position: imp.watermark_position_row.selected(),
            watermark_color: imp.watermark_color_row.selected(),
            save_dir: self.save_directory(),
            show_preview: imp.preview_row.is_active(),
            mode: crate::capture::CaptureMode::from_index(imp.mode_row.selected()),
            captured_at: None,
        }
    }

    /// Read current widget state and initiate the capture pipeline.
    pub fn start_capture_flow(&self) {
        let delay = match self.imp().delay_row.selected() {
            1 => 3,
            2 => 5,
            3 => 10,
            _ => 0,
        };
        crate::capture::start_capture(self, delay, self.capture_options());
    }

    /// Open a folder chooser dialog and update the save directory setting.
    fn open_folder_chooser(&self) {
        let dialog = gtk4::FileDialog::new();
        dialog.set_title(&gettext("Choose Save Directory"));

        if let Some(dir) = self.save_directory() {
            dialog.set_initial_folder(Some(&gio::File::for_path(&dir)));
        }

        dialog.select_folder(
            Some(self),
            gio::Cancellable::NONE,
            glib::clone!(
                #[weak(rename_to = win)]
                self,
                move |result: Result<gio::File, glib::Error>| {
                    let Ok(folder) = result else { return };
                    let Some(path) = folder.path() else { return };

                    // Canonicalize to resolve symlinks and normalize the path
                    // before persisting; fall back to the raw path if that
                    // fails (e.g. the directory vanished between selection and
                    // persist).
                    let resolved = std::fs::canonicalize(&path).unwrap_or(path);
                    let path_str = resolved.to_string_lossy().to_string();

                    let imp = win.imp();
                    imp.save_dir_row.set_subtitle(&path_str);
                    if let Some(settings) = imp.settings.get() {
                        let _ = settings.set_string("save-directory", &path_str);
                    }
                }
            ),
        );
    }

    /// Open the screenshots save directory in the system file manager.
    fn open_screenshots_folder(&self) {
        let dir = self.save_directory().unwrap_or_else(|| {
            dirs::picture_dir()
                .or_else(|| dirs::home_dir().map(|h| h.join("Pictures")))
                .unwrap_or_else(std::env::temp_dir)
                .join("Screenshots")
        });

        // Create on first use so the launcher does not fail on a fresh install
        // where no screenshot has been taken yet.
        let _ = std::fs::create_dir_all(&dir);

        // Build the URI through GIO rather than by string concatenation: a
        // hand-built "file://" + path breaks on any directory containing a
        // space, a '#', a '%' or a non-ASCII character.
        let uri = gio::File::for_path(&dir).uri();
        if let Err(e) = gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE) {
            eprintln!("Failed to open folder: {}", e);
        }
    }
}
