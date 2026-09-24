//! The machine's `TrustedPeople` certificate store, whose certificates Windows trusts for MSIX.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Security::Cryptography::{
    CERT_CONTEXT, CERT_FIND_EXISTING, CERT_QUERY_ENCODING_TYPE, CERT_STORE_ADD_REPLACE_EXISTING,
    CERT_STORE_OPEN_EXISTING_FLAG, CERT_STORE_PROV_SYSTEM_W, CERT_STORE_READONLY_FLAG,
    CERT_SYSTEM_STORE_LOCAL_MACHINE, CertAddEncodedCertificateToStore, CertCloseStore,
    CertCreateCertificateContext, CertFindCertificateInStore, CertFreeCertificateContext,
    CertOpenStore, HCERTSTORE, PKCS_7_ASN_ENCODING, X509_ASN_ENCODING,
};

use crate::wide::to_wide;

/// Not `Root`: trusted for packages only, as Microsoft's sideloading guide advises.
const STORE: &str = "TrustedPeople";

/// The certificate functions expect both encoding types.
const ENCODING: CERT_QUERY_ENCODING_TYPE = X509_ASN_ENCODING | PKCS_7_ASN_ENCODING;

/// Whether exactly this certificate (its DER bytes) is in `LocalMachine\TrustedPeople`: the
/// same certificate, not merely one with the same name. Reading needs no administrator rights.
pub fn contains(der: &[u8]) -> bool {
    let Some(certificate) = Certificate::parse(der) else {
        return false;
    };
    Store::open(STORE, Access::Read).is_ok_and(|store| store.holds(&certificate))
}

/// Adds this DER certificate, replacing an identical one. Needs administrator rights.
pub fn add(der: &[u8]) -> Result<(), String> {
    if Certificate::parse(der).is_none() {
        return Err("not a certificate".to_string());
    }
    let length = u32::try_from(der.len()).map_err(|_| "certificate too large".to_string())?;
    let store = Store::open(STORE, Access::Write)?;
    // SAFETY: open store; `der` is readable for `length` bytes; a null out pointer asks for none.
    let added = unsafe {
        CertAddEncodedCertificateToStore(
            store.0,
            ENCODING,
            der.as_ptr(),
            length,
            CERT_STORE_ADD_REPLACE_EXISTING,
            null_mut(),
        )
    };
    if added == 0 {
        return Err(format!(
            "LocalMachine\\{STORE}: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Access {
    Read,
    Write,
}

struct Store(HCERTSTORE);

impl Store {
    fn open(name: &str, access: Access) -> Result<Self, String> {
        let wide = to_wide(name);
        // Reading neither needs write access nor creates a store that is not there.
        let flags = CERT_SYSTEM_STORE_LOCAL_MACHINE
            | match access {
                Access::Read => CERT_STORE_READONLY_FLAG | CERT_STORE_OPEN_EXISTING_FLAG,
                Access::Write => 0,
            };
        // SAFETY: this provider takes a NUL-terminated UTF-16 name, which outlives the call.
        let store =
            unsafe { CertOpenStore(CERT_STORE_PROV_SYSTEM_W, 0, 0, flags, wide.as_ptr().cast()) };
        if store.is_null() {
            return Err(format!(
                "LocalMachine\\{name}: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self(store))
    }

    fn holds(&self, certificate: &Certificate) -> bool {
        // SAFETY: open store; CERT_FIND_EXISTING takes a valid context; null starts at the first.
        let found = unsafe {
            CertFindCertificateInStore(
                self.0,
                ENCODING,
                0,
                CERT_FIND_EXISTING,
                certificate.0.cast(),
                null(),
            )
        };
        // Wrapped so that it is freed.
        Certificate::from_raw(found).is_some()
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        // SAFETY: the handle came from CertOpenStore and is closed exactly once.
        unsafe { CertCloseStore(self.0, 0) };
    }
}

struct Certificate(*const CERT_CONTEXT);

impl Certificate {
    fn parse(der: &[u8]) -> Option<Self> {
        if der.is_empty() {
            return None;
        }
        let length = u32::try_from(der.len()).ok()?;
        // SAFETY: `der` is readable for `length` bytes; the context keeps its own copy of them.
        let context = unsafe { CertCreateCertificateContext(ENCODING, der.as_ptr(), length) };
        Self::from_raw(context)
    }

    /// Takes over a context the caller must free, if there is one.
    fn from_raw(context: *const CERT_CONTEXT) -> Option<Self> {
        (!context.is_null()).then_some(Self(context))
    }
}

impl Drop for Certificate {
    fn drop(&mut self) {
        // SAFETY: a context the caller must free (see `from_raw`), freed exactly once.
        unsafe { CertFreeCertificateContext(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Security::Cryptography::CertEnumCertificatesInStore;

    #[test]
    fn what_is_no_certificate_is_neither_found_nor_added() {
        assert!(!contains(&[]));
        assert!(!contains(b"not a certificate"));
        // Refused before the store is opened, so this changes nothing on the machine.
        assert!(add(&[]).is_err());
        assert!(add(b"not a certificate").is_err());
    }

    #[test]
    fn a_certificate_is_found_only_as_it_is() {
        let store = Store::open("Root", Access::Read).unwrap();
        // SAFETY: the store is open; a null previous context asks for the first certificate.
        let first = unsafe { CertEnumCertificatesInStore(store.0, null()) };
        let first = Certificate::from_raw(first).expect("the root store is never empty");
        // SAFETY: the context's encoded bytes live as long as `first`, which outlives the copy.
        let mut der = unsafe {
            std::slice::from_raw_parts((*first.0).pbCertEncoded, (*first.0).cbCertEncoded as usize)
        }
        .to_vec();
        assert!(store.holds(&Certificate::parse(&der).unwrap()));

        // The last byte belongs to the signature: still a certificate, but not that one.
        if let Some(last) = der.last_mut() {
            *last ^= 0xFF;
        }
        assert!(!store.holds(&Certificate::parse(&der).unwrap()));
    }
}
