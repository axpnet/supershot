// SuperShot - Screenshot capture pipeline
// Copyright (c) 2026 axpnet <axp@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Orchestrates the full capture workflow: optional countdown, window hiding,
// XDG Desktop Portal screenshot request, file persistence to
// ~/Pictures/Screenshots/, clipboard copy, and desktop notification.
// Provides both GUI-driven and headless (CLI) capture paths.
//
// The XDG Screenshot portal is always invoked with `interactive(true)`, which
// presents the portal's native selection UI where the user chooses between
// area, window, or full-screen capture. The screenshot is always copied to
// the system clipboard after saving to disk.

use gtk4::prelude::*;
use gtk4::{glib, gio};
use libadwaita as adw;
use libadwaita::prelude::*;
use ashpd::desktop::screenshot::Screenshot;
use chrono::Local;
use gettextrs::gettext;
use std::path::PathBuf;

/// Entry point for GUI-mode capture.
///
/// Disables the capture button immediately to prevent concurrent requests,
/// then either starts a visual countdown or hides the window and invokes
/// the portal after a 200 ms compositor settling period.
pub fn start_capture(window: &crate::window::SuperShotWindow, delay_seconds: u32) {
    let window = window.clone();

    // Guard against concurrent captures for all delay values.
    window.set_capture_sensitive(false);

    // Prevent GTK from auto-quitting when the window is hidden.
    // GtkApplication terminates when no visible windows remain. The RAII
    // ApplicationHoldGuard keeps the app alive; it is moved through the closure
    // chain and automatically calls g_application_release() when dropped at
    // the end of perform_screenshot().
    let hold = gio::Application::default().map(|app| app.hold());

    if delay_seconds > 0 {
        start_countdown(window, delay_seconds, hold);
    } else {
        gtk4::prelude::WidgetExt::set_visible(&window, false);
        glib::timeout_add_local_once(
            std::time::Duration::from_millis(200),
            move || {
                perform_screenshot(window, hold);
            },
        );
    }
}

/// Drive the visual countdown timer.
///
/// Uses a one-second periodic GLib timeout to decrement the remaining count.
/// On each tick the window's countdown label is updated. When the counter
/// reaches zero, the window is hidden and the screenshot is taken after the
/// standard 200 ms compositor settling delay.
fn start_countdown(window: crate::window::SuperShotWindow, seconds: u32, hold: Option<gio::ApplicationHoldGuard>) {
    let remaining = std::rc::Rc::new(std::cell::Cell::new(seconds));
    // Wrap the hold guard in Rc<RefCell> so it can be moved out of the periodic closure.
    let hold = std::rc::Rc::new(std::cell::RefCell::new(hold));
    window.show_countdown(seconds);

    glib::timeout_add_seconds_local(1, move || {
        let r = remaining.get() - 1;
        remaining.set(r);

        if r == 0 {
            window.hide_countdown();
            gtk4::prelude::WidgetExt::set_visible(&window, false);

            let window_clone = window.clone();
            let hold_inner = hold.borrow_mut().take();
            glib::timeout_add_local_once(
                std::time::Duration::from_millis(200),
                move || {
                    perform_screenshot(window_clone, hold_inner);
                },
            );
            glib::ControlFlow::Break
        } else {
            window.show_countdown(r);
            glib::ControlFlow::Continue
        }
    });
}

/// Entry point for headless (CLI) capture.
///
/// Invoked when the `--now` flag is passed. No window is created; the
/// screenshot is taken directly via the portal. An optional delay is
/// implemented as a one-shot GLib timeout. After capture (or cancellation),
/// the application exits via `gio::Application::quit()`.
pub fn start_headless(delay_seconds: u32) {
    // Headless mode has no visible windows; the hold guard prevents auto-quit.
    let _hold = gio::Application::default().map(|app| app.hold());

    if delay_seconds > 0 {
        eprintln!("Waiting {} seconds...", delay_seconds);
        glib::timeout_add_seconds_local_once(delay_seconds, move || {
            let _h = _hold;
            perform_headless_screenshot();
        });
    } else {
        perform_headless_screenshot();
    }
}

/// Execute the portal screenshot request in headless mode.
/// Saves the result to disk and terminates the application.
fn perform_headless_screenshot() {
    glib::spawn_future_local(async move {
        let request_result = Screenshot::request()
            .interactive(true)
            .send()
            .await;

        match request_result {
            Ok(request) => {
                match request.response() {
                    Ok(response) => {
                        let uri = response.uri();
                        let uri_string = uri.as_str().to_string();
                        match save_screenshot_to_disk(&uri_string).await {
                            Ok(dest_path) => {
                                eprintln!("Screenshot saved to: {}", dest_path.display());
                            }
                            Err(e) => eprintln!("Error: {}", e),
                        }
                    },
                    Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled))
                    | Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Other)) => {},
                    Err(e) => eprintln!("Portal error: {}", e),
                }
            },
            Err(e) => eprintln!("Request error: {}", e),
        }

        if let Some(app) = gio::Application::default() {
            app.quit();
        }
    });
}

/// Present a modal error dialog using AdwAlertDialog.
/// The dialog is non-blocking; the GLib main loop continues while it is shown.
fn show_error_dialog(window: &crate::window::SuperShotWindow, heading: &str, body: &str) {
    let dialog = adw::AlertDialog::new(Some(&gettext(heading)), Some(body));
    dialog.add_response("ok", "OK");
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("ok");
    dialog.present(Some(window));
}

/// Execute the portal screenshot request in GUI mode.
///
/// Spawns an async future on the GLib main loop. On success, saves the
/// image, copies to clipboard, and sends a desktop notification. On failure,
/// presents an error dialog. The window is re-shown and the capture button
/// re-enabled after the operation completes, regardless of outcome.
fn perform_screenshot(window: crate::window::SuperShotWindow, _hold: Option<gio::ApplicationHoldGuard>) {
    glib::spawn_future_local(async move {
        let request_result = Screenshot::request()
            .interactive(true)
            .send()
            .await;

        match request_result {
            Ok(request) => {
                match request.response() {
                    Ok(response) => {
                        let uri = response.uri();
                        let uri_string = uri.as_str().to_string();

                        match save_screenshot_to_disk(&uri_string).await {
                            Ok(dest_path) => {
                                if let Err(e) = copy_to_clipboard(&window, &dest_path) {
                                    eprintln!("Clipboard error: {}", e);
                                }
                                send_notification(&dest_path.to_string_lossy());
                            },
                            Err(e) => {
                                show_error_dialog(&window, "Save Error",
                                    &format!("Failed to save screenshot:\n{}", e));
                            }
                        }
                    },
                    Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled))
                    | Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Other)) => {},
                    Err(e) => {
                        show_error_dialog(&window, "Portal Error",
                            &format!("Screenshot failed:\n{}", e));
                    }
                }
            },
            Err(e) => {
                show_error_dialog(&window, "Request Error",
                    &format!("Could not request screenshot:\n{}", e));
            }
        }

        window.set_capture_sensitive(true);
        gtk4::prelude::WidgetExt::set_visible(&window, true);
        gtk4::prelude::GtkWindowExt::present(&window);

        // _hold guard is dropped here, automatically calling g_application_release().
        drop(_hold);
    });
}

/// Send a desktop notification indicating a successful capture.
/// Uses the GIO notification API, which integrates with the GNOME notification daemon.
fn send_notification(dest_path: &str) {
    if let Some(app) = gio::Application::default() {
        let notification = gio::Notification::new(&gettext("Screenshot captured"));
        notification.set_body(Some(&gettext("Saved to %s").replace("%s", dest_path)));
        app.send_notification(Some("screenshot-captured"), &notification);
    }
}

/// Save the portal screenshot to ~/Pictures/Screenshots/.
///
/// The portal stores its screenshot in a temporary location and returns a `file://` URI.
/// This function copies it to the user's Pictures/Screenshots directory with a
/// timestamp-based filename including millisecond precision to prevent overwrites
/// from same-second captures. The async file copy uses GIO's future-based API
/// to avoid blocking the main loop.
///
/// Returns the absolute path of the saved file on success.
async fn save_screenshot_to_disk(uri_str: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let user_dirs = dirs::picture_dir().ok_or("Cannot find Pictures directory")?;
    let save_dir = user_dirs.join("Screenshots");

    if !save_dir.exists() {
        std::fs::create_dir_all(&save_dir)?;
    }

    let now = Local::now();
    let filename = format!("Screenshot_{}.png", now.format("%Y-%m-%d_%H-%M-%S%.3f"));
    let dest_path = save_dir.join(&filename);

    let src_file = gio::File::for_uri(uri_str);
    let dest_file = gio::File::for_path(&dest_path);

    let (copy_res, _) = src_file.copy_future(
        &dest_file,
        gio::FileCopyFlags::OVERWRITE,
        glib::Priority::DEFAULT,
    );

    copy_res.await?;

    Ok(dest_path)
}

/// Copy a saved screenshot to the system clipboard via GDK Texture.
fn copy_to_clipboard(
    window: &crate::window::SuperShotWindow,
    path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = gio::File::for_path(path);
    let texture = gtk4::gdk::Texture::from_file(&file)?;
    let clipboard = gtk4::prelude::WidgetExt::clipboard(window);
    clipboard.set_texture(&texture);
    Ok(())
}
