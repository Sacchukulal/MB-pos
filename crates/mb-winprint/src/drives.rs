//! The drive letters, and what each one is: a pen drive, a disk, a share.

use crate::sys;
use crate::{DriveInfo, DriveKind};

/// The label buffer: `MAX_PATH` + 1 is what the API documents.
const LABEL_LEN: usize = 261;

pub fn list() -> Vec<DriveInfo> {
    // SAFETY: takes nothing, answers a bit mask.
    let mask = unsafe { sys::GetLogicalDrives() };
    (0_u32..26)
        .filter(|bit| mask & (1 << bit) != 0)
        .filter_map(|bit| {
            let letter = char::from(b'A' + u8::try_from(bit).ok()?);
            let root = format!("{letter}:\\");
            let wide = sys::wide(&root)?;
            // SAFETY: `wide` is NUL-terminated and outlives the call.
            let kind = match unsafe { sys::GetDriveTypeW(wide.as_ptr()) } {
                sys::DRIVE_REMOVABLE => DriveKind::Removable,
                sys::DRIVE_FIXED => DriveKind::Fixed,
                sys::DRIVE_REMOTE => DriveKind::Network,
                _ => return None,
            };
            Some(DriveInfo {
                label: label_of(&wide),
                root,
                kind,
            })
        })
        .collect()
}

/// The volume label, or nothing: a drive with no medium in it answers nothing too.
fn label_of(root: &[u16]) -> String {
    let mut name = vec![0_u16; LABEL_LEN];
    // SAFETY: every pointer is to a buffer this function owns, sized as passed; the ones the
    // call does not need are allowed to be null.
    let ok = unsafe {
        sys::GetVolumeInformationW(
            root.as_ptr(),
            name.as_mut_ptr(),
            u32::try_from(name.len()).unwrap_or(0),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    if ok == sys::FALSE {
        return String::new();
    }
    let end = name.iter().position(|c| *c == 0).unwrap_or(0);
    String::from_utf16_lossy(&name[..end]).trim().to_owned()
}
