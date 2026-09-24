//! Data resources of the running executable, e.g. the package `packaging/attach-payload.ps1` adds.

use std::ptr::null;

use windows_sys::Win32::System::LibraryLoader::{
    FindResourceW, GetModuleHandleW, LoadResource, LockResource, SizeofResource,
};

use crate::wide::to_wide;

/// `MAKEINTRESOURCE(10)`; windows-sys has it only behind the kernel-streaming feature.
const RT_RCDATA: u16 = 10;

/// The raw-data resource `name` (e.g. `MUJINA_MSIX`) of the running executable. `'static`: a
/// module's resources are part of its image and never freed while it is loaded.
pub fn own_data(name: &str) -> Option<&'static [u8]> {
    let name = to_wide(name);
    // SAFETY: a null name asks for the executable's own module, loaded for the whole process.
    let module = unsafe { GetModuleHandleW(null()) };
    if module.is_null() {
        return None;
    }
    // SAFETY: `module` is loaded; `name` is NUL-terminated; the type is a MAKEINTRESOURCE id.
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
    // SAFETY: as above; for an own resource this points into the mapped image, nothing to free.
    let loaded = unsafe { LoadResource(module, found) };
    if loaded.is_null() {
        return None;
    }
    // SAFETY: `loaded` came from LoadResource; the result points to the resource's first byte.
    let data = unsafe { LockResource(loaded) };
    if data.is_null() {
        return None;
    }
    // SAFETY: `size` readable bytes at `data`, in the image, which stays mapped and unchanged.
    Some(unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_resource_the_executable_does_not_have_is_none() {
        assert_eq!(super::own_data("MUJINA_NO_SUCH_RESOURCE"), None);
    }
}
