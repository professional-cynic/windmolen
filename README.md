# Het Plantsoen

A lean, fast photo viewer for Windows, designed after the GNOME HIG and
libadwaita (colours, type scale, header bar, status pages, boxed lists,
popovers, dialogs, toasts). `het-plantsoen.html` is the original prototype.

- **One dependency:** Microsoft's own [`windows`](https://github.com/microsoft/windows-rs)
  crate (plus its `windows-numerics` companion). The UI toolkit, layout, icons and
  widgets are written in this crate.
- **Native stack:** Win32 window with client-side decorations, Direct2D/DirectWrite
  rendering, and the Windows Imaging Component (WIC) for decoding and encoding.
- **No admin rights:** the manifest declares `asInvoker`, and settings live in
  `%APPDATA%\Het Plantsoen\settings.txt`. Nothing is written to HKLM or Program Files.
- **Small:** a ~750 KB executable, about 6 000 lines of Rust including tests.

## Formats

Decoding uses WIC, so the viewer opens whatever codecs Windows has. It
enumerates installed decoders at start-up.

| Built in | With free Microsoft Store extensions |
|---|---|
| JPEG, PNG, GIF, BMP, TIFF, ICO, JPEG XR, DNG, DDS | HEIC/HEIF (*HEIF Image Extensions* + *HEVC Video Extensions*), WebP (*WebP Image Extensions*), AVIF (*AV1 Video Extension*), camera RAW (*Raw Image Extension*), JPEG XL (Windows 11 24H2+) |

Windows Update and the Store service the codecs, so decoder security fixes
arrive without updating this app. EXIF orientation is applied. Embedded colour
profiles (Display P3, Adobe RGB, …) are converted to sRGB.

## Features

- Open a file (or drop it on the window) and browse its folder in Explorer's natural order.
- Zoom with the mouse wheel or touchpad pinch, anchored at the cursor. Animated
  zoom; double-click toggles best fit / 100 %. Drag to pan. Neighbouring images are pre-decoded.
- Properties sidebar: file, dimensions, colour profile, camera, lens, exposure, aperture, ISO, focal length, date taken.
- Editing: rotate, flip and crop (free or fixed aspect ratios).
  - **Save Copy** writes `name (edited).ext`.
  - **Replace Original…** asks for confirmation first.
  - A pure rotation of a JPEG or TIFF only rewrites the orientation tag (lossless).
  - Otherwise the image is re-encoded from the original pixels, keeping its metadata.
  - Formats without a WIC encoder (HEIC, WebP, RAW…) are saved as JPEG or PNG copies.
- **Marks:** nine marks, each tied to a destination folder; an image can carry several.
  **Send Marked Files** copies or moves everything (across sessions and folders) to the
  marks' folders. Existing files are never overwritten (`name (2).ext`); marks clear once sent.
- Follows the Windows light/dark setting and maps the Windows accent colour to the nearest
  libadwaita accent. Honours "Animation effects" off.
- Per-monitor DPI, fullscreen, Windows 11 snap layouts on the maximise button, keyboard
  navigation with a focus ring, and tooltips.

## Keyboard

| | |
|---|---|
| Open | Ctrl+O |
| Next / previous | → ← · D A · Page Down / Up · mouse back/forward |
| First / last | Home / End |
| Zoom in / out | + − · wheel |
| Best fit / 100 % | F or Ctrl+0 / 0 or Ctrl+1 |
| Toggle mark 1–9 | 1 … 9 |
| Marks menu / remove marks / send | M / U / Ctrl+M |
| Edit · rotate · flip | E · R, Shift+R · H, V (in edit) |
| Save copy / cancel | Ctrl+S / Esc |
| Properties · fullscreen · menu | F9 or I · F11 · F10 |
| Shortcuts / quit | Ctrl+? / Ctrl+Q |

## Building

Both toolchains install per user, without admin rights.

```
rustup default stable-x86_64-pc-windows-gnu     # or -msvc
cargo build --release
```

`target\release\het-plantsoen.exe` is self-contained; copy it anywhere.

- **GNU toolchain:** `build.rs` embeds the manifest and Explorer icon via `windres`
  (set `WINDRES` if it isn't on `PATH`). Without `windres` the app still runs, minus
  long-path support and the Explorer icon.
- **MSVC toolchain** (needs Visual Studio Build Tools): the manifest is always embedded
  by the linker. The Explorer icon is added when `rc.exe` is found, i.e. from a Developer prompt.

`cargo test` runs the platform-independent unit tests (orientation algebra,
crop maths, colour maths, settings parser, path parser, metadata formatting)
on any OS.

## Layout

| File | Purpose |
|---|---|
| `src/window.rs` | Win32 window, custom frame, DPI, input routing |
| `src/app.rs` | State and behaviour; returns re-entrant OS work as effects |
| `src/view.rs` | Every surface's layout, after libadwaita widgets |
| `src/ui.rs` | Immediate-mode widgets and draw commands |
| `src/render.rs` | Direct2D/DirectWrite back end |
| `src/wic.rs` | Decoding thread, metadata, colour management, saving |
| `src/sys.rs` | Shell dialogs, theme settings, locale formatting, no-overwrite file ops |
| `src/edit.rs`, `geom.rs`, `theme.rs`, `icons.rs`, `meta.rs`, `settings.rs` | Pure, tested logic |

## Known limitations

- **Not literally libadwaita.** It reproduces libadwaita's look and behaviour
  natively; it doesn't link GTK.
- **No UI Automation yet.** Screen readers can't see the custom-drawn controls.
  This is the largest gap against the HIG. Keyboard access is complete.
- Animated GIF/WebP show their first frame; editing them is disabled.
- Colour is managed to sRGB, not to the monitor's own profile.
- No clipboard, delete-to-Recycle-Bin or file associations yet.
- Tested here through unit tests and under Wine (rendering, navigation, marks,
  send, crop and save). Wine lacks WIC rotation and JPEG metadata, so those
  paths, and the real DWM frame, still need a check on Windows.
