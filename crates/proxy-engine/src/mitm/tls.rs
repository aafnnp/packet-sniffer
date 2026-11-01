//! 双向 TLS 握手：客户端侧按 SNI 动态签发证书，上游侧校验真实站点证书。

use std::path::Path;
use std::sync::Arc;

use rustls::crypto::CryptoProvider;
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use rustls_pki_types::CertificateDer;

use crate::mitm::ca::CertificateAuthority;
use crate::EngineError;

/// 客户端只支持 HTTP/1.1，ALPN 固定为 `http/1.1`。
const ALPN_HTTP1: &[u8] = b"http/1.1";

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// 按 SNI 动态签发证书的解析器。
#[derive(Debug)]
struct DynamicCertResolver {
    ca: Arc<CertificateAuthority>,
}

impl ResolvesServerCert for DynamicCertResolver {
    fn resolve(&self, hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let name = hello.server_name()?;
        match self.ca.leaf_for(name) {
            Ok(certified) => Some(certified),
            Err(err) => {
                tracing::warn!(host = name, error = %err, "签发域名证书失败");
                None
            }
        }
    }
}

/// 面向浏览器的 TLS 服务端配置。
pub fn server_config(ca: Arc<CertificateAuthority>) -> Arc<ServerConfig> {
    let mut config = ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .expect("ring provider 一定支持默认协议版本")
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(DynamicCertResolver { ca }));
    config.alpn_protocols = vec![ALPN_HTTP1.to_vec()];
    Arc::new(config)
}

/// 读取 PEM 文件中的全部证书。
pub fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>, EngineError> {
    let pem = std::fs::read(path)?;
    let mut cursor = pem.as_slice();
    let mut certs = Vec::new();
    for cert in rustls_pemfile::certs(&mut cursor) {
        certs.push(cert.map_err(|err| {
            EngineError::Certificate(format!("解析 {} 失败：{err}", path.display()))
        })?);
    }
    if certs.is_empty() {
        return Err(EngineError::Certificate(format!(
            "{} 中没有可用的证书",
            path.display()
        )));
    }
    Ok(certs)
}

/// 面向上游站点的 TLS 客户端配置：Mozilla 根证书集合 + 可选的额外信任根。
pub fn client_config(extra_roots: &[CertificateDer<'static>]) -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    for cert in extra_roots {
        if let Err(err) = roots.add(cert.clone()) {
            tracing::warn!(error = %err, "忽略无法加入信任库的上游根证书");
        }
    }

    let mut config = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .expect("ring provider 一定支持默认协议版本")
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![ALPN_HTTP1.to_vec()];
    Arc::new(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn builds_server_and_client_configs() {
        let dir = temp_dir();
        let ca = Arc::new(CertificateAuthority::load_or_create(&dir, "Test CA").unwrap());

        let server = server_config(ca);
        assert_eq!(server.alpn_protocols, vec![b"http/1.1".to_vec()]);

        let client = client_config(&[]);
        assert!(!client.alpn_protocols.is_empty());

        fs::remove_dir_all(&dir).ok();
    }

    fn temp_dir() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("packet-sniffer-tls-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }
}
