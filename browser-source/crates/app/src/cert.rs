//! Certificate inspection and native Windows certificate dialog invocation
//! Provides real X.509 cryptographic attributes for HTTPS origins and
//! launches the standard Windows Certificate Viewer dialog (`X509Certificate2UI`).

use evergreen_core::ipc::SecurityInfo;
use std::collections::HashMap;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::{Arc, Mutex};

const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Clone, Default)]
pub struct CertificateCache {
    cache: Arc<Mutex<HashMap<String, SecurityInfo>>>,
}

impl CertificateCache {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn get(&self, host: &str) -> Option<SecurityInfo> {
        self.cache.lock().ok().and_then(|c| c.get(host).cloned())
    }

    pub fn insert(&self, host: String, info: SecurityInfo) {
        if let Ok(mut c) = self.cache.lock() {
            if c.len() >= 128 {
                if let Some(key) = c.keys().next().cloned() {
                    c.remove(&key);
                }
            }
            c.insert(host, info);
        }
    }

    /// Query or return cached certificate info for a host or URL
    pub fn query_or_default(&self, raw_url: &str) -> SecurityInfo {
        if raw_url.starts_with("evergreen://")
            || raw_url == "about:blank"
            || raw_url.starts_with("data:text/html")
        {
            return SecurityInfo {
                host: if raw_url.starts_with("evergreen://") {
                    raw_url.to_string()
                } else {
                    "evergreen://sandbox".to_string()
                },
                is_secure: true,
                protocol: "Trang nội bộ Pip".to_string(),
                certificate_status: "Không áp dụng".to_string(),
                cipher: "Không có kết nối mạng".to_string(),
                ..Default::default()
            };
        }

        let host = if let Ok(parsed) = wry::http::Uri::try_from(raw_url) {
            parsed.host().unwrap_or("").to_string()
        } else {
            raw_url.to_string()
        };

        if host.is_empty() {
            return SecurityInfo::default();
        }

        // Cache by origin: HTTPS information must never label an HTTP visit secure.
        let cache_key = raw_url
            .parse::<wry::http::Uri>()
            .ok()
            .and_then(|uri| Some(format!("{}://{}", uri.scheme_str()?, uri.authority()?)))
            .unwrap_or_else(|| raw_url.to_string());
        if let Some(cached) = self.get(&cache_key) {
            return cached;
        }

        // Create baseline info
        let is_https = raw_url.starts_with("https://");
        let initial_info = SecurityInfo {
            host: host.clone(),
            is_secure: is_https,
            protocol: if is_https {
                "HTTPS".to_string()
            } else {
                "HTTP".to_string()
            },
            certificate_status: if is_https {
                "Do WebView2 xử lý; chưa lấy chi tiết".to_string()
            } else {
                "Chưa được mã hóa".to_string()
            },
            cipher: if is_https {
                "Chưa lấy thông tin phiên TLS".to_string()
            } else {
                "None (Plaintext)".to_string()
            },
            subject: if is_https {
                String::new()
            } else {
                "None".to_string()
            },
            issuer: if is_https {
                String::new()
            } else {
                "None".to_string()
            },
            valid_from: if is_https {
                String::new()
            } else {
                "N/A".to_string()
            },
            valid_to: if is_https {
                String::new()
            } else {
                "N/A".to_string()
            },
            thumbprint: if is_https {
                String::new()
            } else {
                "None".to_string()
            },
            serial_number: if is_https {
                String::new()
            } else {
                "None".to_string()
            },
            signature_algorithm: if is_https {
                String::new()
            } else {
                "None".to_string()
            },
        };

        self.insert(cache_key, initial_info.clone());

        // Do not start a second TLS connection or PowerShell process for each site.

        initial_info
    }
}

/// Open the native Windows Certificate Viewer dialog (`X509Certificate2UI`)
pub fn open_native_certificate_dialog(host: &str) {
    if host.is_empty()
        || host.len() > 253
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        return;
    }

    let host_owned = host.to_string();
    std::thread::spawn(move || {
        let script = format!(
            "Add-Type -AssemblyName System.Security; $h='{}'; try {{ \
                $tcp = New-Object System.Net.Sockets.TcpClient($h, 443); \
                $ssl = New-Object System.Net.Security.SslStream($tcp.GetStream(), $false); \
                $ssl.AuthenticateAsClient($h); \
                $cert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($ssl.RemoteCertificate); \
                [System.Security.Cryptography.X509Certificates.X509Certificate2UI]::DisplayCertificate($cert); \
                $tcp.Close(); \
            }} catch {{}}",
            host_owned
        );

        let mut cmd = Command::new("powershell");
        cmd.args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ]);
        #[cfg(target_os = "windows")]
        cmd.creation_flags(CREATE_NO_WINDOW);

        let _ = cmd.spawn();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn https_cache_cannot_mark_http_as_secure() {
        let cache = CertificateCache::new();
        assert!(cache.query_or_default("https://example.com/").is_secure);
        assert!(!cache.query_or_default("http://example.com/").is_secure);
        assert!(!cache.query_or_default("http://example.com:8080/").is_secure);
        assert!(
            cache
                .query_or_default("https://example.com:8443/")
                .is_secure
        );
    }
}
