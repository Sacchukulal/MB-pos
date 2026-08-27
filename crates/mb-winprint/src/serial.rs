//! A COM port, opened and configured 8-N-1.

use std::io::{self, Read, Write};
use std::ptr;

use crate::WinPrintError;
use crate::sys::{self, Dword, FALSE, Handle};

/// An open serial port.
#[derive(Debug)]
pub struct SerialPort {
    handle: Handle,
    name: String,
}

// SAFETY: a Windows file handle is not tied to a thread, and this type hands
// out no interior references — the queue moves one to its worker thread and
// nothing else touches it.
unsafe impl Send for SerialPort {}

impl Drop for SerialPort {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful `CreateFileW` and is closed
        // exactly once, here.
        unsafe { sys::CloseHandle(self.handle) };
    }
}

impl Write for SerialPort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = Dword::try_from(buf.len()).unwrap_or(Dword::MAX);
        let mut written: Dword = 0;
        // SAFETY: `buf` is valid for `len` bytes, `written` is writable, and a
        // null `overlapped` means a synchronous write, which is what the handle
        // was opened for.
        let ok = unsafe {
            sys::WriteFile(
                self.handle,
                buf.as_ptr(),
                len,
                &raw mut written,
                ptr::null_mut(),
            )
        };
        if ok == FALSE {
            // SAFETY: no arguments.
            let code = unsafe { sys::GetLastError() };
            return Err(io::Error::other(format!(
                "writing to {} failed (Windows error {code})",
                self.name
            )));
        }
        Ok(written as usize)
    }

    fn flush(&mut self) -> io::Result<()> {
        // SAFETY: a valid, open handle.
        let ok = unsafe { sys::FlushFileBuffers(self.handle) };
        if ok == FALSE {
            // SAFETY: no arguments.
            let code = unsafe { sys::GetLastError() };
            return Err(io::Error::other(format!(
                "flushing {} failed (Windows error {code})",
                self.name
            )));
        }
        Ok(())
    }
}

impl Read for SerialPort {
    /// The scale, which is the one serial device that talks back.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let len = Dword::try_from(buf.len()).unwrap_or(Dword::MAX);
        let mut read: Dword = 0;
        // SAFETY: `buf` is valid for `len` bytes, `read` is writable, and a
        // null `overlapped` means a synchronous read on a handle opened for it.
        let ok = unsafe {
            sys::ReadFile(
                self.handle,
                buf.as_mut_ptr(),
                len,
                &raw mut read,
                ptr::null_mut(),
            )
        };
        if ok == FALSE {
            // SAFETY: no arguments.
            let code = unsafe { sys::GetLastError() };
            return Err(io::Error::other(format!(
                "reading from {} failed (Windows error {code})",
                self.name
            )));
        }
        Ok(read as usize)
    }
}

pub fn open(port: &str, baud: u32) -> Result<SerialPort, WinPrintError> {
    // COM10 and above need the `\\.\` prefix; COM1–COM9 work either way, so everything gets it.
    let path = if port.starts_with(r"\\.\") {
        port.to_owned()
    } else {
        format!(r"\\.\{port}")
    };
    let wide = sys::wide(&path).ok_or_else(|| WinPrintError::BadName {
        name: port.to_owned(),
    })?;

    // No sharing: two processes writing one printer interleave their bytes, and
    // half an ESC/POS stream inside another one prints nothing recognisable.
    // SAFETY: `wide` is NUL-terminated and outlives the call; the null security
    // descriptor and null template are the documented defaults.
    let handle = unsafe {
        sys::CreateFileW(
            wide.as_ptr(),
            sys::GENERIC_WRITE | sys::GENERIC_READ,
            0,
            ptr::null_mut(),
            sys::OPEN_EXISTING,
            0,
            ptr::null_mut(),
        )
    };
    if handle == sys::INVALID_HANDLE_VALUE || handle.is_null() {
        // SAFETY: no arguments.
        let code = unsafe { sys::GetLastError() };
        return Err(WinPrintError::Api {
            what: "CreateFile (serial port)",
            code,
        });
    }
    let opened = SerialPort {
        handle,
        name: port.to_owned(),
    };

    // Read the current settings and change only what we mean to.
    let mut dcb = sys::Dcb {
        dcb_length: Dword::try_from(size_of::<sys::Dcb>()).unwrap_or(0),
        ..sys::Dcb::default()
    };
    // SAFETY: `dcb` is a valid, writable DCB with its length set.
    if unsafe { sys::GetCommState(opened.handle, &raw mut dcb) } == FALSE {
        // SAFETY: no arguments.
        let code = unsafe { sys::GetLastError() };
        return Err(WinPrintError::Api {
            what: "GetCommState",
            code,
        });
    }

    dcb.baud_rate = baud;
    dcb.byte_size = 8;
    dcb.parity = sys::NOPARITY;
    dcb.stop_bits = sys::ONESTOPBIT;
    dcb.flags |= sys::DCB_F_BINARY | sys::DCB_F_RTS_ENABLE | sys::DCB_F_DTR_ENABLE;

    // SAFETY: a fully initialised DCB read from this same handle.
    if unsafe { sys::SetCommState(opened.handle, &raw const dcb) } == FALSE {
        // SAFETY: no arguments.
        let code = unsafe { sys::GetLastError() };
        return Err(WinPrintError::Api {
            what: "SetCommState",
            code,
        });
    }

    // A write that cannot finish must give up rather than hold the worker thread for ever.
    let timeouts = sys::CommTimeouts {
        read_interval_timeout: 50,
        read_total_timeout_constant: 200,
        read_total_timeout_multiplier: 0,
        write_total_timeout_constant: 5_000,
        write_total_timeout_multiplier: 1,
    };
    // SAFETY: a valid COMMTIMEOUTS and an open handle.
    if unsafe { sys::SetCommTimeouts(opened.handle, &raw const timeouts) } == FALSE {
        // SAFETY: no arguments.
        let code = unsafe { sys::GetLastError() };
        return Err(WinPrintError::Api {
            what: "SetCommTimeouts",
            code,
        });
    }

    Ok(opened)
}
