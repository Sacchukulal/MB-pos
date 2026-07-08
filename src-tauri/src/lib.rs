// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: String) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
async fn get_printers() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(|| {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            use std::process::Command;

            let output = Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    "Get-Printer | Select-Object -ExpandProperty Name",
                ])
                .creation_flags(0x08000000)
                .output();

            if let Ok(output) = output {
                let names = String::from_utf8_lossy(&output.stdout);
                return names
                    .lines()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            use std::process::Command;
            let output = Command::new("lpstat").arg("-a").output();

            if let Ok(output) = output {
                let names = String::from_utf8_lossy(&output.stdout);
                return names
                    .lines()
                    .filter_map(|line| line.split_whitespace().next().map(|s| s.to_string()))
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }

        vec![]
    })
    .await
    .unwrap_or_else(|_| vec![])
}

#[tauri::command]
async fn print_receipt_raw(printer_name: String, data: Vec<u8>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "windows")]
        {
            use std::ffi::CString;
            use std::ptr::null_mut;
            use winapi::shared::minwindef::{DWORD, FALSE, LPVOID};
            use winapi::um::errhandlingapi::GetLastError;
            use winapi::um::winspool::{
                ClosePrinter, EndDocPrinter, EndPagePrinter, OpenPrinterA, StartDocPrinterA,
                StartPagePrinter, WritePrinter, DOC_INFO_1A,
            };

            let p_name = CString::new(printer_name.clone())
                .map_err(|_| "Invalid printer name".to_string())?;
            let mut h_printer = null_mut();

            unsafe {
                if OpenPrinterA(p_name.as_ptr() as *mut i8, &mut h_printer, null_mut()) == FALSE {
                    let code = GetLastError();
                    let hint = match code {
                        1801 => " (printer name not found — re-select it in Printer Settings)",
                        5 => " (access denied)",
                        1722 | 1726 => " (print spooler/RPC unavailable — restart Print Spooler service)",
                        _ => "",
                    };
                    return Err(format!(
                        "Cannot connect to printer '{}': Win32 error {}{}",
                        printer_name, code, hint
                    ));
                }

                let doc_name = CString::new("EasyBill Receipt").unwrap();
                let datatype = CString::new("RAW").unwrap();

                let mut doc_info = DOC_INFO_1A {
                    pDocName: doc_name.as_ptr() as *mut i8,
                    pOutputFile: null_mut(),
                    pDatatype: datatype.as_ptr() as *mut i8,
                };

                let doc = StartDocPrinterA(h_printer, 1, &mut doc_info as *mut _ as *mut u8);
                if doc == 0 {
                    ClosePrinter(h_printer);
                    return Err("Failed to start document".to_string());
                }

                if StartPagePrinter(h_printer) == 0 {
                    EndDocPrinter(h_printer);
                    ClosePrinter(h_printer);
                    return Err("Failed to start page".to_string());
                }

                let mut bytes_written: DWORD = 0;
                let success = WritePrinter(
                    h_printer,
                    data.as_ptr() as LPVOID,
                    data.len() as DWORD,
                    &mut bytes_written,
                );

                EndPagePrinter(h_printer);
                EndDocPrinter(h_printer);
                ClosePrinter(h_printer);

                if success == FALSE || bytes_written as usize != data.len() {
                    let code = GetLastError();
                    return Err(format!(
                        "Connected, but failed to send data to '{}' (Win32 error {}, wrote {}/{} bytes)",
                        printer_name,
                        code,
                        bytes_written,
                        data.len()
                    ));
                }

                return Ok("Printed successfully".to_string());
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            use std::io::Write;
            use std::process::{Command, Stdio};

            let mut child = Command::new("lp")
                .arg("-o")
                .arg("raw")
                .arg("-d")
                .arg(printer_name)
                .stdin(Stdio::piped())
                .spawn()
                .map_err(|e| format!("Failed to spawn lp command: {}", e))?;

            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(&data)
                    .map_err(|e| format!("Failed to write to lp: {}", e))?;
            }

            let status = child
                .wait()
                .map_err(|e| format!("Failed to wait for lp: {}", e))?;

            if status.success() {
                Ok("Printed successfully".to_string())
            } else {
                Err("lp command failed".to_string())
            }
        }
    })
    .await
    .unwrap_or_else(|e| Err(format!("Task panicked: {}", e)))
}

#[tauri::command]
async fn print_receipt_text(printer_name: String, text: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            use std::process::Command;

            let mut temp_file = std::env::temp_dir();
            temp_file.push("easy_bill_receipt.txt");
            if let Err(e) = std::fs::write(&temp_file, &text) {
                return Err(format!("Failed to write temp file: {}", e));
            }

            let output = Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    &format!(
                        "Get-Content -Path '{}' | Out-Printer -Name '{}'",
                        temp_file.display(),
                        printer_name
                    ),
                ])
                .creation_flags(0x08000000)
                .output();

            match output {
                Ok(out) => {
                    if out.status.success() {
                        Ok("Printed successfully".to_string())
                    } else {
                        Err(String::from_utf8_lossy(&out.stderr).to_string())
                    }
                }
                Err(e) => Err(format!("Failed to execute powershell: {}", e)),
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            use std::io::Write;
            use std::process::{Command, Stdio};

            let mut child = Command::new("lp")
                .arg("-d")
                .arg(printer_name)
                .stdin(Stdio::piped())
                .spawn()
                .map_err(|e| format!("Failed to spawn lp command: {}", e))?;

            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(text.as_bytes())
                    .map_err(|e| format!("Failed to write to lp: {}", e))?;
            }

            let status = child
                .wait()
                .map_err(|e| format!("Failed to wait for lp: {}", e))?;

            if status.success() {
                Ok("Printed successfully".to_string())
            } else {
                Err("lp command failed".to_string())
            }
        }
    })
    .await
    .unwrap_or_else(|e| Err(format!("Task panicked: {}", e)))
}

#[derive(serde::Serialize)]
struct DeviceInfo {
    id: String,
    name: String,
}

/// Reject well-known blank / placeholder SMBIOS UUIDs that some OEM and
/// whitebox machines report instead of a real per-machine value.
#[cfg(target_os = "windows")]
fn is_valid_hw_id(id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    let upper = id.to_uppercase();
    let bad = [
        "00000000-0000-0000-0000-000000000000",
        "FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF",
        "03000200-0400-0500-0006-000700080009",
    ];
    !bad.contains(&upper.as_str())
}

/// Returns a stable hardware-bound device identifier used for one-desktop
/// licensing. Primary source is the motherboard SMBIOS UUID (survives an OS
/// reinstall), falling back to the first physical adapter MAC address.
#[tauri::command]
async fn get_device_id() -> DeviceInfo {
    tauri::async_runtime::spawn_blocking(|| {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            use std::process::Command;

            let name =
                std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Unknown PC".to_string());

            // Primary: SMBIOS hardware UUID (firmware-bound, survives OS reinstall).
            let uuid = Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    "(Get-CimInstance -ClassName Win32_ComputerSystemProduct).UUID",
                ])
                .creation_flags(0x08000000)
                .output();

            if let Ok(output) = uuid {
                let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if is_valid_hw_id(&id) {
                    return DeviceInfo {
                        id: format!("HWID-{}", id.to_uppercase()),
                        name,
                    };
                }
            }

            // Fallback: first physical network adapter MAC address.
            let mac = Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    "(Get-CimInstance Win32_NetworkAdapter -Filter 'PhysicalAdapter=True AND MACAddress IS NOT NULL' | Sort-Object DeviceID | Select-Object -First 1).MACAddress",
                ])
                .creation_flags(0x08000000)
                .output();

            if let Ok(output) = mac {
                let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !id.is_empty() {
                    return DeviceInfo {
                        id: format!("MAC-{}", id.to_uppercase().replace(':', "-")),
                        name,
                    };
                }
            }

            return DeviceInfo {
                id: "UNKNOWN-DEVICE".to_string(),
                name,
            };
        }

        #[cfg(not(target_os = "windows"))]
        {
            DeviceInfo {
                id: "UNKNOWN-DEVICE".to_string(),
                name: "Unknown PC".to_string(),
            }
        }
    })
    .await
    .unwrap_or_else(|_| DeviceInfo {
        id: "UNKNOWN-DEVICE".to_string(),
        name: "Unknown PC".to_string(),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_sql::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            greet,
            get_printers,
            print_receipt_raw,
            print_receipt_text,
            get_device_id
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
