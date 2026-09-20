//! The Win32 declarations, and nothing else.

use std::ffi::c_void;

pub type Handle = *mut c_void;
pub type Bool = i32;
pub type Dword = u32;

pub const FALSE: Bool = 0;
pub const INVALID_HANDLE_VALUE: Handle = usize::MAX as Handle;

/// `EnumPrinters` flags. Local printers plus the ones this machine has connected to on other
/// PCs.
pub const PRINTER_ENUM_LOCAL: Dword = 0x0000_0002;
pub const PRINTER_ENUM_CONNECTIONS: Dword = 0x0000_0004;

pub const PRINTER_ATTRIBUTE_NETWORK: Dword = 0x0000_0010;
pub const PRINTER_ATTRIBUTE_DEFAULT: Dword = 0x0000_0004;

/// The buffer was too small; `needed` says by how much.
pub const ERROR_INSUFFICIENT_BUFFER: Dword = 122;

/// `SetJob` with this command deletes the job, even one the port is holding.
pub const JOB_CONTROL_DELETE: Dword = 5;

/// `PRINTER_INFO_2.Status` bits a shop can act on.
pub const PRINTER_STATUS_PAUSED: Dword = 0x0000_0001;
pub const PRINTER_STATUS_ERROR: Dword = 0x0000_0002;
pub const PRINTER_STATUS_PAPER_JAM: Dword = 0x0000_0008;
pub const PRINTER_STATUS_PAPER_OUT: Dword = 0x0000_0010;
pub const PRINTER_STATUS_PAPER_PROBLEM: Dword = 0x0000_0040;
pub const PRINTER_STATUS_OFFLINE: Dword = 0x0000_0080;
pub const PRINTER_STATUS_NOT_AVAILABLE: Dword = 0x0000_1000;
pub const PRINTER_STATUS_USER_INTERVENTION: Dword = 0x0010_0000;
pub const PRINTER_STATUS_DOOR_OPEN: Dword = 0x0040_0000;

// CreateFile, for the serial port.
pub const GENERIC_WRITE: Dword = 0x4000_0000;
pub const GENERIC_READ: Dword = 0x8000_0000;
pub const OPEN_EXISTING: Dword = 3;

pub const NOPARITY: u8 = 0;
pub const ONESTOPBIT: u8 = 0;
/// `DCB` packs its sixteen flags into one word.
pub const DCB_F_BINARY: Dword = 0x0000_0001;
/// Bit 12–13 is `fRtsControl`; 1 (RTS_CONTROL_ENABLE) raises RTS and leaves it raised, which is
/// what a printer on a three-wire cable expects.
pub const DCB_F_RTS_ENABLE: Dword = 0x0000_1000;
/// Bit 4–5 is `fDtrControl`; 1 (DTR_CONTROL_ENABLE) does the same for DTR.
pub const DCB_F_DTR_ENABLE: Dword = 0x0000_0010;

/// `PRINTER_INFO_4W` — the cheapest level that carries a name.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PrinterInfo4W {
    pub printer_name: *mut u16,
    pub server_name: *mut u16,
    pub attributes: Dword,
}

/// `DOC_INFO_1W` — the job as the Windows queue window will show it.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DocInfo1W {
    pub doc_name: *const u16,
    pub output_file: *const u16,
    pub datatype: *const u16,
}

/// `SYSTEMTIME`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SystemTime {
    pub year: u16,
    pub month: u16,
    pub day_of_week: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    pub milliseconds: u16,
}

/// `JOB_INFO_1W`: one job in a printer's queue.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct JobInfo1W {
    pub job_id: Dword,
    pub printer_name: *mut u16,
    pub machine_name: *mut u16,
    pub user_name: *mut u16,
    pub document: *mut u16,
    pub datatype: *mut u16,
    pub status_text: *mut u16,
    pub status: Dword,
    pub priority: Dword,
    pub position: Dword,
    pub total_pages: Dword,
    pub pages_printed: Dword,
    pub submitted: SystemTime,
}

/// `PRINTER_INFO_2W`: the printer's own state, of which only `status` is read.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PrinterInfo2W {
    pub server_name: *mut u16,
    pub printer_name: *mut u16,
    pub share_name: *mut u16,
    pub port_name: *mut u16,
    pub driver_name: *mut u16,
    pub comment: *mut u16,
    pub location: *mut u16,
    pub dev_mode: *mut c_void,
    pub sep_file: *mut u16,
    pub print_processor: *mut u16,
    pub datatype: *mut u16,
    pub parameters: *mut u16,
    pub security_descriptor: *mut c_void,
    pub attributes: Dword,
    pub priority: Dword,
    pub default_priority: Dword,
    pub start_time: Dword,
    pub until_time: Dword,
    pub status: Dword,
    pub jobs: Dword,
    pub average_ppm: Dword,
}

/// `DCB` — the serial line settings.
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
    pub fn EnumPrintersW(
        flags: Dword,
        name: *const u16,
        level: Dword,
        buffer: *mut u8,
        buffer_bytes: Dword,
        needed: *mut Dword,
        returned: *mut Dword,
    ) -> Bool;

    pub fn GetDefaultPrinterW(buffer: *mut u16, size: *mut Dword) -> Bool;

    pub fn OpenPrinterW(name: *const u16, printer: *mut Handle, defaults: *mut c_void) -> Bool;

    pub fn ClosePrinter(printer: Handle) -> Bool;

    /// docs.microsoft.com/windows/win32/printdocs/startdocprinter Returns the job id, or zero
    /// on failure.
    pub fn StartDocPrinterW(printer: Handle, level: Dword, info: *const DocInfo1W) -> Dword;

    pub fn EndDocPrinter(printer: Handle) -> Bool;

    pub fn StartPagePrinter(printer: Handle) -> Bool;

    pub fn EndPagePrinter(printer: Handle) -> Bool;

    pub fn WritePrinter(
        printer: Handle,
        buffer: *const u8,
        bytes: Dword,
        written: *mut Dword,
    ) -> Bool;

    pub fn EnumJobsW(
        printer: Handle,
        first: Dword,
        count: Dword,
        level: Dword,
        buffer: *mut u8,
        buffer_bytes: Dword,
        needed: *mut Dword,
        returned: *mut Dword,
    ) -> Bool;

    pub fn SetJobW(
        printer: Handle,
        job: Dword,
        level: Dword,
        info: *mut u8,
        command: Dword,
    ) -> Bool;

    pub fn GetPrinterW(
        printer: Handle,
        level: Dword,
        buffer: *mut u8,
        buffer_bytes: Dword,
        needed: *mut Dword,
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

    /// The scale talks back, which is the one device in this product that sends bytes rather
    /// than receiving them.
    pub fn ReadFile(
        file: Handle,
        buffer: *mut u8,
        bytes: Dword,
        read: *mut Dword,
        overlapped: *mut c_void,
    ) -> Bool;

    pub fn FlushFileBuffers(file: Handle) -> Bool;

    pub fn CloseHandle(object: Handle) -> Bool;

    pub fn GetCommState(file: Handle, dcb: *mut Dcb) -> Bool;

    pub fn SetCommState(file: Handle, dcb: *const Dcb) -> Bool;

    pub fn SetCommTimeouts(file: Handle, timeouts: *const CommTimeouts) -> Bool;

    /// One bit per drive letter, A at bit 0.
    pub fn GetLogicalDrives() -> Dword;

    /// `DRIVE_REMOVABLE` and friends for a root like `E:\`.
    pub fn GetDriveTypeW(root: *const u16) -> Dword;

    pub fn GetVolumeInformationW(
        root: *const u16,
        volume_name: *mut u16,
        volume_name_size: Dword,
        serial: *mut Dword,
        max_component: *mut Dword,
        flags: *mut Dword,
        file_system: *mut u16,
        file_system_size: Dword,
    ) -> Bool;
}

/// `GetDriveTypeW` answers.
pub const DRIVE_REMOVABLE: Dword = 2;
pub const DRIVE_FIXED: Dword = 3;
pub const DRIVE_REMOTE: Dword = 4;

/// A NUL-terminated UTF-16 copy of `s`, which is what every `…W` function wants.
pub fn wide(s: &str) -> Option<Vec<u16>> {
    if s.contains('\0') {
        return None;
    }
    let mut out: Vec<u16> = s.encode_utf16().collect();
    out.push(0);
    Some(out)
}

/// Read a NUL-terminated UTF-16 string out of a buffer the spooler filled in.
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

// COM and the shell, for the firewall.

pub type Hresult = i32;

/// A COM `GUID`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

pub const IID_NULL: Guid = Guid {
    data1: 0,
    data2: 0,
    data3: 0,
    data4: [0; 8],
};
/// `{00020400-0000-0000-C000-000000000046}`.
pub const IID_IDISPATCH: Guid = Guid {
    data1: 0x0002_0400,
    data2: 0,
    data3: 0,
    data4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46],
};
/// `{00020404-0000-0000-C000-000000000046}`.
pub const IID_IENUMVARIANT: Guid = Guid {
    data1: 0x0002_0404,
    data2: 0,
    data3: 0,
    data4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46],
};

pub const S_OK: Hresult = 0;
/// `CoInitializeEx` on a thread that already has a COM apartment of the other kind.
pub const RPC_E_CHANGED_MODE: Hresult = i32::from_ne_bytes(0x8001_0106_u32.to_ne_bytes());
pub const COINIT_APARTMENTTHREADED: Dword = 0x2;
pub const CLSCTX_INPROC_SERVER: Dword = 0x1;
pub const LOCALE_USER_DEFAULT: Dword = 0x400;
pub const DISPATCH_METHOD: u16 = 0x1;
pub const DISPATCH_PROPERTYGET: u16 = 0x2;
/// The `_NewEnum` property every automation collection has.
pub const DISPID_NEWENUM: i32 = -4;

/// `VARENUM` — the kinds a `VARIANT` can hold, of which these are read.
pub const VT_EMPTY: u16 = 0;
pub const VT_I2: u16 = 2;
pub const VT_I4: u16 = 3;
pub const VT_BSTR: u16 = 8;
pub const VT_DISPATCH: u16 = 9;
pub const VT_BOOL: u16 = 11;
pub const VT_UNKNOWN: u16 = 13;

/// The union half of a `VARIANT`: two pointers wide (the `BRECORD` member is the largest).
pub const VARIANT_DATA_LEN: usize = 2 * size_of::<usize>();

/// `VARIANT`. Eight bytes of header, then the union, and the whole thing aligned to eight
/// because the union holds a `double` and a `LONGLONG`.
#[repr(C, align(8))]
#[derive(Debug, Clone, Copy)]
pub struct Variant {
    pub vt: u16,
    pub reserved: [u16; 3],
    pub data: [u8; VARIANT_DATA_LEN],
}

impl Variant {
    /// `VariantInit`: `VT_EMPTY`, nothing to free.
    pub const fn empty() -> Self {
        Variant {
            vt: VT_EMPTY,
            reserved: [0; 3],
            data: [0; VARIANT_DATA_LEN],
        }
    }
}

/// `DISPPARAMS`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DispParams {
    pub args: *mut Variant,
    pub named_ids: *mut i32,
    pub n_args: u32,
    pub n_named: u32,
}

/// A COM object: the first word of it is its vtable.
pub type ComObject = *mut c_void;

/// `IUnknown`'s three slots.
#[repr(C)]
pub struct IUnknownVtbl {
    pub query_interface:
        unsafe extern "system" fn(this: ComObject, iid: *const Guid, out: *mut ComObject) -> Hresult,
    pub add_ref: unsafe extern "system" fn(this: ComObject) -> u32,
    pub release: unsafe extern "system" fn(this: ComObject) -> u32,
}

/// `IDispatch` — the late-bound door every automation object has, which is how the firewall
/// is read without carrying its forty-method interface layouts in here.
#[repr(C)]
pub struct IDispatchVtbl {
    pub base: IUnknownVtbl,
    pub get_type_info_count: *const c_void,
    pub get_type_info: *const c_void,
    pub get_ids_of_names: unsafe extern "system" fn(
        this: ComObject,
        iid: *const Guid,
        names: *const *const u16,
        count: u32,
        lcid: Dword,
        ids: *mut i32,
    ) -> Hresult,
    pub invoke: unsafe extern "system" fn(
        this: ComObject,
        dispid: i32,
        iid: *const Guid,
        lcid: Dword,
        flags: u16,
        params: *mut DispParams,
        result: *mut Variant,
        exception: *mut c_void,
        arg_err: *mut u32,
    ) -> Hresult,
}

/// `IEnumVARIANT`.
#[repr(C)]
pub struct IEnumVariantVtbl {
    pub base: IUnknownVtbl,
    pub next: unsafe extern "system" fn(
        this: ComObject,
        count: u32,
        out: *mut Variant,
        fetched: *mut u32,
    ) -> Hresult,
    pub skip: *const c_void,
    pub reset: *const c_void,
    pub clone: *const c_void,
}

/// `SHELLEXECUTEINFOW`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ShellExecuteInfoW {
    pub size: Dword,
    pub mask: Dword,
    pub hwnd: Handle,
    pub verb: *const u16,
    pub file: *const u16,
    pub parameters: *const u16,
    pub directory: *const u16,
    pub show: i32,
    pub inst_app: Handle,
    pub id_list: *mut c_void,
    pub class: *const u16,
    pub hkey_class: Handle,
    pub hot_key: Dword,
    pub icon: Handle,
    pub process: Handle,
}

/// Hand back the process handle.
pub const SEE_MASK_NOCLOSEPROCESS: Dword = 0x40;
/// Do not return before the launch is done, so the handle is real.
pub const SEE_MASK_NOASYNC: Dword = 0x100;
/// No error box from the shell; the caller reads `GetLastError`.
pub const SEE_MASK_FLAG_NO_UI: Dword = 0x400;
pub const SW_HIDE: i32 = 0;
/// `GetLastError` after a UAC prompt the person answered No to.
pub const ERROR_CANCELLED: Dword = 1223;
pub const WAIT_OBJECT_0: Dword = 0;
pub const WAIT_TIMEOUT: Dword = 258;

#[link(name = "ole32")]
unsafe extern "system" {
    pub fn CoInitializeEx(reserved: *mut c_void, coinit: Dword) -> Hresult;
    pub fn CoUninitialize();
    pub fn CLSIDFromProgID(progid: *const u16, clsid: *mut Guid) -> Hresult;
    pub fn CoCreateInstance(
        clsid: *const Guid,
        outer: *mut c_void,
        context: Dword,
        iid: *const Guid,
        out: *mut ComObject,
    ) -> Hresult;
}

#[link(name = "oleaut32")]
unsafe extern "system" {
    /// Characters, not bytes, and no terminator.
    pub fn SysStringLen(bstr: *const u16) -> u32;
    pub fn VariantClear(variant: *mut Variant) -> Hresult;
}

#[link(name = "shell32")]
unsafe extern "system" {
    pub fn ShellExecuteExW(info: *mut ShellExecuteInfoW) -> Bool;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    pub fn WaitForSingleObject(object: Handle, milliseconds: Dword) -> Dword;
    pub fn GetExitCodeProcess(process: Handle, code: *mut Dword) -> Bool;
}
