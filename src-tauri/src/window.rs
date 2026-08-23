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
//!
//! # The one rule this file exists to keep
//!
//! **The window always opens entirely on the screen.** Whatever is in the
//! config file, whatever monitor it was written on, whatever has been unplugged
//! since. Everything below is that sentence, made true.
//!
//! Owner's report, 2026-08-23: the app opened with its left edge, its right
//! edge, its title bar and **Complete bill** all off the screen. The config
//! said `1600 x 852 at (1358, -8)` — a window that filled a second monitor —
//! and the restore believed that size without ever asking whether it fitted.
//! The position check did catch it, and the answer it gave, "centre it", hung
//! an oversized window off *both* sides instead of one. See `fit`.

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

/// A rectangle in **physical** pixels — the units a monitor is measured in.
///
/// The config file stores logical pixels, because that is what survives a
/// change of display scaling. Every decision below is made in physical ones,
/// because "does this fit on that screen?" is a question about real pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

/// Put the window back where it was, then show it.
pub fn restore_and_show(window: &tauri::WebviewWindow, state: &WindowState) {
    place(window, state);
    if state.maximised {
        let _ = window.maximize();
    }
    let _ = window.show();
    let _ = window.set_focus();
}

/// Decide the window's rectangle and apply it.
///
/// **Nothing here writes the config file, and that is deliberate.** The saved
/// rectangle is the shop's preference; this only decides how to show it on the
/// screen that is actually plugged in today. A counter that normally runs on a
/// big second monitor, used for an afternoon on the laptop alone, fills the
/// laptop — and gets its big window back the moment the monitor returns,
/// because the size it chose was never overwritten. `remember` writes the file,
/// and only when a person moves or resizes the window themselves.
///
/// Best effort throughout: a window that opens at the wrong size is a smaller
/// problem than one that does not open, so every step is allowed to fail
/// quietly and leave the configured size in place.
fn place(window: &tauri::WebviewWindow, state: &WindowState) {
    let Some(work) = work_area(window, state) else {
        // No monitor to measure against, so nothing here can be decided
        // honestly. Leave the window exactly as `tauri.conf.json` asked.
        log_info!("no monitor could be measured — opening at the configured size");
        return;
    };
    let border = border(window);
    let wanted = wanted_rect(window, state, work);
    let fitted = fit(wanted, work);

    if fitted == wanted {
        log_info!(
            "window {}x{} at {},{}",
            fitted.width,
            fitted.height,
            fitted.x,
            fitted.y
        );
    } else {
        log_info!(
            "the saved window does not fit this screen — {}x{} at {},{} becomes {}x{} at {},{} (work area {}x{} at {},{})",
            wanted.width,
            wanted.height,
            wanted.x,
            wanted.y,
            fitted.width,
            fitted.height,
            fitted.x,
            fitted.y,
            work.width,
            work.height,
            work.x,
            work.y
        );
    }

    // `fitted` is the rectangle of the PAGE — the pixels a shopkeeper can read.
    // `set_size` speaks in those pixels too, so the size goes straight on; but
    // `set_position` places the outer rectangle, which on Windows includes the
    // invisible resize border, so the border comes off the position here.
    //
    // Measured, 2026-08-23, with `GetClientRect` on the running counter:
    // clamping the OUTER rectangle instead left a seven-pixel margin of unused
    // screen down the left, the right and the bottom. The border is invisible,
    // so it is *meant* to hang over the edge — which is what Windows itself
    // does when it maximises a window. With the border taken off, the page
    // lands at exactly 1366 × 720 at (0, 0) on the reference machine: the work
    // area, to the pixel, with nothing wasted and nothing hidden.
    let _ = window.set_size(PhysicalSize::new(fitted.width, fitted.height));
    let _ = window.set_position(PhysicalPosition::new(
        fitted.x.saturating_sub(border.0),
        fitted.y.saturating_sub(border.1),
    ));
}

/// **Make a rectangle fit a work area. This is the fix.**
///
/// Two independent things can be wrong with a saved rectangle, and the old code
/// looked for only one of them:
///
/// * it can be **bigger than the screen** — nothing checked this at all, and
///   that is the owner's bug;
/// * it can be **off the screen** — this was checked, but the answer was to
///   centre it, and centring something too big hangs it off both edges instead
///   of one.
///
/// Shrink first, then move. Moving alone cannot save an oversized window, and
/// shrinking alone cannot save a well-sized one that is in the wrong place. The
/// result is always wholly inside `work`.
fn fit(wanted: Rect, work: Rect) -> Rect {
    let width = wanted.width.min(work.width);
    let height = wanted.height.min(work.height);
    // The room left over once the window is in. Never negative: the size was
    // clamped to the work area on the two lines above.
    let slack_x = signed(work.width.saturating_sub(width));
    let slack_y = signed(work.height.saturating_sub(height));
    Rect {
        x: wanted.x.clamp(work.x, work.x.saturating_add(slack_x)),
        y: wanted.y.clamp(work.y, work.y.saturating_add(slack_y)),
        width,
        height,
    }
}

/// The rectangle the config file is asking for, in physical pixels.
///
/// A missing or absurd size means **fill the work area**, which is what a first
/// run does (P30.5, D156). The configuration asks for 1366 × 768 centred, which
/// is exactly the screen a counter PC has, so Windows put the bottom forty-eight
/// pixels — **Complete bill** — under the taskbar. `maximize()` is not the fix
/// and was tried first: an undecorated window maximises over the taskbar. The
/// work area is the number Windows keeps for precisely this question.
///
/// A missing position means centre it in that work area.
fn wanted_rect(window: &tauri::WebviewWindow, state: &WindowState, work: Rect) -> Rect {
    let scale = window.scale_factor().unwrap_or(1.0);

    // **A saved size that could not hold a counter is not a saved size.**
    //
    // The minimised-rect fix: `remember` no longer writes 144 × 19, but every
    // machine that has already run an older build has that in its config file,
    // and a shop should not have to know why its window opens the size of a
    // tooltip. An absurd size is ignored and the work area used instead, so the
    // file repairs itself on the next resize.
    let saved = match (state.width, state.height) {
        (Some(width), Some(height))
            if width >= MIN_SENSIBLE_WIDTH && height >= MIN_SENSIBLE_HEIGHT =>
        {
            // The config holds the size a person could see, because that is
            // what `remember` reads back, so it needs no adjusting — only
            // turning from logical pixels into real ones.
            let seen = LogicalSize::<f64>::new(f64::from(width), f64::from(height))
                .to_physical::<u32>(scale);
            Some((seen.width, seen.height))
        }
        _ => None,
    };
    let (width, height) = saved.unwrap_or((work.width, work.height));

    let (x, y) = match (state.x, state.y) {
        (Some(x), Some(y)) => {
            let point =
                LogicalPosition::<f64>::new(f64::from(x), f64::from(y)).to_physical::<i32>(scale);
            (point.x, point.y)
        }
        // Centred in what is usable, not in the whole monitor: a window centred
        // on the monitor sits half a taskbar too low.
        _ => (
            work.x
                .saturating_add(signed(work.width.saturating_sub(width)).div_euclid(2)),
            work.y
                .saturating_add(signed(work.height.saturating_sub(height)).div_euclid(2)),
        ),
    };

    Rect {
        x,
        y,
        width,
        height,
    }
}

/// How far the page inside the window sits from the window's own top-left
/// corner, in physical pixels: `(left, top)`.
///
/// Nothing at all on a platform with no frame. On Windows an undecorated window
/// still gets an invisible resize border — seven pixels on the reference
/// machine — and this is how wide it is. Measured rather than assumed, because
/// it changes with the display's scaling and with the Windows version.
fn border(window: &tauri::WebviewWindow) -> (i32, i32) {
    let (Ok(outer), Ok(inner)) = (window.outer_position(), window.inner_position()) else {
        return (0, 0);
    };
    (
        inner.x.saturating_sub(outer.x),
        inner.y.saturating_sub(outer.y),
    )
}

/// The usable part of the screen this window belongs on.
///
/// **The monitor holding the saved position, if it is still attached** — a shop
/// that keeps the counter on a second screen must reopen there. If that monitor
/// is gone, or there is no saved position, it is the monitor the window is on
/// now. A shop that unplugs the second monitor must not get a window at
/// x = 3000, and must not get one measured against a screen that is not there.
fn work_area(window: &tauri::WebviewWindow, state: &WindowState) -> Option<Rect> {
    let saved = match (state.x, state.y) {
        (Some(x), Some(y)) => {
            let scale = window.scale_factor().unwrap_or(1.0);
            let point =
                LogicalPosition::<f64>::new(f64::from(x), f64::from(y)).to_physical::<i32>(scale);
            window
                .available_monitors()
                .ok()
                .and_then(|monitors| monitors.into_iter().find(|monitor| holds(monitor, point)))
        }
        _ => None,
    };
    let monitor = match saved {
        Some(monitor) => monitor,
        None => window.current_monitor().ok().flatten()?,
    };
    let area = monitor.work_area();
    if area.size.width == 0 || area.size.height == 0 {
        return None;
    }
    Some(Rect {
        x: area.position.x,
        y: area.position.y,
        width: area.size.width,
        height: area.size.height,
    })
}

/// Is this point on that monitor?
///
/// The whole monitor, not its work area: a window remembered with its title bar
/// tucked under the taskbar is still on that screen, and moving it to a
/// different one would be the wrong answer to a smaller question.
fn holds(monitor: &tauri::Monitor, point: PhysicalPosition<i32>) -> bool {
    let position = monitor.position();
    let size = monitor.size();
    let right = position
        .x
        .saturating_add(i32::try_from(size.width).unwrap_or(i32::MAX));
    let bottom = position
        .y
        .saturating_add(i32::try_from(size.height).unwrap_or(i32::MAX));
    point.x >= position.x && point.x < right && point.y >= position.y && point.y < bottom
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
    if window.is_minimized().unwrap_or(false) {
        return;
    }
    let scale = window.scale_factor().unwrap_or(1.0);
    if let Ok(PhysicalSize { width, height }) = window.inner_size() {
        let size = LogicalSize::<f64>::from_physical(PhysicalSize::new(width, height), scale);
        config.window.width = whole_u32(size.width);
        config.window.height = whole_u32(size.height);
    }
    // **The position of what a person can SEE, to match the size above.**
    //
    // This read `outer_position`, which on Windows is seven pixels left of and
    // above the visible corner because of the invisible resize border. The size
    // beside it has always been the inner one. That mismatch was harmless while
    // `restore_and_show` put the saved position straight back — the same seven
    // pixels came off both ends — but `place` now positions by the visible
    // corner, and a window saved by its outer one would walk seven pixels to
    // the left on every restart. Both halves of the rectangle mean the same
    // thing now.
    if let Ok(PhysicalPosition { x, y }) = window.inner_position() {
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
/// `None` simply means the size is not remembered — the next start fills the
/// work area, which is the same thing a first run does.
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

/// A pixel count as a signed one, saturating rather than wrapping.
///
/// Every caller has already clamped the value to a monitor's size, so the
/// saturation is unreachable. It is here so that no arithmetic in this file can
/// wrap a screen coordinate.
fn signed(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
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

    /// The reference machine: a 1366 × 768 counter PC with a taskbar down the
    /// bottom, so 720 of the 768 are usable.
    const COUNTER: Rect = Rect {
        x: 0,
        y: 0,
        width: 1366,
        height: 720,
    };

    /// Every pixel of `rect` is inside `work`.
    fn inside(rect: Rect, work: Rect) -> bool {
        let right = i64::from(rect.x) + i64::from(rect.width);
        let bottom = i64::from(rect.y) + i64::from(rect.height);
        rect.x >= work.x
            && rect.y >= work.y
            && right <= i64::from(work.x) + i64::from(work.width)
            && bottom <= i64::from(work.y) + i64::from(work.height)
    }

    /// **THE OWNER'S BUG, 2026-08-23.**
    ///
    /// `1600 x 852 at (1358, -8)` — a window that filled a second monitor —
    /// restored onto the 1366 × 768 laptop after that monitor was unplugged.
    /// The old code believed the size, found the position off-screen, and
    /// centred it: 117 pixels off the left, 117 off the right, 42 off the top
    /// and 42 off the bottom. The search box, the left rail, the title bar and
    /// **Complete bill** were all outside the screen.
    ///
    /// This test fails on that code and passes on this.
    #[test]
    fn a_window_from_a_bigger_monitor_comes_back_onto_this_one() {
        let from_the_big_screen = Rect {
            x: 1358,
            y: -8,
            width: 1600,
            height: 852,
        };
        let fitted = fit(from_the_big_screen, COUNTER);
        assert_eq!(
            fitted,
            Rect {
                x: 0,
                y: 0,
                width: 1366,
                height: 720
            },
            "it has to shrink to the work area, not merely move"
        );
        assert!(inside(fitted, COUNTER));
    }

    /// The half of the bug the old code did not look for **at all** — the size.
    /// Centring is not enough, and this is the assertion that says so.
    #[test]
    fn a_window_wider_than_the_screen_is_made_narrower() {
        let too_wide = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 600,
        };
        let fitted = fit(too_wide, COUNTER);
        assert_eq!(fitted.width, COUNTER.width, "1920 does not fit in 1366");
        assert_eq!(fitted.height, 600, "the height was fine — leave it alone");
        assert!(inside(fitted, COUNTER));
    }

    /// The half it did look for, still working: a window that fits but sits off
    /// the edge is moved back and **keeps its size**. The old answer, centring,
    /// threw away a position the shop had chosen for no reason.
    #[test]
    fn a_window_off_the_edge_is_moved_back_at_its_own_size() {
        let off_the_right = Rect {
            x: 1300,
            y: 400,
            width: 900,
            height: 500,
        };
        let fitted = fit(off_the_right, COUNTER);
        assert_eq!(fitted.width, 900, "the shop chose this width");
        assert_eq!(fitted.height, 500);
        assert_eq!(fitted.x, 1366 - 900, "pushed left until it fits, no further");
        assert_eq!(fitted.y, 720 - 500);
        assert!(inside(fitted, COUNTER));
    }

    /// A window that is already fine is not touched. A fix that moves windows
    /// which were never wrong is its own bug report.
    #[test]
    fn a_window_that_already_fits_is_left_exactly_alone() {
        let fine = Rect {
            x: 100,
            y: 50,
            width: 1000,
            height: 600,
        };
        assert_eq!(fit(fine, COUNTER), fine);
    }

    /// The taskbar. A window the full height of the *monitor* is still too tall
    /// for the *work area*, and the forty-eight pixels it loses are the ones
    /// **Complete bill** lives in (P30.5, D156).
    #[test]
    fn the_taskbar_is_not_screen_the_window_may_use() {
        let whole_monitor = Rect {
            x: 0,
            y: 0,
            width: 1366,
            height: 768,
        };
        let fitted = fit(whole_monitor, COUNTER);
        assert_eq!(fitted.height, 720, "the taskbar's forty-eight are not ours");
        assert!(inside(fitted, COUNTER));
    }

    /// A second monitor sitting to the right has a work area that does not
    /// start at zero, and the clamp has to be against *that* origin — otherwise
    /// restoring onto it drags every window back to the left-hand screen.
    #[test]
    fn a_monitor_that_does_not_start_at_zero_is_clamped_to_its_own_origin() {
        let second = Rect {
            x: 1366,
            y: 0,
            width: 1600,
            height: 852,
        };
        let wandered = Rect {
            x: 1366 + 1500,
            y: 0,
            width: 900,
            height: 500,
        };
        let fitted = fit(wandered, second);
        assert_eq!(fitted.x, 1366 + 1600 - 900);
        assert!(inside(fitted, second));
    }

    /// **The minimised rect, refused.**
    ///
    /// Windows reports a minimised window as 144 × 19 at (−32000, −32000), and
    /// a resize event fires when it is minimised — so that got saved and the
    /// next start restored it. A shop that minimised Magic Bill and closed it
    /// reopened to a window the size of a tooltip.
    #[test]
    fn a_minimised_rect_is_not_a_saved_size() {
        let minimised = WindowState {
            width: Some(144),
            height: Some(19),
            x: Some(-32_000),
            y: Some(-32_000),
            maximised: false,
        };
        assert!(
            minimised.width.is_some_and(|w| w < MIN_SENSIBLE_WIDTH),
            "144 wide must be refused"
        );
        assert!(
            minimised.height.is_some_and(|h| h < MIN_SENSIBLE_HEIGHT),
            "19 tall must be refused"
        );
        // And what it becomes instead: the work area, wholly on the screen.
        assert!(inside(fit(COUNTER, COUNTER), COUNTER));
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

    /// **The page fills the work area exactly.**
    ///
    /// The number this whole file is for, and the one that was measured on the
    /// running counter with `GetClientRect`: a first run, or a saved rectangle
    /// too big for the screen, gives a page of 1366 × 720 at (0, 0) — the work
    /// area to the pixel. Not 1352 × 713 at (7, 0), which is what clamping the
    /// window's outer rectangle produced, and which threw away fourteen columns
    /// and seven rows of a screen this product has none to spare on.
    #[test]
    fn a_window_too_big_for_the_screen_fills_it_exactly() {
        let too_big = Rect {
            x: 1358,
            y: -8,
            width: 1600,
            height: 852,
        };
        let fitted = fit(too_big, COUNTER);
        assert_eq!((fitted.width, fitted.height), (1366, 720));
        assert_eq!((fitted.x, fitted.y), (0, 0));
    }

    /// Whatever goes in, the window lands on the screen. The single rule this
    /// file exists to keep, asserted over the shapes that have actually turned
    /// up in a config file — including the two that caused bug reports.
    #[test]
    fn nothing_ever_lands_off_the_screen() {
        let seen = [
            Rect {
                x: 1358,
                y: -8,
                width: 1600,
                height: 852,
            },
            Rect {
                x: -32_000,
                y: -32_000,
                width: 144,
                height: 19,
            },
            Rect {
                x: 0,
                y: 0,
                width: 3840,
                height: 2160,
            },
            Rect {
                x: i32::MAX,
                y: i32::MAX,
                width: 1,
                height: 1,
            },
            Rect {
                x: i32::MIN,
                y: i32::MIN,
                width: u32::MAX,
                height: u32::MAX,
            },
        ];
        for rect in seen {
            assert!(inside(fit(rect, COUNTER), COUNTER), "{rect:?} escaped");
        }
    }
}
