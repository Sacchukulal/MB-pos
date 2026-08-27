//! Enumerating Windows printers, and printing RAW bytes to one.

use std::ptr;

use crate::sys::{self, Dword, FALSE, Handle};
use crate::{PrinterInfo, WinPrintError};

/// A printer handle that closes itself, on every path including an unwind.
#[derive(Debug)]
struct Printer(Handle);

impl Drop for Printer {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from a successful `OpenPrinterW` and is closed
        // exactly once, here.
        unsafe { sys::ClosePrinter(self.0) };
    }
}

/// A document open on a printer, which must be ended even if a write fails.
#[derive(Debug)]
struct Document<'a> {
    printer: &'a Printer,
    page_open: bool,
}

impl Drop for Document<'_> {
    fn drop(&mut self) {
        if self.page_open {
            // SAFETY: a page was started on this handle and is ended once.
            unsafe { sys::EndPagePrinter(self.printer.0) };
        }
        // SAFETY: a document was started on this handle and is ended once.
        unsafe { sys::EndDocPrinter(self.printer.0) };
    }
}

fn last_error(what: &'static str) -> WinPrintError {
    // SAFETY: no arguments, no pointers; reads this thread's error code.
    let code = unsafe { sys::GetLastError() };
    WinPrintError::Api { what, code }
}

pub fn list() -> Result<Vec<PrinterInfo>, WinPrintError> {
    let flags = sys::PRINTER_ENUM_LOCAL | sys::PRINTER_ENUM_CONNECTIONS;
    let level = 4;

    // Two calls, which is how every EnumPrinters example is written: the first one is expected
    // to fail with ERROR_INSUFFICIENT_BUFFER and to say how many bytes are needed.
    let mut needed: Dword = 0;
    let mut returned: Dword = 0;
    // SAFETY: a zero-length buffer with a zero length is the documented way to
    // ask for the required size; `needed` and `returned` are valid to write.
    let ok = unsafe {
        sys::EnumPrintersW(
            flags,
            ptr::null(),
            level,
            ptr::null_mut(),
            0,
            &raw mut needed,
            &raw mut returned,
        )
    };
    if ok == FALSE {
        // SAFETY: no arguments.
        let code = unsafe { sys::GetLastError() };
        if code != sys::ERROR_INSUFFICIENT_BUFFER {
            return Err(WinPrintError::Api {
                what: "EnumPrinters (sizing)",
                code,
            });
        }
    }
    if needed == 0 {
        return Ok(Vec::new());
    }

    // The buffer holds an array of PRINTER_INFO_4W at the front and the strings they point at
    // behind it, so it must stay alive until every name has been copied out.
    let count = (needed as usize).div_ceil(size_of::<sys::PrinterInfo4W>());
    let mut buffer: Vec<sys::PrinterInfo4W> = vec![
        sys::PrinterInfo4W {
            printer_name: ptr::null_mut(),
            server_name: ptr::null_mut(),
            attributes: 0,
        };
        count.max(1)
    ];

    // SAFETY: the buffer is `count * size_of::<PRINTER_INFO_4W>()` bytes, which
    // is at least `needed`, and it is writable for that whole length.
    let ok = unsafe {
        sys::EnumPrintersW(
            flags,
            ptr::null(),
            level,
            buffer.as_mut_ptr().cast::<u8>(),
            needed,
            &raw mut needed,
            &raw mut returned,
        )
    };
    if ok == FALSE {
        return Err(last_error("EnumPrinters"));
    }

    let default = default_name()?.unwrap_or_default();
    let mut out = Vec::with_capacity(returned as usize);
    for index in 0..(returned as usize) {
        let Some(info) = buffer.get(index) else { break };
        // SAFETY: the spooler filled this entry, so `printer_name` is either
        // null or a NUL-terminated UTF-16 string inside `buffer`, which is
        // still alive.
        let name = unsafe { sys::from_wide(info.printer_name) };
        if name.is_empty() {
            continue;
        }
        out.push(PrinterInfo {
            is_default: name == default || info.attributes & sys::PRINTER_ATTRIBUTE_DEFAULT != 0,
            is_network: info.attributes & sys::PRINTER_ATTRIBUTE_NETWORK != 0,
            name,
        });
    }
    Ok(out)
}

pub fn default_name() -> Result<Option<String>, WinPrintError> {
    let mut size: Dword = 0;
    // SAFETY: the documented sizing call — a null buffer with a zero size.
    let _ = unsafe { sys::GetDefaultPrinterW(ptr::null_mut(), &raw mut size) };
    if size == 0 {
        return Ok(None);
    }
    let mut buffer = vec![0_u16; size as usize];
    // SAFETY: the buffer holds `size` UTF-16 units, which is what the sizing
    // call asked for.
    let ok = unsafe { sys::GetDefaultPrinterW(buffer.as_mut_ptr(), &raw mut size) };
    if ok == FALSE {
        // A machine with no printers at all has no default, and that is not an error — it is a
        // shop that has not plugged one in yet.
        return Ok(None);
    }
    // SAFETY: `buffer` is NUL-terminated by the call above and is still alive.
    let name = unsafe { sys::from_wide(buffer.as_ptr()) };
    Ok((!name.is_empty()).then_some(name))
}

/// Open a printer by the name Windows gives it.
fn open(printer: &str) -> Result<Printer, WinPrintError> {
    let name = sys::wide(printer).ok_or_else(|| WinPrintError::BadName {
        name: printer.to_owned(),
    })?;
    let mut handle: Handle = ptr::null_mut();
    // SAFETY: `name` is NUL-terminated and outlives the call; `handle` is
    // writable; a null `defaults` means "the printer's own settings".
    let ok = unsafe { sys::OpenPrinterW(name.as_ptr(), &raw mut handle, ptr::null_mut()) };
    if ok == FALSE || handle.is_null() {
        // SAFETY: no arguments.
        let code = unsafe { sys::GetLastError() };
        // 1801 is ERROR_INVALID_PRINTER_NAME, and it is the one a shop will actually hit —
        // somebody renamed the printer in Windows.
        return Err(if code == 1801 {
            WinPrintError::NoSuchPrinter {
                name: printer.to_owned(),
            }
        } else {
            WinPrintError::Api {
                what: "OpenPrinter",
                code,
            }
        });
    }
    Ok(Printer(handle))
}

/// Send bytes as one RAW document, telling `on_job` which Windows job they became the moment
/// it exists — so another thread can delete it if the port never takes them.
pub fn write_raw(
    printer: &str,
    document: &str,
    bytes: &[u8],
    mut on_job: impl FnMut(u32),
) -> Result<(), WinPrintError> {
    let doc_name = sys::wide(document).ok_or_else(|| WinPrintError::BadName {
        name: document.to_owned(),
    })?;
    // "RAW" is the whole point: it tells the driver these bytes are already the printer's own
    // language and must be passed through untouched.
    let raw = sys::wide("RAW").ok_or_else(|| WinPrintError::BadName {
        name: "RAW".to_owned(),
    })?;

    let printer_handle = open(printer)?;

    let info = sys::DocInfo1W {
        doc_name: doc_name.as_ptr(),
        output_file: ptr::null(),
        datatype: raw.as_ptr(),
    };
    // SAFETY: level 1 matches `DOC_INFO_1W`, and every pointer in it outlives
    // the call.
    let job = unsafe { sys::StartDocPrinterW(printer_handle.0, 1, &raw const info) };
    if job == 0 {
        return Err(last_error("StartDocPrinter"));
    }
    on_job(job);
    // From here on the document must be ended however this function leaves, so it is a guard
    // rather than a pair of calls somebody can return between.
    let mut open = Document {
        printer: &printer_handle,
        page_open: false,
    };

    // SAFETY: a document is open on this handle.
    if unsafe { sys::StartPagePrinter(printer_handle.0) } == FALSE {
        return Err(last_error("StartPagePrinter"));
    }
    open.page_open = true;

    // WritePrinter is allowed to accept fewer bytes than it was given, so this loops.
    let mut sent = 0_usize;
    while sent < bytes.len() {
        let chunk = &bytes[sent..];
        let len = Dword::try_from(chunk.len()).unwrap_or(Dword::MAX);
        let mut written: Dword = 0;
        // SAFETY: `chunk` is valid for `len` bytes and `written` is writable.
        let ok =
            unsafe { sys::WritePrinter(printer_handle.0, chunk.as_ptr(), len, &raw mut written) };
        if ok == FALSE {
            return Err(last_error("WritePrinter"));
        }
        if written == 0 {
            // Not an error code, but no progress either.
            return Err(WinPrintError::Api {
                what: "WritePrinter (accepted no bytes)",
                code: 0,
            });
        }
        sent += written as usize;
    }

    drop(open);
    drop(printer_handle);
    Ok(())
}

/// Delete one job from the printer's queue. This is what frees a `WritePrinter` that the port
/// is not answering: the blocked call returns with an error, and the thread behind it ends.
pub fn cancel_job(printer: &str, job_id: u32) -> Result<(), WinPrintError> {
    let printer_handle = open(printer)?;
    // SAFETY: the handle is open; level 0 with no job info is the documented form of a call
    // that only carries a command.
    let ok = unsafe {
        sys::SetJobW(
            printer_handle.0,
            job_id,
            0,
            ptr::null_mut(),
            sys::JOB_CONTROL_DELETE,
        )
    };
    if ok == FALSE {
        return Err(last_error("SetJob (delete)"));
    }
    Ok(())
}

/// Delete every job in the printer's queue whose document name starts with `prefix`, and say
/// how many went. A job hung at the head of a queue blocks every job behind it for ever, and
/// only our own documents are ever touched.
pub fn purge_jobs(printer: &str, prefix: &str) -> Result<usize, WinPrintError> {
    let printer_handle = open(printer)?;
    let level = 1;
    // Sizing call: expected to fail with ERROR_INSUFFICIENT_BUFFER and say how many bytes.
    let mut needed: Dword = 0;
    let mut returned: Dword = 0;
    // SAFETY: a zero-length buffer with a zero length is the documented way to ask for the
    // size; `needed` and `returned` are valid to write.
    let ok = unsafe {
        sys::EnumJobsW(
            printer_handle.0,
            0,
            Dword::MAX,
            level,
            ptr::null_mut(),
            0,
            &raw mut needed,
            &raw mut returned,
        )
    };
    if ok == FALSE {
        // SAFETY: no arguments.
        let code = unsafe { sys::GetLastError() };
        if code != sys::ERROR_INSUFFICIENT_BUFFER {
            return Err(WinPrintError::Api {
                what: "EnumJobs (sizing)",
                code,
            });
        }
    }
    if needed == 0 {
        return Ok(0);
    }

    // `u64`s, so the JOB_INFO_1W records at the front of the buffer are aligned for reading.
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(size_of::<u64>())];
    // SAFETY: the buffer is at least `needed` bytes and writable for that whole length.
    let ok = unsafe {
        sys::EnumJobsW(
            printer_handle.0,
            0,
            Dword::MAX,
            level,
            buffer.as_mut_ptr().cast::<u8>(),
            needed,
            &raw mut needed,
            &raw mut returned,
        )
    };
    if ok == FALSE {
        return Err(last_error("EnumJobs"));
    }

    let records = buffer.as_ptr().cast::<sys::JobInfo1W>();
    let mut gone = 0_usize;
    for index in 0..(returned as usize) {
        // SAFETY: the spooler filled `returned` records at the front of the buffer, which is
        // still alive and aligned.
        let job = unsafe { records.add(index).read() };
        // SAFETY: `document` is null or a NUL-terminated UTF-16 string inside the buffer.
        let document = unsafe { sys::from_wide(job.document) };
        if !document.starts_with(prefix) {
            continue;
        }
        // SAFETY: the handle is open; a command-only call.
        let ok = unsafe {
            sys::SetJobW(
                printer_handle.0,
                job.job_id,
                0,
                ptr::null_mut(),
                sys::JOB_CONTROL_DELETE,
            )
        };
        if ok != FALSE {
            gone += 1;
        }
    }
    Ok(gone)
}

/// What Windows says is wrong with the printer, in a shopkeeper's words — or nothing.
pub fn trouble(printer: &str) -> Result<Option<String>, WinPrintError> {
    let printer_handle = open(printer)?;
    let level = 2;
    let mut needed: Dword = 0;
    // SAFETY: the documented sizing call — a null buffer with a zero size.
    let _ = unsafe { sys::GetPrinterW(printer_handle.0, level, ptr::null_mut(), 0, &raw mut needed) };
    if needed == 0 {
        return Err(last_error("GetPrinter (sizing)"));
    }
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(size_of::<u64>())];
    // SAFETY: the buffer is at least `needed` bytes and writable for that whole length.
    let ok = unsafe {
        sys::GetPrinterW(
            printer_handle.0,
            level,
            buffer.as_mut_ptr().cast::<u8>(),
            needed,
            &raw mut needed,
        )
    };
    if ok == FALSE {
        return Err(last_error("GetPrinter"));
    }
    // SAFETY: level 2 fills a PRINTER_INFO_2W at the front of the buffer, which is aligned.
    let info = unsafe { buffer.as_ptr().cast::<sys::PrinterInfo2W>().read() };
    Ok(describe_status(printer, info.status))
}

/// The status bits a shop can act on, as one sentence.
fn describe_status(printer: &str, status: Dword) -> Option<String> {
    let words: Vec<&str> = [
        (sys::PRINTER_STATUS_OFFLINE, "is offline"),
        (sys::PRINTER_STATUS_PAUSED, "is paused in Windows"),
        (sys::PRINTER_STATUS_PAPER_OUT, "is out of paper"),
        (sys::PRINTER_STATUS_PAPER_JAM, "has a paper jam"),
        (sys::PRINTER_STATUS_PAPER_PROBLEM, "has a paper problem"),
        (sys::PRINTER_STATUS_DOOR_OPEN, "has its cover open"),
        (sys::PRINTER_STATUS_USER_INTERVENTION, "needs attention"),
        (sys::PRINTER_STATUS_NOT_AVAILABLE, "is not available"),
        (sys::PRINTER_STATUS_ERROR, "reports an error"),
    ]
    .into_iter()
    .filter(|(bit, _)| status & bit != 0)
    .map(|(_, words)| words)
    .collect();
    if words.is_empty() {
        return None;
    }
    Some(format!("the printer \"{printer}\" {}", words.join(" and ")))
}
