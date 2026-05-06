// SuperShot - Screenshot capture pipeline
// Copyright (c) 2026 axpnet <https://github.com/axpnet>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Orchestrates the full capture workflow: optional countdown, window hiding,
// XDG Desktop Portal screenshot request, post-processing (crop, watermark,
// format conversion), file persistence, clipboard copy, and desktop notification.
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

// ---------------------------------------------------------------------------
// Display backend detection & X11 fallback
// ---------------------------------------------------------------------------

/// Check if the current session is running on Wayland.
fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|v| v == "wayland")
            .unwrap_or(false)
}

/// Attempt an X11 screenshot using available CLI tools.
/// Tries in order: gnome-screenshot, xfce4-screenshooter, scrot, maim,
/// flameshot, and finally ImageMagick's `import` (full-screen capture).
/// Returns the file:// URI of the captured screenshot on success.
fn x11_screenshot_sync(tmp_path: &str) -> Result<String, String> {
    // Interactive CLI tools (ESC supported by gnome-screenshot, scrot, maim, flameshot)
    let interactive_tools: &[(&str, &[&str])] = &[
        ("gnome-screenshot", &["-a", "-f", tmp_path]),
        ("xfce4-screenshooter", &["-r", "-s", tmp_path]),
        ("scrot", &["-s", tmp_path]),
        ("maim", &["-s", tmp_path]),
        ("flameshot", &["gui", "--raw"]),
    ];

    for (cmd, args) in interactive_tools {
        if *cmd == "flameshot" {
            if let Ok(output) = std::process::Command::new(cmd).args(*args).output() {
                if output.status.success()
                    && !output.stdout.is_empty()
                    && std::fs::write(tmp_path, &output.stdout).is_ok()
                {
                    return Ok(format!("file://{}", tmp_path));
                }
            }
            continue;
        }
        match std::process::Command::new(cmd).args(*args).status() {
            Ok(status) if status.success() && std::path::Path::new(tmp_path).exists() => {
                return Ok(format!("file://{}", tmp_path));
            }
            Ok(_) => continue,
            Err(_) => continue,
        }
    }

    // Last resort: ImageMagick `import` (right-click to cancel, ESC not supported)
    match std::process::Command::new("import")
        .arg(tmp_path)
        .status()
    {
        Ok(status) if status.success() && std::path::Path::new(tmp_path).exists() => {
            return Ok(format!("file://{}", tmp_path));
        }
        _ => {}
    }

    Err("No screenshot tool available. Install gnome-screenshot: sudo apt install gnome-screenshot".to_string())
}

/// Aggregates user-configurable capture settings.
///
/// Constructed from widget state in `window.rs` and threaded through the
/// capture pipeline so each stage can consult active settings without
/// reaching back into window state.
#[derive(Clone, Debug, Default)]
pub struct CaptureOptions {
    /// Output format: 0 = PNG, 1 = JPEG.
    pub format_idx: u32,
    /// Whether to overlay a date/time watermark.
    pub watermark: bool,
    /// Watermark date format preset index (0-4).
    pub watermark_format: u32,
    /// Optional custom text prepended to the watermark date (e.g. brand name).
    pub watermark_text: String,
    /// Watermark corner: 0 = bottom-right, 1 = bottom-left, 2 = top-right, 3 = top-left.
    pub watermark_position: u32,
    /// Watermark text color: 0 = white (dark shadow), 1 = black (light shadow).
    pub watermark_color: u32,
    /// Custom save directory, or None for the default ~/Pictures/Screenshots/.
    pub save_dir: Option<PathBuf>,
    /// Whether to show the preview/crop window before saving.
    pub show_preview: bool,
    /// Optional crop rectangle (x, y, width, height) in image pixels.
    /// Set by the preview window; None when capturing without preview.
    pub crop: Option<(i32, i32, i32, i32)>,
}

/// Date format presets for the watermark.
const WATERMARK_FORMATS: &[&str] = &[
    "%Y-%m-%d %H:%M:%S",   // 0: 2026-02-16 14:30:25
    "%d/%m/%Y %H:%M",      // 1: 16/02/2026 14:30
    "%b %d, %Y %H:%M",     // 2: Feb 16, 2026 14:30
    "%Y-%m-%d",             // 3: 2026-02-16
    "%H:%M:%S",             // 4: 14:30:25
];

/// Entry point for GUI-mode capture.
///
/// Disables the capture button immediately to prevent concurrent requests,
/// then either starts a visual countdown or hides the window and invokes
/// the portal after a 200 ms compositor settling period.
pub fn start_capture(
    window: &crate::window::SuperShotWindow,
    delay_seconds: u32,
    options: CaptureOptions,
) {
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
        start_countdown(window, delay_seconds, hold, options);
    } else {
        gtk4::prelude::WidgetExt::set_visible(&window, false);
        glib::timeout_add_local_once(
            std::time::Duration::from_millis(200),
            move || {
                perform_screenshot(window, hold, options);
            },
        );
    }
}

/// Drive the visual countdown timer.
///
/// Uses a one-second periodic GLib timeout to decrement the remaining count.
/// On each tick the window's countdown display is updated. When the counter
/// reaches zero, the window is hidden and the screenshot is taken after the
/// standard 200 ms compositor settling delay.
fn start_countdown(
    window: crate::window::SuperShotWindow,
    seconds: u32,
    hold: Option<gio::ApplicationHoldGuard>,
    options: CaptureOptions,
) {
    let remaining = std::rc::Rc::new(std::cell::Cell::new(seconds));
    // Wrap the hold guard in Rc<RefCell> so it can be moved out of the periodic closure.
    let hold = std::rc::Rc::new(std::cell::RefCell::new(hold));
    let options = std::rc::Rc::new(options);
    window.show_countdown(seconds);

    glib::timeout_add_seconds_local(1, move || {
        let r = remaining.get() - 1;
        remaining.set(r);

        if r == 0 {
            window.hide_countdown();
            gtk4::prelude::WidgetExt::set_visible(&window, false);

            let window_clone = window.clone();
            let hold_inner = hold.borrow_mut().take();
            let opts = (*options).clone();
            glib::timeout_add_local_once(
                std::time::Duration::from_millis(200),
                move || {
                    perform_screenshot(window_clone, hold_inner, opts);
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

/// Execute the screenshot request in headless mode.
/// Detects Wayland vs X11 and uses the appropriate backend.
fn perform_headless_screenshot() {
    if is_wayland() {
        perform_headless_wayland();
    } else {
        perform_headless_x11();
    }
}

/// Headless Wayland: use the XDG Desktop Portal.
fn perform_headless_wayland() {
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
                        match process_and_save(&uri_string, &CaptureOptions::default()) {
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

/// Headless X11: fall back to CLI screenshot tools.
fn perform_headless_x11() {
    let tmp_path = format!("/tmp/supershot_{}.png", std::process::id());
    let tmp_clone = tmp_path.clone();

    glib::spawn_future_local(async move {
        let result = tokio::task::spawn_blocking(move || {
            x11_screenshot_sync(&tmp_clone)
        }).await;

        match result {
            Ok(Ok(uri_string)) => {
                match process_and_save(&uri_string, &CaptureOptions::default()) {
                    Ok(dest_path) => {
                        eprintln!("Screenshot saved to: {}", dest_path.display());
                    }
                    Err(e) => eprintln!("Error: {}", e),
                }
            }
            Ok(Err(e)) => eprintln!("Screenshot error: {}", e),
            Err(e) => eprintln!("Screenshot tool failed: {}", e),
        }

        if let Some(app) = gio::Application::default() {
            app.quit();
        }
    });
}

/// Present a modal error dialog using AdwAlertDialog.
/// The dialog is non-blocking; the GLib main loop continues while it is shown.
pub(crate) fn show_error_dialog(window: &crate::window::SuperShotWindow, heading: &str, body: &str) {
    let dialog = adw::AlertDialog::new(Some(&gettext(heading)), Some(body));
    dialog.add_response("ok", "OK");
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("ok");
    dialog.present(Some(window));
}

/// Execute the screenshot request in GUI mode.
///
/// Always tries the XDG Desktop Portal first (works on both Wayland and X11
/// with a portal backend). Falls back to CLI tools on X11 only if the portal
/// is unavailable.
fn perform_screenshot(
    window: crate::window::SuperShotWindow,
    _hold: Option<gio::ApplicationHoldGuard>,
    options: CaptureOptions,
) {
    perform_screenshot_portal(window, _hold, options);
}

/// Try the XDG Desktop Portal first, fall back to X11 CLI tools if the portal
/// is not available (request error). User cancellation is not a fallback trigger.
fn perform_screenshot_portal(
    window: crate::window::SuperShotWindow,
    _hold: Option<gio::ApplicationHoldGuard>,
    options: CaptureOptions,
) {
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
                        handle_screenshot_result(&window, uri_string, options, _hold);
                        return;
                    },
                    Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled))
                    | Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Other)) => {
                        // User cancelled — just re-show the window, no fallback.
                    },
                    Err(e) => {
                        show_error_dialog(&window, "Portal Error",
                            &format!("Screenshot failed:\n{}", e));
                    }
                }
            },
            Err(_) if !is_wayland() => {
                // Portal unavailable on X11 — fall back to CLI tools.
                perform_screenshot_x11_inner(&window, _hold, options).await;
                return;
            },
            Err(e) => {
                show_error_dialog(&window, "Request Error",
                    &format!("Could not request screenshot:\n{}", e));
            }
        }

        window.set_capture_sensitive(true);
        gtk4::prelude::WidgetExt::set_visible(&window, true);
        gtk4::prelude::GtkWindowExt::present(&window);
        drop(_hold);
    });
}

/// X11 fallback: use CLI screenshot tools. Called as async inner function
/// from the portal path when the portal is unavailable on X11.
async fn perform_screenshot_x11_inner(
    window: &crate::window::SuperShotWindow,
    _hold: Option<gio::ApplicationHoldGuard>,
    options: CaptureOptions,
) {
    let tmp_path = format!("/tmp/supershot_{}.png", std::process::id());
    let tmp_clone = tmp_path.clone();

    let result = tokio::task::spawn_blocking(move || {
        x11_screenshot_sync(&tmp_clone)
    }).await;

    match result {
        Ok(Ok(uri_string)) => {
            handle_screenshot_result(window, uri_string, options, _hold);
            return;
        }
        Ok(Err(e)) => {
            show_error_dialog(window, "Screenshot Error", &e);
        }
        Err(e) => {
            show_error_dialog(window, "Screenshot Error",
                &format!("Screenshot tool failed:\n{}", e));
        }
    }

    window.set_capture_sensitive(true);
    gtk4::prelude::WidgetExt::set_visible(window, true);
    gtk4::prelude::GtkWindowExt::present(window);
    drop(_hold);
}

/// Common handler for a successful screenshot URI (shared by Wayland and X11 paths).
fn handle_screenshot_result(
    window: &crate::window::SuperShotWindow,
    uri_string: String,
    options: CaptureOptions,
    _hold: Option<gio::ApplicationHoldGuard>,
) {
    if options.show_preview {
        crate::preview::PreviewWindow::present_for(
            uri_string, options, window.clone(), _hold,
        );
        return;
    }

    match process_and_save(&uri_string, &options) {
        Ok(dest_path) => {
            if let Err(e) = copy_to_clipboard(window, &dest_path) {
                eprintln!("Clipboard error: {}", e);
            }
            send_notification(&dest_path.to_string_lossy());
        },
        Err(e) => {
            show_error_dialog(window, "Save Error",
                &format!("Failed to save screenshot:\n{}", e));
        }
    }

    window.set_capture_sensitive(true);
    gtk4::prelude::WidgetExt::set_visible(window, true);
    gtk4::prelude::GtkWindowExt::present(window);
}

/// Send a desktop notification indicating a successful capture.
/// Uses the GIO notification API, which integrates with the GNOME notification daemon.
pub(crate) fn send_notification(dest_path: &str) {
    if let Some(app) = gio::Application::default() {
        let notification = gio::Notification::new(&gettext("Screenshot captured"));
        notification.set_body(Some(&gettext("Saved to %s").replace("%s", dest_path)));
        notification.set_default_action_and_target_value("app.open-screenshot", Some(&dest_path.to_variant()));
        let notif_id = format!("screenshot-{}", chrono::Local::now().format("%H%M%S%3f"));
        app.send_notification(Some(&notif_id), &notification);
    }
}

// ---------------------------------------------------------------------------
// Post-processing pipeline
// ---------------------------------------------------------------------------

/// Process and save the captured screenshot.
///
/// Applies optional crop and watermark, then saves in the chosen format.
/// The source image is the portal's temporary PNG file (identified by URI).
/// Returns the absolute path of the saved file.
pub(crate) fn process_and_save(
    uri_str: &str,
    options: &CaptureOptions,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    // 1. Resolve source file path from the portal's file:// URI.
    let src_path = gio::File::for_uri(uri_str)
        .path()
        .ok_or("Cannot convert URI to local path")?;

    // 2. Determine save directory.
    let save_dir = match &options.save_dir {
        Some(dir) => dir.clone(),
        None => {
            let user_dirs = dirs::picture_dir().ok_or("Cannot find Pictures directory")?;
            user_dirs.join("Screenshots")
        }
    };
    if !save_dir.exists() {
        std::fs::create_dir_all(&save_dir)?;
    }

    // 3. Build filename with the correct extension.
    let now = Local::now();
    let ext = if options.format_idx == 1 { "jpg" } else { "png" };
    let filename = format!("Screenshot_{}_supershot.{}", now.format("%Y-%m-%d_%H-%M-%S%.3f"), ext);
    let dest_path = save_dir.join(&filename);

    // 4. Branch based on whether watermark is needed.
    //    Watermark requires Cairo for text rendering; the no-watermark path
    //    uses GdkPixbuf which is simpler and avoids a format round-trip.
    if options.watermark {
        save_with_watermark(&src_path, &dest_path, options)?;
    } else {
        save_without_watermark(&src_path, &dest_path, options)?;
    }

    // 5. Remove the portal's original file to prevent duplicates.
    //    The XDG Screenshot portal saves its own copy (e.g. ~/Pictures/Screenshots/
    //    or the locale-specific equivalent). Since we have already saved a processed
    //    copy to our target directory, the portal's original is redundant.
    if src_path != dest_path {
        let _ = std::fs::remove_file(&src_path);
    }

    Ok(dest_path)
}

/// Process and save a screenshot with editing operations applied.
///
/// Used by the preview window when edits (rotate, flip, brightness, etc.)
/// have been made. The pipeline: load → edit → crop → watermark → save.
pub(crate) fn process_and_save_edited(
    uri_str: &str,
    options: &CaptureOptions,
    edits: &crate::editing::EditState,
    crop: Option<(i32, i32, i32, i32)>,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    // 1. Resolve source file path.
    let src_path = gio::File::for_uri(uri_str)
        .path()
        .ok_or("Cannot convert URI to local path")?;

    // 2. Determine save directory.
    let save_dir = match &options.save_dir {
        Some(dir) => dir.clone(),
        None => {
            let user_dirs = dirs::picture_dir().ok_or("Cannot find Pictures directory")?;
            user_dirs.join("Screenshots")
        }
    };
    if !save_dir.exists() {
        std::fs::create_dir_all(&save_dir)?;
    }

    // 3. Build filename.
    let now = Local::now();
    let ext = if options.format_idx == 1 { "jpg" } else { "png" };
    let filename = format!("Screenshot_{}_supershot.{}", now.format("%Y-%m-%d_%H-%M-%S%.3f"), ext);
    let dest_path = save_dir.join(&filename);

    // 4. Load and apply edits using the `image` crate.
    let mut img = crate::editing::apply_edits_to_file(uri_str, edits)?;

    // 5. Apply crop on the edited image.
    if let Some((x, y, w, h)) = crop {
        img = img.crop_imm(x as u32, y as u32, w as u32, h as u32);
    }

    // 6. Apply watermark if enabled.
    //    Convert the edited image to a Cairo surface for text rendering,
    //    then save the result.
    if options.watermark {
        // Save edited image to a temp PNG, load as Cairo surface, draw watermark.
        let tmp = dest_path.with_extension("tmp.png");
        img.save(&tmp)?;

        let mut reader = std::io::BufReader::new(std::fs::File::open(&tmp)?);
        let surface = cairo::ImageSurface::create_from_png(&mut reader)?;
        apply_watermark(&surface, options)?;

        if options.format_idx == 1 {
            let mut png_buf: Vec<u8> = Vec::new();
            surface.write_to_png(&mut png_buf)?;
            let loader = gdk_pixbuf::PixbufLoader::new();
            loader.write(&png_buf)?;
            loader.close()?;
            let pixbuf = loader.pixbuf().ok_or("Failed to decode image")?;
            pixbuf.savev(&dest_path, "jpeg", &[("quality", "90")])?;
        } else {
            let mut writer = std::io::BufWriter::new(std::fs::File::create(&dest_path)?);
            surface.write_to_png(&mut writer)?;
        }
        let _ = std::fs::remove_file(&tmp);
    } else {
        crate::editing::save_edited_image(&img, &dest_path, options.format_idx)?;
    }

    // 7. Remove original portal file.
    if src_path != dest_path {
        let _ = std::fs::remove_file(&src_path);
    }

    Ok(dest_path)
}

/// Save without watermark using GdkPixbuf (fast path).
fn save_without_watermark(
    src_path: &std::path::Path,
    dest_path: &std::path::Path,
    options: &CaptureOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let pixbuf = gdk_pixbuf::Pixbuf::from_file(src_path)?;

    let pixbuf = if let Some((x, y, w, h)) = options.crop {
        pixbuf.new_subpixbuf(x, y, w, h)
    } else {
        pixbuf
    };

    if options.format_idx == 1 {
        pixbuf.savev(dest_path, "jpeg", &[("quality", "90")])?;
    } else {
        pixbuf.savev(dest_path, "png", &[])?;
    }

    Ok(())
}

/// Save with watermark using Cairo for text rendering.
fn save_with_watermark(
    src_path: &std::path::Path,
    dest_path: &std::path::Path,
    options: &CaptureOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    // Load the portal's temp PNG as a Cairo surface.
    let mut reader = std::io::BufReader::new(std::fs::File::open(src_path)?);
    let surface = cairo::ImageSurface::create_from_png(&mut reader)?;

    // Apply crop by painting the source onto a smaller surface with an offset.
    let surface = if let Some((x, y, w, h)) = options.crop {
        let cropped = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h)?;
        let ctx = cairo::Context::new(&cropped)?;
        ctx.set_source_surface(&surface, -x as f64, -y as f64)?;
        ctx.paint()?;
        drop(ctx);
        cropped
    } else {
        surface
    };

    // Draw the watermark.
    apply_watermark(&surface, options)?;

    // Save in the chosen format.
    if options.format_idx == 1 {
        // JPEG: encode surface as in-memory PNG, decode via PixbufLoader, re-save as JPEG.
        let mut png_buf: Vec<u8> = Vec::new();
        surface.write_to_png(&mut png_buf)?;
        let loader = gdk_pixbuf::PixbufLoader::new();
        loader.write(&png_buf)?;
        loader.close()?;
        let pixbuf = loader.pixbuf().ok_or("Failed to decode image")?;
        pixbuf.savev(dest_path, "jpeg", &[("quality", "90")])?;
    } else {
        let mut writer = std::io::BufWriter::new(std::fs::File::create(dest_path)?);
        surface.write_to_png(&mut writer)?;
    }

    Ok(())
}

/// Overlay a customizable watermark on the Cairo surface.
fn apply_watermark(
    surface: &cairo::ImageSurface,
    options: &CaptureOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let width = surface.width() as f64;
    let height = surface.height() as f64;
    let ctx = cairo::Context::new(surface)?;
    draw_watermark_overlay(&ctx, width, height, options)
}

/// Draw watermark text on any Cairo context at the given image dimensions.
///
/// Shared by the save pipeline (drawing on the final surface) and the preview
/// window (drawing on the display context within the image transform). The
/// caller is responsible for setting up the coordinate system so that
/// (0,0)-(width,height) maps to the image area.
pub fn draw_watermark_overlay(
    ctx: &cairo::Context,
    width: f64,
    height: f64,
    options: &CaptureOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    // Build the watermark string.
    let fmt = WATERMARK_FORMATS
        .get(options.watermark_format as usize)
        .unwrap_or(&WATERMARK_FORMATS[0]);
    let date_str = Local::now().format(fmt).to_string();
    let text = if options.watermark_text.is_empty() {
        date_str
    } else {
        format!("{} | {}", options.watermark_text, date_str)
    };

    let font_size = (height * 0.02).clamp(14.0, 36.0);
    ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    ctx.set_font_size(font_size);

    let extents = ctx.text_extents(&text)?;
    let margin = 10.0;
    let tw = extents.width();
    let th = extents.height();

    // Compute position based on the selected corner.
    let (x, y) = match options.watermark_position {
        1 => (margin, height - margin),                       // bottom-left
        2 => (width - tw - margin, margin + th),              // top-right
        3 => (margin, margin + th),                           // top-left
        _ => (width - tw - margin, height - margin),          // bottom-right (default)
    };

    // Color scheme: 0 = white text / dark shadow, 1 = black text / light shadow.
    let (shadow_r, shadow_a, text_r, text_a) = if options.watermark_color == 1 {
        (1.0, 0.4, 0.0, 0.7)   // black text, light shadow
    } else {
        (0.0, 0.5, 1.0, 0.7)   // white text, dark shadow
    };

    // Shadow (offset by 1px).
    ctx.set_source_rgba(shadow_r, shadow_r, shadow_r, shadow_a);
    ctx.move_to(x + 1.0, y + 1.0);
    ctx.show_text(&text)?;

    // Main text.
    ctx.set_source_rgba(text_r, text_r, text_r, text_a);
    ctx.move_to(x, y);
    ctx.show_text(&text)?;

    Ok(())
}

/// Copy a saved screenshot to the system clipboard via GDK Texture.
pub(crate) fn copy_to_clipboard(
    window: &crate::window::SuperShotWindow,
    path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = gio::File::for_path(path);
    let texture = gtk4::gdk::Texture::from_file(&file)?;
    let clipboard = gtk4::prelude::WidgetExt::clipboard(window);
    clipboard.set_texture(&texture);
    Ok(())
}
