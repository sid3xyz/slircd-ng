use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair,
};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct TlsTestPaths {
    pub ca_path: PathBuf,
    pub server_cert_path: PathBuf,
    pub server_key_path: PathBuf,
    pub client_cert_path: PathBuf,
    pub client_key_path: PathBuf,
    pub server_name: String,
}

#[derive(Clone, Debug)]
pub struct TlsClientConfig {
    pub ca_path: PathBuf,
    pub server_name: String,
    pub client_cert_path: Option<PathBuf>,
    pub client_key_path: Option<PathBuf>,
}

impl TlsClientConfig {
    pub fn with_client_cert(paths: &TlsTestPaths) -> Self {
        Self {
            ca_path: paths.ca_path.clone(),
            server_name: paths.server_name.clone(),
            client_cert_path: Some(paths.client_cert_path.clone()),
            client_key_path: Some(paths.client_key_path.clone()),
        }
    }

    pub fn without_client_cert(paths: &TlsTestPaths) -> Self {
        Self {
            ca_path: paths.ca_path.clone(),
            server_name: paths.server_name.clone(),
            client_cert_path: None,
            client_key_path: None,
        }
    }
}

pub fn generate_tls_assets(dir: &Path) -> anyhow::Result<TlsTestPaths> {
    std::fs::create_dir_all(dir)?;

    let (ca_cert, ca_key) = build_ca()?;
    let (server_cert, server_key) = build_server(&ca_cert, &ca_key)?;
    let (client_cert, client_key) = build_client(&ca_cert, &ca_key)?;

    let ca_path = dir.join("ca.pem");
    let server_cert_path = dir.join("server.pem");
    let server_key_path = dir.join("server.key");
    let client_cert_path = dir.join("client.pem");
    let client_key_path = dir.join("client.key");

    std::fs::write(&ca_path, ca_cert.pem())?;
    std::fs::write(&server_cert_path, server_cert.pem())?;
    std::fs::write(&server_key_path, server_key.serialize_pem())?;
    std::fs::write(&client_cert_path, client_cert.pem())?;
    std::fs::write(&client_key_path, client_key.serialize_pem())?;

    Ok(TlsTestPaths {
        ca_path,
        server_cert_path,
        server_key_path,
        client_cert_path,
        client_key_path,
        server_name: "localhost".to_string(),
    })
}

fn build_ca() -> anyhow::Result<(Certificate, KeyPair)> {
    let mut params = CertificateParams::default();
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, "slircd-test-ca");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;
    Ok((cert, key_pair))
}

fn build_server(ca_cert: &Certificate, ca_key: &KeyPair) -> anyhow::Result<(Certificate, KeyPair)> {
    let mut params =
        CertificateParams::new(vec!["localhost".to_string(), "127.0.0.1".to_string()])?;
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, "localhost");
    params.is_ca = IsCa::NoCa;
    let key_pair = KeyPair::generate()?;
    let cert = params.signed_by(&key_pair, ca_cert, ca_key)?;
    Ok((cert, key_pair))
}

fn build_client(ca_cert: &Certificate, ca_key: &KeyPair) -> anyhow::Result<(Certificate, KeyPair)> {
    let mut params = CertificateParams::new(Vec::<String>::new())?;
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, "slircd-test-client");
    params.is_ca = IsCa::NoCa;
    let key_pair = KeyPair::generate()?;
    let cert = params.signed_by(&key_pair, ca_cert, ca_key)?;
    Ok((cert, key_pair))
}
