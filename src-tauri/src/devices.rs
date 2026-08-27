//! The things a counter is plugged into.

use std::io::Read as _;

use serde::Serialize;
use ts_rs::TS;

use mb_auth::Permission;
use mb_core::devices::{Reading, ScaleProtocol, read_scale};

use crate::guard;
use crate::state::App;
use crate::words::{UiError, UiResult};

/// One thing that may or may not be plugged in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    /// `printer`, `scanner`, `scale`, `display`, `label`, `payment`.
    pub kind: String,
    pub name: String,
    /// What it is for, in one line.
    pub what: String,
    /// Whether this shop has set it up at all.
    pub set_up: bool,
    /// The honest sentence: "Not set up", "COM3 at 9600 baud", "Nothing answered on COM3".
    pub says: String,
    /// Whether a Test button does anything for this device.
    pub testable: bool,
}

/// What came back from a test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DeviceTest {
    /// True when the device answered.
    pub answered: bool,
    /// A sentence for the person standing at the counter.
    pub says: String,
    /// The raw bytes, exactly as they arrived.
    pub raw: String,
}

/// The device screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct DevicesView {
    pub devices: Vec<DeviceView>,
    /// The honest headline: what this build has and has not been tried against.
    pub says: String,
}

// The screen.

pub fn devices_on(app: &App) -> UiResult<DevicesView> {
    guard::require(app, Permission::SettingsPrinter)?;
    let config = app.shop_config();
    let d = &config.devices;

    let printers = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    mb_db::Repos::new(tx)
                        .settings()
                        .list_printers(crate::state::OUTLET)
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .unwrap_or_default();

    let mut devices = vec![
        DeviceView {
            kind: "printer".to_owned(),
            name: "Printers".to_owned(),
            what: "Bills, kitchen tickets and the closing slip.".to_owned(),
            set_up: !printers.is_empty(),
            says: if printers.is_empty() {
                "None set up yet".to_owned()
            } else {
                format!("{} set up", printers.len())
            },
            testable: !printers.is_empty(),
        },
        DeviceView {
            kind: "scanner".to_owned(),
            name: "Barcode scanner".to_owned(),
            what: "Types a code into the search box, like a keyboard.".to_owned(),
            // Always set up, because it needs nothing: a scanner is a keyboard and there is no
            // port to get wrong.
            set_up: true,
            says: format!(
                "Nothing to plug in. A scan is {} characters or more, arriving \
                 under {} ms apart.",
                d.scan_min_length, d.scan_average_gap_ms
            ),
            testable: true,
        },
        DeviceView {
            kind: "scale".to_owned(),
            name: "Weighing scale".to_owned(),
            what: "Weighs into a line's quantity.".to_owned(),
            set_up: !d.scale_port.trim().is_empty(),
            says: if d.scale_port.trim().is_empty() {
                "Not set up. Most shops have no scale.".to_owned()
            } else {
                format!("{} at {} baud", d.scale_port.trim(), d.scale_baud)
            },
            testable: !d.scale_port.trim().is_empty(),
        },
        DeviceView {
            kind: "display".to_owned(),
            name: "Customer display".to_owned(),
            what: "Shows the customer their bill as it is typed.".to_owned(),
            set_up: d.display_on,
            says: if !d.display_on {
                "Off".to_owned()
            } else if d.display_port.trim().is_empty() {
                "A second window, for a second monitor".to_owned()
            } else {
                format!("Pole display on {}", d.display_port.trim())
            },
            testable: d.display_on,
        },
        DeviceView {
            kind: "label".to_owned(),
            name: "Label printer".to_owned(),
            what: "Parcel labels: what it is, how many, and for which table.".to_owned(),
            set_up: !d.label_printer.trim().is_empty(),
            says: if d.label_printer.trim().is_empty() {
                "Not set up".to_owned()
            } else {
                d.label_printer.trim().to_owned()
            },
            testable: !d.label_printer.trim().is_empty(),
        },
        DeviceView {
            kind: "payment".to_owned(),
            name: "Payment machine".to_owned(),
            what: "Says whether money against a reference actually arrived.".to_owned(),
            set_up: true,
            says: app.provider().name().to_owned(),
            testable: false,
        },
    ];
    devices.sort_by_key(|row| !row.set_up);

    Ok(DevicesView {
        devices,
        says: "Every one of these is optional. A device that is missing, \
               unplugged or slow can never stop a bill — the counter says so \
               and carries on."
            .to_owned(),
    })
}

// The scale.

/// How long to listen to a scale before giving up.
const SCALE_DEADLINE_MS: u64 = 500;

/// Read the scale once.
pub fn read_scale_on(app: &App) -> UiResult<DeviceTest> {
    guard::require(app, Permission::BillCreate)?;
    let config = app.shop_config();
    let d = &config.devices;

    if d.scale_port.trim().is_empty() {
        return Ok(DeviceTest {
            answered: false,
            says: "No scale is set up. Type the quantity instead.".to_owned(),
            raw: String::new(),
        });
    }

    let raw = match listen(d.scale_port.trim(), d.scale_baud) {
        Ok(text) => text,
        Err(says) => {
            return Ok(DeviceTest {
                answered: false,
                says,
                raw: String::new(),
            });
        }
    };
    if raw.trim().is_empty() {
        return Ok(DeviceTest {
            answered: false,
            says: format!(
                "Nothing came back from {}. Check it is switched on and on the \
                 right port.",
                d.scale_port.trim()
            ),
            raw,
        });
    }

    let protocol = d.protocol();
    if protocol == ScaleProtocol::Raw {
        // Raw decides nothing, and that is its job.
        return Ok(DeviceTest {
            answered: true,
            says: "This is exactly what the scale is sending. Pick the shape \
                   that matches it in Settings."
                .to_owned(),
            raw,
        });
    }

    // The LAST complete line: a scale sends continuously, so the newest reading is the one the
    // person is looking at.
    let line = raw.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("");
    match read_scale(line, protocol) {
        Ok(reading) => Ok(DeviceTest {
            answered: true,
            says: words_for(&reading),
            raw,
        }),
        Err(e) => Ok(DeviceTest {
            answered: false,
            says: format!(
                "That is not a shape this counter understands — {e}. Try \
                 \"Show me what it is sending\" in Settings."
            ),
            raw,
        }),
    }
}

fn words_for(reading: &Reading) -> String {
    if !reading.stable {
        // A bouncing weight is never taken.
        return "Still settling — wait for the scale to hold still.".to_owned();
    }
    format!("{} {}", reading.qty, reading.unit)
}

/// Listen to a serial port for a moment.
#[cfg(windows)]
fn listen(port: &str, baud: u32) -> Result<String, String> {
    let mut open = mb_winprint::open_serial_duplex(port, baud)
        .map_err(|e| format!("{port} could not be opened — {e}. Another program may have it."))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(SCALE_DEADLINE_MS);
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 256];
    while std::time::Instant::now() < deadline {
        match open.read(&mut chunk) {
            Ok(0) => {}
            Ok(n) => buffer.extend_from_slice(chunk.get(..n).unwrap_or(&[])),
            Err(e) => return Err(format!("{port} stopped answering — {e}.")),
        }
        if buffer.len() > 4_096 {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

#[cfg(not(windows))]
fn listen(port: &str, _baud: u32) -> Result<String, String> {
    Err(format!(
        "this build cannot open {port} — serial ports are Windows only here"
    ))
}

// The scanner.

/// What a burst of characters turned out to be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ScanOutcome {
    /// `typing`, `item`, `weighed`, `bill` or `unknown`.
    pub what: String,
    /// The item to add, when there is one.
    pub item_id: String,
    pub item_name: String,
    /// The quantity to add it at — "1", or "0.45" off a weight label.
    pub qty: String,
    /// The order to bring back, when a printed bill was scanned.
    pub order_id: String,
    /// The code itself, so an unknown one can be offered to an item.
    pub code: String,
    /// What to say, when there is anything to say.
    pub says: String,
}

fn typing() -> ScanOutcome {
    ScanOutcome {
        what: "typing".to_owned(),
        item_id: String::new(),
        item_name: String::new(),
        qty: String::new(),
        order_id: String::new(),
        code: String::new(),
        says: String::new(),
    }
}

/// Scan, or person?.
pub fn scanned_on(app: &App, text: String, gaps_ms: Vec<u32>) -> UiResult<ScanOutcome> {
    guard::require(app, Permission::BillCreate)?;
    let config = app.shop_config();
    let keys = mb_core::devices::Keystrokes {
        text: text.trim().to_owned(),
        gaps_ms,
    };
    if mb_core::devices::how_it_arrived(&keys, config.devices.scan_rule())
        == mb_core::devices::Typed::Person
    {
        return Ok(typing());
    }
    let code = keys.text;

    // A weight-encoded label, if this shop's scale prints them.
    if let Some(rule) = config.devices.label_rule()
        && let Ok(label) = mb_core::devices::read_label(&code, &rule)
    {
        let found = find_item(app, &label.item_code)?;
        return Ok(match (found, label.embedded) {
            (Some((id, name)), mb_core::devices::Embedded::Quantity(qty)) => ScanOutcome {
                what: "weighed".to_owned(),
                item_id: id,
                item_name: name.clone(),
                qty: qty.to_string(),
                order_id: String::new(),
                code,
                says: format!("{name}, {qty}"),
            },
            // A price label bills at the SHOP's price, not the label's.
            (Some((id, name)), mb_core::devices::Embedded::Price(money)) => ScanOutcome {
                what: "item".to_owned(),
                item_id: id,
                item_name: name.clone(),
                qty: "1".to_owned(),
                order_id: String::new(),
                code,
                says: format!("{name} — the label says {money}"),
            },
            (None, _) => ScanOutcome {
                what: "unknown".to_owned(),
                says: format!(
                    "That is one of your scale's labels, but no item has the                      code {}.",
                    label.item_code
                ),
                code,
                ..typing()
            },
        });
    }

    // An item's own code.
    if let Some((id, name)) = find_item(app, &code)? {
        return Ok(ScanOutcome {
            what: "item".to_owned(),
            item_id: id,
            item_name: name.clone(),
            qty: "1".to_owned(),
            order_id: String::new(),
            code,
            says: name,
        });
    }

    // A printed bill. The one thing a scanner does that nothing else here can: point it at a
    // bill and get the bill back.
    if let Some(order_id) = find_bill(app, &code)? {
        return Ok(ScanOutcome {
            what: "bill".to_owned(),
            order_id,
            says: format!("Bill {code}"),
            code,
            ..typing()
        });
    }

    Ok(ScanOutcome {
        what: "unknown".to_owned(),
        says: format!("Nothing on this counter has the code {code}."),
        code,
        ..typing()
    })
}

fn find_item(app: &App, code: &str) -> UiResult<Option<(String, String)>> {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx)
                    .menu()
                    .find_by_code(crate::state::OUTLET, code)
            })
            .map_err(|e| crate::words::from_db(&e))
    })
}

fn find_bill(app: &App, number: &str) -> UiResult<Option<String>> {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx)
                    .corrections()
                    .order_by_bill_number(crate::state::OUTLET, number)
            })
            .map_err(|e| crate::words::from_db(&e))
    })
}

// The customer display.

/// The second window must NEVER take focus.
const DISPLAY_TAKES_FOCUS: bool = false;

/// The label of the customer-facing window.
pub const DISPLAY_WINDOW: &str = "display";

/// Open (or close) the customer display to match the shop's settings.
pub fn sync_display(app: &tauri::AppHandle, on: bool) {
    use tauri::Manager as _;

    let existing = app.get_webview_window(DISPLAY_WINDOW);
    match (on, existing) {
        (false, Some(window)) => {
            let _ = window.close();
        }
        (true, Some(_)) | (false, None) => {}
        (true, None) => {
            let built = tauri::WebviewWindowBuilder::new(
                app,
                DISPLAY_WINDOW,
                tauri::WebviewUrl::App("index.html?display=1".into()),
            )
            .title("Magic Bill — your bill")
            .inner_size(800.0, 600.0)
            .focused(DISPLAY_TAKES_FOCUS)
            .skip_taskbar(true)
            .build();
            if let Err(e) = built {
                crate::log_warn!(
                    "the customer display could not be opened ({e}); billing is unaffected"
                );
            }
        }
    }
}

/// Show the customer the bill as it is typed.
pub fn show_bill(app: &tauri::AppHandle, cart: &crate::billing::CartView) {
    use tauri::{Emitter as _, Manager as _};

    let Some(state) = app.try_state::<App>() else {
        return;
    };
    let config = state.shop_config();
    if !config.devices.display_on {
        return;
    }

    let title = if cart.is_empty && !config.devices.display_idle.trim().is_empty() {
        config.devices.display_idle.trim().to_owned()
    } else {
        config.store.name.clone()
    };
    let lines: Vec<crate::state::DisplayLine> = cart
        .lines
        .iter()
        .map(|line| crate::state::DisplayLine {
            name: line.name.clone(),
            qty: line.qty.clone(),
            amount: line.amount.text.clone(),
        })
        .collect();

    let message = crate::state::Pushed::CustomerBill {
        lines,
        total: cart.bill.grand_total.text.clone(),
        title,
        qr: String::new(),
        idle: cart.is_empty,
    };
    if let Err(e) = app.emit(crate::push::CHANNEL, message) {
        crate::log_warn!("the customer display could not be told ({e}); billing is unaffected");
    }

    // A pole display is two lines of characters on a serial port.
    let port = config.devices.display_port.trim().to_owned();
    if !port.is_empty() {
        let top = if cart.is_empty {
            config.store.name.clone()
        } else {
            cart.lines
                .last()
                .map(|l| l.name.clone())
                .unwrap_or_default()
        };
        pole_write(&port, &top, &cart.bill.grand_total.text);
    }
}

/// Two lines to a pole display, or a log line.
#[cfg(windows)]
fn pole_write(port: &str, top: &str, bottom: &str) {
    use std::io::Write as _;

    // 20 characters is what every VFD pole display on the market shows.
    let line = |text: &str| -> String { text.chars().take(20).collect() };
    // Form feed clears a VFD; the rest is plain text, which every one of them understands
    // without a driver.
    let payload = format!("{:<20}{:>20}", line(top), line(bottom));
    match mb_winprint::open_serial(port, 9_600) {
        Ok(mut open) => {
            if let Err(e) = open.write_all(payload.as_bytes()) {
                crate::log_warn!("the pole display on {port} did not take the line ({e})");
            }
        }
        Err(e) => crate::log_warn!("the pole display on {port} could not be opened ({e})"),
    }
}

#[cfg(not(windows))]
fn pole_write(port: &str, _top: &str, _bottom: &str) {
    crate::log_warn!("this build cannot write to the pole display on {port}");
}

// The commands.

#[tauri::command]
pub fn device_manager(app: tauri::State<'_, App>) -> UiResult<DevicesView> {
    devices_on(&app)
}

/// Scan, or person? The screen sends what arrived and when; Rust decides.
#[tauri::command]
pub fn scanned(
    app: tauri::State<'_, App>,
    text: String,
    gaps_ms: Vec<u32>,
) -> UiResult<ScanOutcome> {
    scanned_on(&app, text, gaps_ms)
}

#[tauri::command]
pub fn read_scale_once(app: tauri::State<'_, App>) -> UiResult<DeviceTest> {
    read_scale_on(&app)
}

/// Turn the customer display on or off from the device screen, without going to Settings —
/// because the person testing it is standing in front of it.
#[tauri::command]
pub fn show_customer_display(
    app: tauri::State<'_, App>,
    handle: tauri::AppHandle,
    on: bool,
) -> UiResult<DevicesView> {
    guard::require(&app, Permission::SettingsPrinter)?;
    let mut config = app.shop_config();
    config.devices.display_on = on;
    app.publish_shop_config(config);
    sync_display(&handle, on);
    devices_on(&app)
}

/// Print a parcel label.
#[tauri::command]
pub fn print_label(app: tauri::State<'_, App>, line: String, token: String) -> UiResult<String> {
    print_label_on(&app, line, token)
}

pub fn print_label_on(app: &App, line: String, token: String) -> UiResult<String> {
    guard::require(app, Permission::BillCreate)?;
    let config = app.shop_config();
    let wanted = config.devices.label_printer.trim().to_owned();
    if wanted.is_empty() {
        return Err(UiError::new(
            "label.none",
            "No label printer is set up. Choose one in Settings, under Devices.",
        ));
    }
    let printer = crate::flows::printer_by_id(app, &wanted)?;
    let document = mb_print::template::label_document(
        printer.paper,
        &mb_print::template::LabelContext {
            shop: &config.store.name,
            token: &token,
            line: &line,
            of: None,
        },
    );
    app.print(
        mb_print::queue::Job::new(
            mb_print::queue::JobKind::Label,
            &printer.id,
            document,
            crate::flows::today(crate::flows::now()),
        )
        .because(format!("label for {token}")),
    )
}
