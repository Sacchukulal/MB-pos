//! The Win32 declarations, and nothing else.
//!
//! Nine functions and three structures, each named after the documentation page
//! it came from. Keeping them in one file means the whole of this crate's
//! contact with the operating system can be reviewed in one screen — which is
//! the only way an `unsafe` boundary stays reviewable.

use std::ffi::c_void;

pub type Handle = *mut c_void;
pub type Bool = i32;
pub type Dword = u32;

pub const FALSE: Bool = 0;
pub const INVALID_HANDLE_VALUE: Handle = usize::MAX as Handle;

/// `EnumPrinters` flags. Local printers plus the ones this machine has
/// connected to on other PCs — a kitchen printer shared from the back office is
/// the second kind and v1's users have them.
pub const PRINTER_ENUM_LOCAL: Dword = 0x0000_0002;
pub const PRINTER_ENUM_CONNECTIONS: Dword = 0x0000_0004;

/// `PRINTER_INFO_4W.Attributes`.
pub const PRINTER_ATTRIBUTE_NETWORK: Dword = 0x0000_0010;
pub const PRINTER_ATTRIBUTE_DEFAULT: Dword = 0x0000_0004;

/// The buffer was too small; `needed` says by how much. The only error code
/// this crate treats as ordinary rather than as a failure.
pub const ERROR_INSUFFICIENT_BUFFER: Dword = 122;

// CreateFile, for the serial port.
pub const GENERIC_WRITE: Dword = 0x4000_0000;
pub const GENERIC_READ: Dword = 0x8000_0000;
pub const OPEN_EXISTING: Dword = 3;

// DCB.
pub const NOPARITY: u8 = 0;
pub const ONESTOPBIT: u8 = 0;
/// `DCB` packs its sixteen flags into one word. Bit 0 is `fBinary`, which
/// Windows requires to be set — it has not supported non-binary mode since NT.
pub const DCB_F_BINARY: Dword = 0x0000_0001;
/// Bit 12–13 is `fRtsControl`; 1 (RTS_CONTROL_ENABLE) raises RTS and leaves it
/// raised, which is what a printer on a three-wire cable expects.
pub const DCB_F_RTS_ENABLE: Dword = 0x0000_1000;
/// Bit 4–5 is `fDtrControl`; 1 (DTR_CONTROL_ENABLE) does the same for DTR.
pub const DCB_F_DTR_ENABLE: Dword = 0x0000_0010;

/// `PRINTER_INFO_4W` — the cheapest level that carries a name. Level 2 would
/// bring the driver, the port, the comment and a `DEVMODE`, none of which this
/// crate looks at, and it costs a much larger buffer per printer.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PrinterInfo4W {
    pub printer_name: *mut u16,
    pub server_name: *mut u16,
    pub attributes: Dword,
}

/// `DOC_INFO_1W` — the job as the Windows queue window will show it.
///
/// `datatype` **must** be "RAW" or the driver renders a page instead of passing
/// our ESC/POS bytes through.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DocInfo1W {
    pub doc_name: *const u16,
    pub output_file: *const u16,
    pub datatype: *const u16,
}

/// `DCB` — the serial line settings. Laid out exactly as Windows declares it;
/// the flags are one packed word and the constants above name the bits we set.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Dcb {
    pub dcb_length: Dword,
    pub baud_rate: Dword,
    pub flags: Dword,
    pub w_reserved: u16,
    pub xon_lim: u16,
    pub xoff_lim: u16,
    pub byte_size: u8,
    pub parity: u8,
    pub stop_bits: u8,
    pub xon_char: i8,
    pub xoff_char: i8,
    pub error_char: i8,
    pub eof_char: i8,
    pub evt_char: i8,
    pub w_reserved1: u16,
}

/// `COMMTIMEOUTS`. Every field is milliseconds.
///
/// A printer that has stopped answering must not hold the worker thread for
/// ever — this is one of the timeouts P07 item 7.3 says *can* be honoured,
/// unlike the spooler's.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CommTimeouts {
    pub read_interval_timeout: Dword,
    pub read_total_timeout_multiplier: Dword,
    pub read_total_timeout_constant: Dword,
    pub write_total_timeout_multiplier: Dword,
    pub write_total_timeout_constant: Dword,
}

#[link(name = "winspool")]
unsafe extern "system" {
    /// docs.microsoft.com/windows/win32/printdocs/enumprinters
    pub fn EnumPrintersW(
        flags: Dword,
        name: *const u16,
        level: Dword,
        buffer: *mut u8,
        buffer_bytes: Dword,
        needed: *mut Dword,
        returned: *mut Dword,
    ) -> Bool;

    /// docs.microsoft.com/windows/win32/printdocs/getdefaultprinter
    pub fn GetDefaultPrinterW(buffer: *mut u16, size: *mut Dword) -> Bool;

    /// docs.microsoft.com/windows/win32/printdocs/openprinter
    pub fn OpenPrinterW(name: *const u16, printer: *mut Handle, defaults: *mut c_void) -> Bool;

    /// docs.microsoft.com/windows/win32/printdocs/closeprinter
    pub fn ClosePrinter(printer: Handle) -> Bool;

    /// docs.microsoft.com/windows/win32/printdocs/startdocprinter
    /// Returns the job id, or zero on failure.
    pub fn StartDocPrinterW(printer: Handle, level: Dword, info: *const DocInfo1W) -> Dword;

    /// docs.microsoft.com/windows/win32/printdocs/enddocprinter
    pub fn EndDocPrinter(printer: Handle) -> Bool;

    /// docs.microsoft.com/windows/win32/printdocs/startpageprinter
    pub fn StartPagePrinter(printer: Handle) -> Bool;

    /// docs.microsoft.com/windows/win32/printdocs/endpageprinter
    pub fn EndPagePrinter(printer: Handle) -> Bool;

    /// docs.microsoft.com/windows/win32/printdocs/writeprinter
    pub fn WritePrinter(
        printer: Handle,
        buffer: *const u8,
        bytes: Dword,
        written: *mut Dword,
    ) -> Bool;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    pub fn GetLastError() -> Dword;

    pub fn CreateFileW(
        name: *const u16,
        access: Dword,
        share: Dword,
        security: *mut c_void,
        creation: Dword,
        flags: Dword,
        template: Handle,
    ) -> Handle;

    pub fn WriteFile(
        file: Handle,
        buffer: *const u8,
        bytes: Dword,
        written: *mut Dword,
        overlapped: *mut c_void,
    ) -> Bool;

    pub fn FlushFileBuffers(file: Handle) -> Bool;

    pub fn CloseHandle(object: Handle) -> Bool;

    pub fn GetCommState(file: Handle, dcb: *mut Dcb) -> Bool;

    pub fn SetCommState(file: Handle, dcb: *const Dcb) -> Bool;

    pub fn SetCommTimeouts(file: Handle, timeouts: *const CommTimeouts) -> Bool;
}

/// A NUL-terminated UTF-16 copy of `s`, which is what every `…W` function wants.
///
/// `None` when the string contains a NUL of its own: passing it would silently
/// truncate, and a printer name that is silently truncated prints to the wrong
/// printer.
pub fn wide(s: &str) -> Option<Vec<u16>> {
    if s.contains('\0') {
        return None;
    }
    let mut out: Vec<u16> = s.encode_utf16().collect();
    out.push(0);
    Some(out)
}

/// Read a NUL-terminated UTF-16 string out of a buffer the spooler filled in.
///
/// # Safety
///
/// `ptr` must be null, or point to a NUL-terminated UTF-16 string that stays
/// valid for the length of this call.
pub unsafe fn from_wide(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0_usize;
    // SAFETY: the caller promises a NUL-terminated string, so this walks to the
    // terminator and no further.
    while unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: `len` characters were just proven readable.
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf16_lossy(slice)
}
