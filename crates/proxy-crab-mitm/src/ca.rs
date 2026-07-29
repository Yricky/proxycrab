use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, PKCS_ECDSA_P256_SHA256,
};
use rustls::{
    ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
};
use time::{Duration, OffsetDateTime};

const MAX_SERVER_CERTIFICATES: usize = 256;

pub struct CertificateAuthority {
    certificate_path: PathBuf,
    certificate_pem: String,
    issuer: Issuer<'static, KeyPair>,
    server_configs: Mutex<HashMap<String, Arc<ServerConfig>>>,
}

impl CertificateAuthority {
    pub fn load_or_generate(workspace: &Path) -> Result<Self> {
        let certificate_path = workspace.join("proxy-crab-ca.crt");
        let key_path = workspace.join("proxy-crab-ca.key");
        match Self::load(&certificate_path, &key_path) {
            Ok(authority) => Ok(authority),
            Err(error) => {
                tracing::warn!("CA missing or invalid; generating a new CA: {error}");
                Self::generate(&certificate_path, &key_path)
            }
        }
    }

    pub fn regenerate(workspace: &Path) -> Result<Self> {
        Self::generate(
            &workspace.join("proxy-crab-ca.crt"),
            &workspace.join("proxy-crab-ca.key"),
        )
    }

    pub fn certificate_pem(&self) -> &str {
        &self.certificate_pem
    }

    pub fn certificate_path(&self) -> &Path {
        &self.certificate_path
    }

    pub fn server_config(&self, host: &str) -> Result<Arc<ServerConfig>> {
        let mut cache = self
            .server_configs
            .lock()
            .expect("certificate cache lock poisoned");
        if let Some(config) = cache.get(host).cloned() {
            return Ok(config);
        }

        let now = OffsetDateTime::now_utc();
        let mut params = CertificateParams::new(vec![host.to_string()])?;
        params.distinguished_name.push(DnType::CommonName, host);
        params.key_usages.push(KeyUsagePurpose::DigitalSignature);
        params
            .extended_key_usages
            .push(ExtendedKeyUsagePurpose::ServerAuth);
        params.not_before = now - Duration::days(1);
        params.not_after = now + Duration::days(397);
        params.use_authority_key_identifier_extension = true;

        let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;
        let certificate = params.signed_by(&key, &self.issuer)?;
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(certificate.der().to_vec())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
            )?;
        let mut config = config;
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        let config = Arc::new(config);
        if cache.len() >= MAX_SERVER_CERTIFICATES {
            cache.clear();
        }
        cache.insert(host.to_string(), config.clone());
        Ok(config)
    }

    fn load(certificate_path: &Path, key_path: &Path) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(key_path, fs::Permissions::from_mode(0o600))?;
        }
        let certificate_pem = fs::read_to_string(certificate_path)?;
        let key_pem = fs::read_to_string(key_path)?;
        let key = KeyPair::from_pem(&key_pem)?;
        let issuer = Issuer::from_ca_cert_pem(&certificate_pem, key)?;
        Ok(Self {
            certificate_path: certificate_path.to_path_buf(),
            certificate_pem,
            issuer,
            server_configs: Mutex::new(HashMap::new()),
        })
    }

    fn generate(certificate_path: &Path, key_path: &Path) -> Result<Self> {
        let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;
        let mut params = CertificateParams::new(Vec::<String>::new())?;
        params
            .distinguished_name
            .push(DnType::CommonName, "ProxyCrab Root CA");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::CrlSign,
        ];
        let now = OffsetDateTime::now_utc();
        params.not_before = now - Duration::days(1);
        params.not_after = now + Duration::days(3650);
        let certificate = params.self_signed(&key)?;
        let certificate_pem = certificate.pem();
        let key_pem = key.serialize_pem();
        atomic_write(certificate_path, certificate_pem.as_bytes(), false)?;
        atomic_write(key_path, key_pem.as_bytes(), true)?;
        Self::load(certificate_path, key_path).context("generated CA could not be reloaded")
    }
}

fn atomic_write(path: &Path, content: &[u8], private: bool) -> Result<()> {
    let temporary = path.with_extension("tmp");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if private { 0o600 } else { 0o644 });
    }
    let mut file = options.open(&temporary)?;
    file.write_all(content)?;
    file.sync_all()?;
    if private {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
    }
    drop(file);
    fs::rename(temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::CertificateAuthority;

    #[test]
    fn generated_ca_can_be_reloaded_and_sign_hosts() {
        let root = tempdir().unwrap();
        let authority = CertificateAuthority::load_or_generate(root.path()).unwrap();
        assert!(authority.certificate_pem().contains("BEGIN CERTIFICATE"));
        authority.server_config("example.com").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(root.path().join("proxy-crab-ca.key"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0);
        }

        let reloaded = CertificateAuthority::load_or_generate(root.path()).unwrap();
        reloaded.server_config("api.example.com").unwrap();
    }
}
