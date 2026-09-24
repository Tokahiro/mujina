//! Data resources of the running executable, such as the package that
//! `packaging/attach-payload.ps1` adds to the built Mujina Setup.

use std::ptr::null;

use windows_sys::Win32::System::LibraryLoader::{
    FindResourceW, GetModuleHandleW, LoadResource, LockResource, SizeofResource,
};

use crate::wide::to_wide;

/// `RT_RCDATA`, raw data: `MAKEINTRESOURCE(10)`. Defined here rather than taken from the
/// windows-sys feature that happens to carry it (kernel streaming).
const RT_RCDATA: u16 = 10;

/// The bytes of the raw-data resource `name` (e.g. `MUJINA_MSIX`) of the running executable, or
/// `None` if it has none of that name. They live as long as the process: a module's resources are
/// part of its image and are never freed while it is loaded.
pub fn own_data(name: &str) -> Option<&'static [u8]> {
    let name = to_wide(name);
    // SAFETY: a null name asks for the executable's own module, which is loaded for as long as
    // the process runs.
    let module = unsafe { GetModuleHandleW(null()) };
    if module.is_null() {
        return None;
    }
    // SAFETY: `module` is loaded; `name` is NUL-terminated and outlives the call; the type is an
    // integer resource id, as MAKEINTRESOURCE makes it.
    let found = unsafe {
        FindResourceW(
            module,
            name.as_ptr(),
            std::ptr::without_provenance(usize::from(RT_RCDATA)),
        )
    };
    if found.is_null() {
        return None;
    }
    // SAFETY: `found` is a resource of `module`.
    let size = unsafe { SizeofResource(module, found) } as usize;
    // SAFETY: as above. For a module's own resources LoadResource returns a pointer into the
    // mapped image; there is nothing to free.
    let loaded = unsafe { LoadResource(module, found) };
    if loaded.is_null() {
        return None;
    }
    // SAFETY: `loaded` came from LoadResource; the result points to the resource's first byte.
    let data = unsafe { LockResource(loaded) };
    if data.is_null() {
        return None;
    }
    // SAFETY: the resource is `size` readable bytes at `data`, part of the executable's image,
    // which stays mapped and unchanged for the rest of the process.
    Some(unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_resource_the_executable_does_not_have_is_none() {
        assert_eq!(super::own_data("MUJINA_NO_SUCH_RESOURCE"), None);
    }
}
