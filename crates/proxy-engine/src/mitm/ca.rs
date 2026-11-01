//! 根证书管理与动态域名签发。
//!
//! 首次启动时生成一对根证书 / 私钥并保存到 `ca_dir`，之后按 SNI 动态签发叶子证书，
//! 叶子证书带 SAN 字段，保证浏览器校验通过（前提是根证书已被系统信任）。

use std::collections::HashMap;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::{Mutex, MutexGuard};

use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose, SanType,
};
use rustls::sign::CertifiedKey;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

use crate::EngineError;

/// 叶子证书缓存上限，避免长时间抓包后内存膨胀。
const MAX_CACHED_LEAVES: usize = 512;

pub struct CertificateAuthority {
    cert: Certificate,
    key: KeyPair,
    cert_path: PathBuf,
    key_path: PathBuf,
    leaves: Mutex<HashMap<String, Arc<CertifiedKey>>>,
}

impl std::fmt::Debug for CertificateAuthority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CertificateAuthority")
            .field("cert_path", &self.cert_path)
            .finish_non_exhaustive()
    }
}

impl CertificateAuthority {
    /// 加载已有根证书；不存在时生成并落盘。
    pub fn load_or_create(dir: &Path, common_name: &str) -> Result<Self, EngineError> {
        fs::create_dir_all(dir)?;
        let cert_path = dir.join("root-ca.pem");
        let key_path = dir.join("root-ca.key");

        if cert_path.exists() && key_path.exists() {
            let cert_pem = fs::read_to_string(&cert_path)?;
            let key_pem = fs::read_to_string(&key_path)?;
            let params = CertificateParams::from_ca_cert_pem(&cert_pem)
                .map_err(|err| EngineError::Certificate(err.to_string()))?;
            let key = KeyPair::from_pem(&key_pem)
                .map_err(|err| EngineError::Certificate(err.to_string()))?;
            let cert = params
                .self_signed(&key)
                .map_err(|err| EngineError::Certificate(err.to_string()))?;

            return Ok(Self {
                cert,
                key,
                cert_path,
                key_path,
                leaves: Mutex::new(HashMap::new()),
            });
        }

        let key = KeyPair::generate().map_err(|err| EngineError::Certificate(err.to_string()))?;
        let mut params = CertificateParams::new(Vec::new())
            .map_err(|err| EngineError::Certificate(err.to_string()))?;
        params.distinguished_name = DistinguishedName::new();
        params
            .distinguished_name
            .push(DnType::CommonName, common_name.to_string());
        params
            .distinguished_name
            .push(DnType::OrganizationName, "Packet Sniffer");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];

        let cert = params
            .self_signed(&key)
            .map_err(|err| EngineError::Certificate(err.to_string()))?;

        fs::write(&cert_path, cert.pem())?;
        fs::write(&key_path, key.serialize_pem())?;
        tracing::info!(path = %cert_path.display(), "已生成根证书");

        Ok(Self {
            cert,
            key,
            cert_path,
            key_path,
            leaves: Mutex::new(HashMap::new()),
        })
    }

    pub fn cert_path(&self) -> &Path {
        &self.cert_path
    }

    pub fn key_path(&self) -> &Path {
        &self.key_path
    }

    pub fn cert_pem(&self) -> String {
        self.cert.pem()
    }

    /// 为指定域名（或 IP）签发叶子证书，结果带缓存。
    pub fn leaf_for(&self, host: &str) -> Result<Arc<CertifiedKey>, EngineError> {
        let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
        if host.is_empty() {
            return Err(EngineError::InvalidServerName(host));
        }

        if let Some(cached) = self.leaves().get(&host) {
            return Ok(cached.clone());
        }

        let leaf_key =
            KeyPair::generate().map_err(|err| EngineError::Certificate(err.to_string()))?;
        let mut params = CertificateParams::new(vec![host.clone()])
            .map_err(|err| EngineError::Certificate(err.to_string()))?;
        if let Ok(ip) = host.parse::<IpAddr>() {
            params.subject_alt_names = vec![SanType::IpAddress(ip)];
        }
        params.distinguished_name = DistinguishedName::new();
        params
            .distinguished_name
            .push(DnType::CommonName, host.clone());
        params.is_ca = IsCa::ExplicitNoCa;
        params.use_authority_key_identifier_extension = true;
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
        ];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];

        let leaf_cert = params
            .signed_by(&leaf_key, &self.cert, &self.key)
            .map_err(|err| EngineError::Certificate(err.to_string()))?;

        let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der()));
        let signing_key = rustls::crypto::ring::sign::any_supported_type(&key_der)
            .map_err(|err| EngineError::Certificate(err.to_string()))?;

        // 只回传叶子证书：根证书由用户手动安装到信任存储，重复下发反而会被部分客户端拒绝。
        let chain: Vec<CertificateDer<'static>> = vec![leaf_cert.der().clone()];
        let certified = Arc::new(CertifiedKey::new(chain, signing_key));

        let mut leaves = self.leaves();
        if leaves.len() >= MAX_CACHED_LEAVES {
            leaves.clear();
        }
        leaves.insert(host, certified.clone());

        Ok(certified)
    }

    /// 根证书安装提示，供 UI 引导用户手动信任。
    pub fn install_hint(&self) -> String {
        format!(
            "请将根证书安装到系统信任存储后重启浏览器：{}",
            self.cert_path.display()
        )
    }

    fn leaves(&self) -> MutexGuard<'_, HashMap<String, Arc<CertifiedKey>>> {
        self.leaves.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "packet-sniffer-ca-test-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn creates_and_reloads_ca() {
        let dir = temp_dir("reload");
        let ca = CertificateAuthority::load_or_create(&dir, "Test CA").unwrap();
        assert!(ca.cert_path().exists());
        assert!(ca.key_path().exists());
        assert!(ca.cert_pem().contains("BEGIN CERTIFICATE"));

        let saved = fs::read_to_string(ca.cert_path()).unwrap();
        let reloaded = CertificateAuthority::load_or_create(&dir, "Test CA").unwrap();
        // 重新加载不应该重新生成或覆盖已安装的根证书。
        assert_eq!(fs::read_to_string(reloaded.cert_path()).unwrap(), saved);
        assert_eq!(reloaded.leaf_for("example.com").unwrap().cert.len(), 1);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn signs_leaf_for_host_and_caches() {
        let dir = temp_dir("leaf");
        let ca = CertificateAuthority::load_or_create(&dir, "Test CA").unwrap();

        let first = ca.leaf_for("example.com").unwrap();
        let second = ca.leaf_for("Example.COM.").unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.cert.len(), 1);

        let ip = ca.leaf_for("127.0.0.1").unwrap();
        assert_eq!(ip.cert.len(), 1);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rejects_empty_host() {
        let dir = temp_dir("empty");
        let ca = CertificateAuthority::load_or_create(&dir, "Test CA").unwrap();
        assert!(ca.leaf_for("  ").is_err());
        fs::remove_dir_all(&dir).ok();
    }
}
