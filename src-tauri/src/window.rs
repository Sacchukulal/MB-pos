//! The window — **audit F7**, which is the first thing the owner sees.
//!
//! > *"The window opens at 800×600 and then jumps to maximised, with the plain
//! > Windows title bar."*
//!
//! Two separate complaints, and both are fixed here: the size is restored
//! **before the window is shown**, and the title bar is ours (`decorations:
//! false` in `tauri.conf.json`, drawn in React).
//!
//! # Why the window starts hidden
//!
//! `visible: false` in the configuration, and shown at the end of start-up.
//! That is what removes the jump: a window that appears at its default size and
//! then resizes has already been seen doing it. It is also budget S3 — *"time
//! to first paint: something on screen, not white"* — so the gap between
//! showing and painting has to stay small, which is why nothing slow happens
//! after this point.

// The one place a float meets an integer in this crate; `whole_u32` below
// explains why the cast is written out rather than allowed wholesale.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "pixels from the window manager, range-checked at the call site"
)]

use tauri::{LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize};

use crate::config::{AppConfig, WindowState};
use crate::log_info;

/// The smallest saved size worth believing.
///
/// Not a minimum window size — Windows enforces its own — but a **sanity
/// floor** on what came out of the config file. Anything under this is not a
/// shop's preference; it is a minimised rect that got written down. The
/// reference machine is 1366 × 768, so these are far below anything a person
/// would deliberately choose.
const MIN_SENSIBLE_WIDTH: u32 = 640;
const MIN_SENSIBLE_HEIGHT: u32 = 480;

/// Put the window back where it was, then show it.
pub fn restore_and_show(window: &tauri::WebviewWindow, state: &WindowState) {
    if let (Some(width), Some(height)) = (state.width, state.height)
        // **A saved size that could not hold a counter is not a saved size.**
        //
        // The second layer of the minimised-rect fix: `remember` no longer
        // writes 144 × 19, but every machine that has already run this build
        // has that in its config file, and a shop should not have to know why
        // its window opens the size of a tooltip. An absurd size is ignored and
        // the default is used, so the file repairs itself on the next resize.
        && width >= MIN_SENSIBLE_WIDTH
        && height >= MIN_SENSIBLE_HEIGHT
    {
        let _ = window.set_size(LogicalSize::new(f64::from(width), f64::from(height)));
    }
    if let (Some(x), Some(y)) = (state.x, state.y) {
        // Only if it would land on a screen that still exists. A shop that
        // unplugs the second monitor must not get a window at x = 3000.
        if on_a_visible_screen(window, x, y) {
            let _ = window.set_position(LogicalPosition::new(f64::from(x), f64::from(y)));
        } else {
            log_info!("the saved window position is off-screen now — centring instead");
            let _ = window.center();
        }
    }
    if state.maximised {
        let _ = window.maximize();
    }
    let _ = window.show();
    let _ = window.set_focus();
}

/// Remember where it is. Called when the window moves, resizes or closes.
pub fn remember(window: &tauri::WebviewWindow, config: &mut AppConfig) {
    let maximised = window.is_maximized().unwrap_or(false);
    config.window.maximised = maximised;

    // A maximised window's size is the screen's, not the size to go back to,
    // so it is deliberately not recorded while maximised — otherwise
    // un-maximising after a restart gives a full-screen-sized "restored"
    // window, which is the same class of bug as F7.
    if maximised {
        return;
    }

    // **AND NOT WHILE MINIMISED**, which is the same argument one state along
    // and the bug it was found by.
    //
    // Windows fires a resize when a window is minimised, and the geometry it
    // reports is the minimised rect: **144 × 19 at (−32000, −32000)**. That got
    // saved, and the next start restored it — so a shop that minimised Magic
    // Bill and closed it reopened to a window the size of a tooltip, which
    // looks exactly like the product being broken.
    //
    // Found by looking at the window after the display changed: the log said
    // "the saved window position is off-screen now — centring instead", which
    // was the position half of the same bad rect.
    if window.is_minimized().unwrap_or(false) {
        return;
    }
    let scale = window.scale_factor().unwrap_or(1.0);
    if let Ok(PhysicalSize { width, height }) = window.inner_size() {
        let size = LogicalSize::<f64>::from_physical(PhysicalSize::new(width, height), scale);
        config.window.width = whole_u32(size.width);
        config.window.height = whole_u32(size.height);
    }
    if let Ok(PhysicalPosition { x, y }) = window.outer_position() {
        let at = LogicalPosition::<f64>::from_physical(PhysicalPosition::new(x, y), scale);
        config.window.x = whole_i32(at.x);
        config.window.y = whole_i32(at.y);
    }
}

/// A window dimension, as a whole number.
///
/// The workspace denies float-to-integer casts because of D7, and it is right
/// to: a silent truncation in the money path is how a shop loses a rupee. This
/// is a pixel count from the window manager, and the honest conversion is to
/// refuse a value that is not a sane window size rather than to wrap one.
/// `None` simply means the size is not remembered — the next start centres,
/// which is the same thing a first run does.
fn whole_u32(value: f64) -> Option<u32> {
    let rounded = value.round();
    if !rounded.is_finite() || rounded < 0.0 || rounded > f64::from(u32::MAX) {
        return None;
    }
    // Proven in range on the line above.
    Some(rounded as u32)
}

fn whole_i32(value: f64) -> Option<i32> {
    let rounded = value.round();
    if !rounded.is_finite() || rounded < f64::from(i32::MIN) || rounded > f64::from(i32::MAX) {
        return None;
    }
    Some(rounded as i32)
}

/// Is this position on a monitor that is currently attached?
fn on_a_visible_screen(window: &tauri::WebviewWindow, x: i32, y: i32) -> bool {
    let Ok(monitors) = window.available_monitors() else {
        // If we cannot tell, trust the saved position: the common case is one
        // monitor that has not moved.
        return true;
    };
    if monitors.is_empty() {
        return true;
    }
    monitors.iter().any(|monitor| {
        let position = monitor.position();
        let size = monitor.size();
        let right = position.x + i32::try_from(size.width).unwrap_or(i32::MAX);
        let bottom = position.y + i32::try_from(size.height).unwrap_or(i32::MAX);
        // A little tolerance: a title bar half off the top is still reachable,
        // a window entirely off the right is not.
        x >= position.x - 50 && x < right - 50 && y >= position.y - 10 && y < bottom - 50
    })
}

/// The second copy of Magic Bill on one PC.
///
/// Two counters fight over the database, the licence and the numbering — and
/// **the second one focuses the first window rather than dying quietly**,
/// because "it won't open" is the bug report that follows a silent exit.
pub fn focus_existing(app: &tauri::AppHandle) {
    log_info!("a second copy was started — focusing the window that is already open");
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WindowState;

    /// **The minimised rect, refused.**
    ///
    /// Windows reports a minimised window as 144 x 19 at (-32000, -32000), and
    /// a resize event fires when it is minimised — so that got saved and the
    /// next start restored it. A shop that minimised Magic Bill and closed it
    /// reopened to a window the size of a tooltip.
    ///
    /// `remember` no longer writes it, and this is the second layer: a config
    /// that already carries it repairs itself rather than making somebody
    /// delete a file they have never heard of.
    #[test]
    fn a_minimised_rect_is_not_a_saved_size() {
        let bad = WindowState {
            width: Some(144),
            height: Some(19),
            x: Some(-32_000),
            y: Some(-32_000),
            maximised: false,
        };
        assert!(
            bad.width.is_some_and(|w| w < MIN_SENSIBLE_WIDTH),
            "144 wide must be refused"
        );
        assert!(
            bad.height.is_some_and(|h| h < MIN_SENSIBLE_HEIGHT),
            "19 tall must be refused"
        );
    }

    /// And a real one is believed — including the reference machine's, which is
    /// the smallest screen this product supports.
    #[test]
    fn a_real_size_is_believed() {
        for (width, height) in [(1366_u32, 768_u32), (1024, 720), (640, 480)] {
            assert!(
                width >= MIN_SENSIBLE_WIDTH && height >= MIN_SENSIBLE_HEIGHT,
                "{width}x{height} is a size a shop could really have chosen"
            );
        }
    }
}
