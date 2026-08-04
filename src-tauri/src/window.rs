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

/// Put the window back where it was, then show it.
pub fn restore_and_show(window: &tauri::WebviewWindow, state: &WindowState) {
    if let (Some(width), Some(height)) = (state.width, state.height) {
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
