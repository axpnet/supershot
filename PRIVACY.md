# Privacy Policy

## SuperShot -- Privacy Statement

Last updated: 2026-02-11

### Scope

This document describes the data handling practices of SuperShot, a desktop
screenshot capture application for GNOME.

### Data Collection

SuperShot does not collect, transmit, store, or process any personal data
beyond the local system on which it is installed. The application operates
entirely offline and makes no network connections of any kind.

### Screenshot Files

All captured screenshots are saved exclusively to the local filesystem at
`~/Pictures/Screenshots/`. File names are generated from the system clock
timestamp. No image data is transmitted to external servers, cloud storage
services, or third-party endpoints.

### Clipboard

The captured image is automatically copied to the local system clipboard via
the GDK clipboard API. This operation is confined to the local desktop
session and does not involve network transfer.

### GSettings

User preferences (delay, format, watermark, preview, save directory) are stored
locally via the GNOME GSettings subsystem, backed by the dconf database on the
user's machine. No preference data leaves the local system.

### Desktop Notifications

Capture success notifications are delivered through the local GNOME
notification daemon (via the GIO Notification API). Notification content
consists solely of the file path of the saved screenshot. Clicking a
notification opens the screenshot file using the system's default image
viewer via `g_app_info_launch_default_for_uri`, which is a local operation.
No notification data is sent to external services.

### XDG Desktop Portal

Screenshot capture is performed through the XDG Desktop Portal D-Bus
interface, which is a standard component of the GNOME desktop environment.
The portal operates locally between the application and the compositor.
No data passes through external servers.

### Sound Playback

The optional shutter sound is played through the local audio subsystem
(libcanberra or PulseAudio). No audio data is transmitted externally.

### Third-Party Dependencies

SuperShot's runtime dependencies (GTK4, Libadwaita, ashpd, GLib) are
standard GNOME platform libraries. None of these dependencies introduce
telemetry, analytics, or remote data collection as used by this application.

### Changes to This Policy

Any changes to these data handling practices will be documented in this file
and noted in the project changelog.

### Contact

For questions regarding this privacy policy:
axpnet -- https://github.com/axpnet
