//! MSIX package identity: family names, and which packages are installed.

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
use windows_sys::Win32::Storage::Packaging::Appx::{
    GetCurrentPackageFamilyName, GetPackagesByPackageFamily, PACKAGE_FAMILY_NAME_MAX_LENGTH,
    PACKAGE_ID, PackageFamilyNameFromId,
};

use crate::wide::{from_wide, to_wide};

/// For the manifest's `Identity` name and publisher, exactly as written; the same for every
/// version and architecture. `None` if Windows rejects the pair.
pub fn family_name_from_id(name: &str, publisher: &str) -> Option<String> {
    let mut name = to_wide(name);
    let mut publisher = to_wide(publisher);
    // SAFETY: PACKAGE_ID is plain data for which all-zero is a valid value.
    let mut id: PACKAGE_ID = unsafe { std::mem::zeroed() };
    id.name = name.as_mut_ptr();
    id.publisher = publisher.as_mut_ptr();
    let mut buffer = [0u16; PACKAGE_FAMILY_NAME_MAX_LENGTH as usize + 1];
    let mut length = PACKAGE_FAMILY_NAME_MAX_LENGTH + 1;
    // SAFETY: `id` and its NUL-terminated strings outlive the call; `buffer` fits `length` units.
    let status =
        unsafe { PackageFamilyNameFromId(&raw const id, &raw mut length, buffer.as_mut_ptr()) };
    (status == ERROR_SUCCESS).then(|| from_wide(&buffer))
}

/// For the current user.
pub fn is_installed(family: &str) -> bool {
    let family = to_wide(family);
    let mut count: u32 = 0;
    let mut length: u32 = 0;
    // SAFETY: `family` is NUL-terminated and outlives the call; null buffers ask for the count.
    let status = unsafe {
        GetPackagesByPackageFamily(
            family.as_ptr(),
            &raw mut count,
            null_mut(),
            &raw mut length,
            null_mut(),
        )
    };
    (status == ERROR_SUCCESS || status == ERROR_INSUFFICIENT_BUFFER) && count > 0
}

/// For the current user, e.g. `Mujina_0.27.0.0_x64__k2veznmcx4n98` (name, version, architecture,
/// resource id, publisher id). Empty if none is installed or Windows does not say.
pub fn installed_full_names(family: &str) -> Vec<String> {
    let family = to_wide(family);
    let mut count: u32 = 0;
    let mut length: u32 = 0;
    // SAFETY: as in `is_installed`: the documented way to ask for the sizes first.
    let status = unsafe {
        GetPackagesByPackageFamily(
            family.as_ptr(),
            &raw mut count,
            null_mut(),
            &raw mut length,
            null_mut(),
        )
    };
    if status != ERROR_INSUFFICIENT_BUFFER || count == 0 {
        return Vec::new();
    }
    let mut names: Vec<*mut u16> = vec![null_mut(); count as usize];
    let mut buffer = vec![0u16; length as usize];
    // SAFETY: `names` and `buffer` have the sizes Windows asked for.
    let status = unsafe {
        GetPackagesByPackageFamily(
            family.as_ptr(),
            &raw mut count,
            names.as_mut_ptr(),
            &raw mut length,
            buffer.as_mut_ptr(),
        )
    };
    if status != ERROR_SUCCESS {
        return Vec::new();
    }
    // Each pointer points into `buffer`: read each name from there, by its offset, up to its NUL.
    let start = buffer.as_ptr() as usize;
    names
        .iter()
        .take(count as usize)
        .filter_map(|&name| {
            let offset = (name as usize).checked_sub(start)? / size_of::<u16>();
            let rest = buffer.get(offset..)?;
            let end = rest.iter().position(|&unit| unit == 0)?;
            Some(String::from_utf16_lossy(&rest[..end]))
        })
        .collect()
}

/// `None` when this process runs unpackaged.
pub fn family_name() -> Option<String> {
    let mut length: u32 = 0;
    // SAFETY: a null buffer with length 0 is the documented way to query the required length.
    let status = unsafe { GetCurrentPackageFamilyName(&raw mut length, null_mut()) };
    if status != ERROR_INSUFFICIENT_BUFFER {
        return None;
    }
    let mut buffer = vec![0u16; length as usize];
    // SAFETY: `buffer` is writable for `length` UTF-16 units.
    let status = unsafe { GetCurrentPackageFamilyName(&raw mut length, buffer.as_mut_ptr()) };
    (status == ERROR_SUCCESS).then(|| from_wide(&buffer))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_process_is_unpackaged() {
        assert_eq!(super::family_name(), None);
    }

    #[test]
    fn the_family_follows_from_name_and_publisher() {
        // Windows' calculator, as `Get-AppxPackage` reports it.
        assert_eq!(
            super::family_name_from_id(
                "Microsoft.WindowsCalculator",
                "CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US"
            )
            .as_deref(),
            Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe")
        );
        // The release's publisher, and its family, which must never change.
        assert_eq!(
            super::family_name_from_id("Mujina", "CN=Mujina").as_deref(),
            Some("Mujina_k2veznmcx4n98")
        );
        assert_ne!(
            super::family_name_from_id("Mujina", "CN=Mujina"),
            super::family_name_from_id("Mujina", "CN=Mujina Dev")
        );
    }

    #[test]
    fn an_unknown_family_is_not_installed() {
        assert!(!super::is_installed(
            "Mujina.Test.NoSuchPackage_0000000000000"
        ));
        assert!(super::installed_full_names("Mujina.Test.NoSuchPackage_0000000000000").is_empty());
    }

    /// A stripped test image may lack the calculator.
    #[test]
    fn an_installed_family_names_its_packages() {
        let family = "Microsoft.WindowsCalculator_8wekyb3d8bbwe";
        let names = super::installed_full_names(family);
        assert_eq!(!names.is_empty(), super::is_installed(family));
        for name in names {
            assert!(
                name.starts_with("Microsoft.WindowsCalculator_")
                    && name.ends_with("__8wekyb3d8bbwe"),
                "{name}"
            );
        }
    }
}
