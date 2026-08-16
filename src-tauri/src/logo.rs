//! **The shop's logo** — P31, and the hole D37 left open for four sessions.
//!
//! # What was wrong
//!
//! `mb-print` has drawn a logo since P07: [`mb_print::image`] is the one-bit
//! format, [`mb_print::raster`] blits it, the bill template places it, and
//! `receipt.logo` / `receipt.logo_width_pct` have been on the settings screen
//! the whole time. **And `BillContext.logo` was `None` at every call site in
//! the product**, so the settings pointed at a feature nothing could feed. The
//! owner asked for the missing half: *"Logo: browse and pick a PNG."*
//!
//! # Where the conversion happens, and why not here
//!
//! D37 decided this and it still holds:
//!
//! > *"A PNG decoder is a dependency, an inflate implementation and a parser
//! > being fed a file a shopkeeper uploaded, all to answer a question with two
//! > possible answers per dot."*
//!
//! So the split is:
//!
//! | | |
//! |---|---|
//! | pick the file | here — a webview cannot give anyone a real path ([`pick_a_logo`]) |
//! | decode the PNG, threshold it, show the result | the browser, which does all three for free |
//! | store the dots, and hand them to the printer | here ([`save_logo`], [`stored`]) |
//!
//! **The shop sees exactly what will print, at the moment it chooses**, which
//! is what D37 wanted and what a threshold done at print time could never give.
//!
//! # Where it is kept
//!
//! `attachments/logo.mb1`, beside the database — the same folder purchase
//! photographs go in. **It travels with the shop**: a restore brings the logo
//! back, and moving the data file to another machine moves the letterhead with
//! it. In the config folder it would have been left behind by both.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::state::App;
use crate::words::{UiError, UiResult};

/// The most dots a logo may be, and it is generous on purpose.
///
/// 832 is the widest thermal head this product knows about, and a metre of
/// paper is 1200-odd dots tall. Anything past that is not a logo somebody
/// meant to choose — and `mb_print::image` refuses it at decode anyway, so this
/// is only here to say so in words while the person is still looking at it.
const MAX_DOTS: usize = 832 * 1200;

/// What the screen shows about the logo this shop has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct LogoView {
    pub has_one: bool,
    /// "220 × 80 dots — about 30 mm wide on 80 mm paper". Written here, because
    /// dots per millimetre is a fact about the paper and not about the screen.
    pub says: String,
    /// The dots, so the screen can draw exactly what will print rather than a
    /// second rendering of the original file. `null` when there is none.
    ///
    /// Sent as the picture itself and not as a path: the webview cannot read a
    /// file, and this is a few kilobytes.
    pub dots: Option<Dots>,
}

/// A one-bit picture, unpacked for a canvas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct Dots {
    pub width: u32,
    pub height: u32,
    /// One byte per dot, row by row: 1 is ink. Fat compared with the packed
    /// form and far simpler to draw, and a logo is small.
    pub ink: Vec<u8>,
}

/// A file somebody chose, on its way to the browser to be converted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PickedFile {
    /// What to call it on screen — "shop-logo.png".
    pub name: String,
    /// `data:image/png;base64,…`, which is what an `<img>` takes.
    pub data_url: String,
}

/// Where this shop's logo lives.
fn path_for(app: &App) -> UiResult<std::path::PathBuf> {
    let dir = app.with_shop(|shop| Ok(mb_db::backup::attachments_dir(shop.db.path())))?;
    Ok(dir.join("logo.mb1"))
}

/// **The bytes the bill template wants**, or nothing.
///
/// Never an error. A logo that will not read is a logo that does not print;
/// it is not a bill that does not print (D37, and requirement 3 of the ten).
#[must_use]
pub fn stored(app: &App) -> Option<Vec<u8>> {
    let path = path_for(app).ok()?;
    let bytes = std::fs::read(&path).ok()?;
    // Checked here rather than trusted, so a corrupt file is one log line now
    // instead of a note on every bill for the rest of the day.
    match mb_print::image::Monochrome::decode(&bytes) {
        Ok(_) => Some(bytes),
        Err(e) => {
            crate::log_warn!("this shop's logo will not read and will not print: {e}");
            None
        }
    }
}

fn look(app: &App) -> UiResult<LogoView> {
    let Some(bytes) = stored(app) else {
        return Ok(LogoView {
            has_one: false,
            says: String::new(),
            dots: None,
        });
    };
    let picture = mb_print::image::Monochrome::decode(&bytes).map_err(|e| {
        UiError::new("logo.read", "This shop's logo could not be read.")
            .with_detail(e.to_string())
    })?;

    // **Said in millimetres, because that is the unit on the roll in the
    // shop's hand** — nobody buys paper by the dot.
    //
    // 80 mm paper is 576 dots across (`PaperKind::Mm80`), so the conversion is
    // `dots × 80 ÷ 576`. Integers, not a float: the workspace denies
    // floating-point arithmetic outright (D7), and while nothing here is money
    // the rule is worth keeping whole — a division that truncates to the
    // nearest millimetre is exactly right for a sentence that already says
    // "about".
    #[allow(
        clippy::integer_division,
        reason = "dots into whole millimetres for a sentence that says \"about\""
    )]
    let mm = u64::from(picture.width) * 80 / 576;
    let says = format!(
        "{} × {} dots — about {mm} mm wide on 80 mm paper",
        picture.width, picture.height
    );

    let mut ink = Vec::with_capacity((picture.width * picture.height) as usize);
    for y in 0..picture.height {
        for x in 0..picture.width {
            ink.push(u8::from(picture.ink(x, y)));
        }
    }

    Ok(LogoView {
        has_one: true,
        says,
        dots: Some(Dots {
            width: picture.width,
            height: picture.height,
            ink,
        }),
    })
}

/// **Browse for a picture.**
///
/// Returns the file as a data URL rather than a path, because the next thing
/// that happens to it is `<img src=…>` in the webview — and a webview cannot
/// open a path even when it has one. `None` means the person pressed Cancel,
/// which is not a failure and must not raise anything.
pub fn pick_a_logo_on(app: &App, window: &tauri::Window) -> UiResult<Option<PickedFile>> {
    guard_it(app)?;
    use tauri_plugin_dialog::DialogExt;

    let picked = window
        .dialog()
        .file()
        .add_filter("Pictures", &["png", "jpg", "jpeg", "gif", "bmp"])
        .set_title("Choose your logo")
        .blocking_pick_file();

    let Some(picked) = picked else { return Ok(None) };
    let path = picked
        .into_path()
        .map_err(|e| UiError::new("logo.path", "That file could not be opened.").with_detail(e.to_string()))?;

    let bytes = std::fs::read(&path)
        .map_err(|e| crate::words::from_io("Reading that picture", &e))?;

    // A picture bigger than this is a photograph somebody chose by mistake.
    // Said now, while they are still in the dialog's frame of mind, rather
    // than after the browser has spent ten seconds decoding it.
    const MAX_FILE: usize = 8 * 1024 * 1024;
    if bytes.len() > MAX_FILE {
        return Err(UiError::new(
            "logo.size",
            "That picture is very large. A logo only needs to be a few hundred \
             dots across — try a smaller file.",
        ));
    }

    let mime = match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        _ => "image/png",
    };

    Ok(Some(PickedFile {
        name: path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("logo")
            .to_owned(),
        data_url: format!("data:{mime};base64,{}", base64_encode(&bytes)),
    }))
}

/// **Keep the dots the browser made.**
///
/// The argument is the encoded `MB1` picture, base64'd — the same shape
/// `attach_photo` takes a photograph in, and for the same reason: the webview
/// has bytes and JSON has no way to carry them.
pub fn save_logo_on(app: &App, encoded: String) -> UiResult<LogoView> {
    guard_it(app)?;

    let payload = encoded
        .split_once(',')
        .map_or(encoded.as_str(), |(_, rest)| rest);
    let bytes = crate::buying::base64_decode(payload)
        .ok_or_else(|| UiError::new("logo.data", "That logo could not be read."))?;

    // **Decoded before it is written**, so a picture that would be skipped at
    // print time is refused now, in front of the person who chose it. A logo
    // that silently does not print is the worst version of this feature.
    let picture = mb_print::image::Monochrome::decode(&bytes).map_err(|e| {
        UiError::new(
            "logo.format",
            "That picture could not be turned into printable dots. Try a PNG.",
        )
        .with_detail(e.to_string())
    })?;
    if (picture.width as usize) * (picture.height as usize) > MAX_DOTS {
        return Err(UiError::new(
            "logo.size",
            "That logo is bigger than a roll of paper. Make it smaller and try again.",
        ));
    }

    let path = path_for(app)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| crate::words::from_io("Making the logo folder", &e))?;
    }
    std::fs::write(&path, &bytes).map_err(|e| crate::words::from_io("Saving the logo", &e))?;
    crate::log_info!(
        "this shop's logo is now {}x{} dots",
        picture.width,
        picture.height
    );
    look(app)
}

/// Take it off. The file goes; the `receipt.logo` position setting does not,
/// because a shop that removes one picture to choose another should not have to
/// switch the setting back on afterwards.
pub fn remove_logo_on(app: &App) -> UiResult<LogoView> {
    guard_it(app)?;
    let path = path_for(app)?;
    match std::fs::remove_file(&path) {
        Ok(()) => crate::log_info!("this shop's logo was removed"),
        // Already gone is the state that was asked for.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(crate::words::from_io("Removing the logo", &e)),
    }
    look(app)
}

/// The logo is part of what a bill looks like, so it is the printer permission
/// — the same one that owns the paper size and the offsets.
fn guard_it(app: &App) -> UiResult<()> {
    crate::guard::require(app, mb_auth::Permission::SettingsPrinter)?;
    Ok(())
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

// ---------------------------------------------------------------------------
// The commands.
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn logo(app: tauri::State<'_, App>) -> UiResult<LogoView> {
    crate::guard::require(&app, mb_auth::Permission::SettingsPrinter)?;
    look(&app)
}

#[tauri::command]
pub fn pick_a_logo(
    app: tauri::State<'_, App>,
    window: tauri::Window,
) -> UiResult<Option<PickedFile>> {
    pick_a_logo_on(&app, &window)
}

#[tauri::command]
pub fn save_logo(app: tauri::State<'_, App>, encoded: String) -> UiResult<LogoView> {
    save_logo_on(&app, encoded)
}

#[tauri::command]
pub fn remove_logo(app: tauri::State<'_, App>) -> UiResult<LogoView> {
    remove_logo_on(&app)
}

/// **Browse for a folder** — the first run's half of the same problem.
///
/// `Access::FirstRun`, the same as `create_shop`: on a machine with no shop
/// there is nobody to hold a permission, and the moment there IS one, pointing
/// this till at a different folder stops being set-up and becomes a
/// backup-level decision. `only_before_set_up` is that rule, and it is shared
/// with the three commands next to it in the table rather than written twice.
#[tauri::command]
pub fn pick_a_folder(
    app: tauri::State<'_, App>,
    window: tauri::Window,
    start: Option<String>,
) -> UiResult<Option<String>> {
    crate::firstrun::only_before_set_up(&app)?;
    use tauri_plugin_dialog::DialogExt;

    let mut dialog = window.dialog().file().set_title("Where should your shop's data be kept?");
    // Opening in the folder they are being offered means the common answer is
    // one click away, and an unusual one starts from somewhere familiar.
    if let Some(start) = start.as_deref().filter(|s| !s.trim().is_empty()) {
        let at = std::path::Path::new(start.trim());
        let at = if at.is_dir() { at } else { at.parent().unwrap_or(at) };
        if at.is_dir() {
            dialog = dialog.set_directory(at);
        }
    }

    let Some(picked) = dialog.blocking_pick_folder() else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|e| {
        UiError::new("folder.path", "That folder could not be used.").with_detail(e.to_string())
    })?;
    Ok(Some(path.display().to_string()))
}
