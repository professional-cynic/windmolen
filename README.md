<p align="center">
  <img src="res/windmolen.png" alt="Windmolen icon" width="128" height="128">
</p>

<h1 align="center">Windmolen</h1>

A lean, fast photo viewer for Windows, designed after the GNOME HIG and
libadwaita (colours, type scale, header bar, status pages, boxed lists,
popovers, dialogs, toasts).

- **One dependency:** Microsoft's own [`windows`](https://github.com/microsoft/windows-rs)
  crate (plus its `windows-numerics` companion). The UI toolkit, layout, icons and
  widgets are written in this crate.
- **Native stack:** Win32 window with client-side decorations, Direct2D/DirectWrite
  rendering, and the Windows Imaging Component (WIC) for decoding and encoding.
- **No admin rights:** the manifest declares `asInvoker`, and settings live in
  `%APPDATA%\Windmolen\settings.txt`. Nothing is written to HKLM or Program Files.
- **Small:** a ~750 KB executable, about 7 000 lines of Rust including tests.

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

- Open a file (or drop it on the window) and browse its folder in Explorer's natural order,
  or open just that image (**Open Image Only…**) and add the rest of its folder later with
  **Show All Images in Folder**.
- Zoom with the mouse wheel or touchpad pinch, anchored at the cursor. Animated
  zoom; double-click toggles best fit / 100 %. Drag to pan. Neighbouring images are pre-decoded at screen size; only the image
  on screen is kept at full size, for zooming, and it is drawn from a screen-sized
  copy until you zoom past it, so redrawing costs the same for any photo size.
- Properties sidebar: file, dimensions, colour profile, camera, lens, exposure, aperture, ISO, focal length, date taken.
- **Copy** (Ctrl+C) puts the image on the clipboard both as a file, which Explorer
  pastes as a copy, and as a picture for Paint, Word or a chat. The picture is only
  decoded when a program pastes it.
- **Delete** moves the image to the Recycle Bin, with **Undo** in the toast. The file
  goes there once the toast is gone (or the window closes); where a drive has no
  Recycle Bin, Windows asks before deleting it for good.
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

## Installing

Download `Windmolen-Setup.exe` from the latest release and run it. It installs for
you only, so no administrator rights are needed:

- The program goes to `%LOCALAPPDATA%\Programs\Windmolen`, with a Start menu entry
  and an entry in Settings › Apps (which uninstalls it).
- The program registers itself (`windmolen.exe --register`) for every format WIC can
  decode: per-format **Open with** entries, a right-click **Open in Windmolen** that
  shows just the chosen files, and an entry in Settings › Default apps. Codecs added
  later (HEIC, WebP, … from the Store) are registered the next time it starts.
- Windows doesn't let any app make itself the default. The installer's last page
  explains it: in Settings › Apps › Default apps, choose Windmolen and select
  **Set default** (on Windows 10, choose Windmolen under Photo viewer). It can open
  that page for you.
- Uninstalling keeps your marks and settings in `%APPDATA%`.

### What opens

- Double-clicking an image, or **Open with › Windmolen**, browses its folder, as
  Microsoft Photos does. (Windows runs the same command for both, so they can't differ.)
- Right-click › **Open in Windmolen** shows just the chosen files. **Show All Images
  in Folder** (Ctrl+B) adds the rest of the folder later.
- In the app, **Open…** (Ctrl+O) browses the folder of the image you choose, and
  **Open Image Only…** (Ctrl+Shift+O) doesn't. Choosing several files shows just those.

### One window per action

Opening several selected files (Enter, right-click › Open in Windmolen,
or Open with) shows them together in one window as a selection ("2 of 4
selected"). Each separate open gets a new window.

Explorer starts one process per selected file. The first process accepts the
others' files for about a second and a half after it starts, or while more keep
arriving. Those processes hand their file over and exit. Files opened separately
more than a moment apart get their own window. Several windows can be open at
once; marks and settings are shared safely between them.

## Updates

With **Automatic Updates** on (Preferences; on by default), the app checks GitHub at
most once a day. **Check Now** checks immediately. Each check:

1. Reads the latest release's `latest.txt` from `github.com` over HTTPS, using
   Windows' own WinHTTP and TLS stack.
2. If that release is newer, downloads its `Windmolen-Setup.exe` to the temporary
   folder and verifies its SHA-256 against `latest.txt` with Windows' BCrypt.
3. Shows a banner offering **Restart**, which checks the file again and runs the
   installer silently: it closes Windmolen, updates it and opens the image you
   were viewing again.

Only installed copies update; a `windmolen.exe` built or copied elsewhere doesn't.
Nothing is sent beyond a normal download request. The requests carry no login, so
updates only work while the repository is public.

The checksum guards against corrupted or truncated downloads; authenticity comes
from HTTPS to github.com and the release being yours. While the repository is
public, every release also gets a signed build-provenance attestation, which you
can check with
`gh attestation verify Windmolen-Setup.exe -R professional-cynic/windmolen`.
For protection even against a compromised GitHub account, releases would need
offline code signing (Authenticode), which needs a certificate.

## Security and safety

- Runs with your own rights and never asks for elevation; it writes only to
  `HKCU` and your own folders.
- Images are decoded by Windows' codecs (WIC), which Windows Update services. The
  app's own parsers (colour-profile names, settings, the update manifest, the
  hand-over between instances) are bounds-checked and unit-tested.
- Release builds load their system DLLs from System32 only, so a DLL planted next
  to the program is never loaded. CI checks the flag.
  Builds made with the GNU toolchain lack this protection.
- No file is overwritten unless you confirm **Replace Original**, which swaps the
  file atomically.
- While an image is being saved or files sent, closing waits until they are complete.
  Closing again stops the work cleanly: the file being written is removed, the
  original stays, and marks are kept only for files not yet sent. A third close
  quits at once, for when even stopping hangs (a lost network drive).
- Windows waits for that work before shutting down, showing why. If you shut down
  anyway, the work stops cleanly as above.
- A settings file that cannot be read is never saved over, so a passing read error
  cannot wipe your marks.

## Keyboard

| | |
|---|---|
| Open / open image only / show all images in folder | Ctrl+O / Ctrl+Shift+O / Ctrl+B |
| Next / previous | → ← · D A · Page Down / Up · mouse back/forward |
| First / last | Home / End |
| Zoom in / out | + − · wheel |
| Best fit / 100 % | F or Ctrl+0 / 0 or Ctrl+1 |
| Toggle mark 1–9 | 1 … 9 |
| Marks menu / remove marks / send | M / U / Ctrl+M |
| Copy / move to Recycle Bin | Ctrl+C / Delete |
| Edit · rotate · flip | E · R, Shift+R · H, V (in edit) |
| Save copy / cancel | Ctrl+S / Esc |
| Properties · fullscreen · menu | F9 or I · F11 · F10 |
| Preferences / shortcuts / quit | Ctrl+, / Ctrl+? / Ctrl+Q |

## Building

Both toolchains install per user, without admin rights.

```
rustup default stable-x86_64-pc-windows-gnu     # or -msvc
cargo build --release
```

`target\release\windmolen.exe` runs from anywhere; `pwsh installer/build.ps1 -Version
0.1.0` (after copying it to `dist\`) builds the installer.

- **GNU toolchain:** `build.rs` embeds the manifest and Explorer icon via `windres`
  (set `WINDRES` if it isn't on `PATH`). Without `windres` the app still runs, minus
  long-path support and the Explorer icon.
- **MSVC toolchain** (needs Visual Studio Build Tools): the manifest is always embedded
  by the linker, which also restricts the program's DLL imports to System32. The Explorer icon is added when `rc.exe` is found, i.e. from a Developer prompt.

`cargo test` runs the platform-independent unit tests (orientation algebra and
its mapping onto WIC's flip-rotator, crop maths, colour maths, settings parser,
path parser, metadata formatting) on any OS.

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
| `src/edit.rs`, `geom.rs`, `theme.rs`, `icons.rs`, `meta.rs`, `settings.rs`, `clip.rs` | Pure, tested logic |
| `src/install.rs`, `update.rs` | File-type registration; updates through the installer |
| `res/make-icon.py` | Draws the icon: `windmolen.png` and `windmolen.ico` with every size Windows uses (16–256 px), so Windows never shrinks it |
| `installer/` | Inno Setup script, and the scripts CI uses to build and test it |

## Known limitations

- **Not literally libadwaita.** It reproduces libadwaita's look and behaviour
  natively; it doesn't link GTK.
- **No UI Automation yet.** Screen readers can't see the custom-drawn controls.
  This is the largest gap against the HIG. Keyboard access is complete.
- Animated GIF/WebP show their first frame; editing them is disabled.
- Colour is managed to sRGB, not to the monitor's own profile.
- Windows 11 lists the right-click verb under **Show more options** (Shift+F10). The new
  top-level menu only accepts apps with package identity (a signed MSIX), which this
  deliberately avoids.
- Two separate opens less than about 1.5 s apart are treated as one selection.
- Tested here through unit tests and under Wine (rendering, navigation, marks,
  send, crop and save, install/uninstall registration, multi-file grouping).
  Explorer itself and the Settings default-apps page can't be exercised under Wine. Wine 9 lacks WIC rotation
  (newer Wine has it) and JPEG metadata, so those paths, and the real DWM frame, still need a check on Windows;
  the order in which WIC flips and rotates is pinned by a unit test modelled on Wine's implementation.

## License

Copyright (C) 2026 professional-cynic

Windmolen is free software, licensed under the GNU Affero General Public License
v3.0. See [LICENSE](LICENSE).
