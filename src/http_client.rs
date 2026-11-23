use std::sync::OnceLock;
use std::time::Duration;

static WINDOWS_ROOT_CERTS: OnceLock<Vec<Vec<u8>>> = OnceLock::new();

#[cfg(windows)]
fn load_windows_root_certs_der() -> Vec<Vec<u8>> {
    use windows::core::w;
    use windows::Win32::Security::Cryptography::{
        CertCloseStore, CertEnumCertificatesInStore, CertOpenSystemStoreW, X509_ASN_ENCODING,
    };

    let mut certs = Vec::new();
    unsafe {
        let Ok(store) = CertOpenSystemStoreW(None, w!("ROOT")) else {
            return certs;
        };
        if store.is_invalid() {
            return certs;
        }
        let mut ctx = CertEnumCertificatesInStore(store, None);
        while !ctx.is_null() {
            let record = &*ctx;
            if (record.dwCertEncodingType.0 & X509_ASN_ENCODING.0) != 0
                && !record.pbCertEncoded.is_null()
                && record.cbCertEncoded > 0
            {
                let bytes = std::slice::from_raw_parts(
                    record.pbCertEncoded,
                    record.cbCertEncoded as usize,
                );
                certs.push(bytes.to_vec());
            }
            ctx = CertEnumCertificatesInStore(store, Some(ctx));
        }
        let _ = CertCloseStore(store, 0);
    }
    certs
}

#[cfg(not(windows))]
fn load_windows_root_certs_der() -> Vec<Vec<u8>> {
    Vec::new()
}

/// Builds a `reqwest::Client` that trusts both bundled WebPKI roots and
/// certificates installed in the Windows OS `ROOT` store (e.g. corporate CAs).
pub fn build_http_client(timeout: Duration) -> reqwest::Client {
    let mut builder = reqwest::Client::builder().timeout(timeout);
    let root_ders = WINDOWS_ROOT_CERTS.get_or_init(load_windows_root_certs_der);
    for der in root_ders {
        if let Ok(cert) = reqwest::Certificate::from_der(der) {
            builder = builder.add_root_certificate(cert);
        }
    }
    builder.build().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_http_client_loads_windows_roots_without_panicking() {
        let client = build_http_client(Duration::from_secs(5));
        let _ = client;
        #[cfg(windows)]
        {
            let ders = WINDOWS_ROOT_CERTS.get_or_init(load_windows_root_certs_der);
            assert!(!ders.is_empty(), "Windows ROOT store should contain trusted CAs");
        }
    }
}
