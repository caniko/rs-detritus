//! End-to-end TLS smoke test: aws-lc-rs `CryptoProvider` + reqwest's rustls
//! stack must complete a real TLS handshake against a tokio-rustls server we
//! spin up in-process.
#![allow(clippy::missing_docs_in_private_items)]

use std::sync::Arc;

use rcgen::generate_simple_self_signed;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        NamedGroup, ServerConfig,
        crypto::{self, CryptoProvider},
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    },
};

const RESPONSE_BODY: &str = "ok";

#[tokio::test(flavor = "multi_thread")]
async fn aws_lc_rs_provider_drives_real_tls_handshake() {
    detritus::install_default_crypto_provider();

    let provider = CryptoProvider::get_default()
        .expect("provider installed by install_default_crypto_provider");
    assert_provider_looks_like_aws_lc(provider);

    let cert = generate_simple_self_signed(vec!["localhost".to_owned()])
        .expect("generate self-signed localhost cert");
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    let key_der = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der()));

    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der.clone()], key_der)
        .expect("build rustls server config");

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind TLS listener");
    let addr = listener.local_addr().expect("TLS listener addr");
    let acceptor = TlsAcceptor::from(Arc::new(server_config));

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("accept TCP client");
        let mut tls = acceptor
            .accept(socket)
            .await
            .expect("complete TLS handshake");
        let mut request = [0_u8; 1024];
        let bytes_read = tls.read(&mut request).await.expect("read HTTP request");
        assert!(bytes_read > 0, "expected non-empty HTTP request");
        tls.write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{RESPONSE_BODY}",
                RESPONSE_BODY.len()
            )
            .as_bytes(),
        )
        .await
        .expect("write HTTP response");
        tls.shutdown().await.expect("shutdown TLS stream");
    });

    let client = reqwest::Client::builder()
        .add_root_certificate(
            reqwest::Certificate::from_der(cert_der.as_ref())
                .expect("build reqwest root certificate from test cert"),
        )
        .https_only(true)
        .build()
        .expect("build reqwest client");

    let response = client
        .get(format!("https://localhost:{}/", addr.port()))
        .send()
        .await
        .expect("HTTPS GET completes");

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response.text().await.expect("read response body"),
        RESPONSE_BODY
    );

    server.await.expect("TLS server task joins cleanly");
}

fn assert_provider_looks_like_aws_lc(provider: &'static Arc<CryptoProvider>) {
    let suite_debug = provider
        .cipher_suites
        .iter()
        .map(|suite| format!("{suite:?}"))
        .collect::<Vec<_>>();

    assert!(
        suite_debug
            .iter()
            .any(|suite| suite.contains("TLS13_AES_128_GCM_SHA256")),
        "expected TLS13_AES_128_GCM_SHA256 in provider cipher suites; got {suite_debug:?}"
    );

    assert!(
        provider_fingerprint(provider.as_ref())
            == provider_fingerprint(&crypto::aws_lc_rs::default_provider()),
        "installed provider did not match aws-lc-rs cipher suite fingerprint"
    );

    let group_names = kx_group_names(provider.as_ref());
    assert!(
        group_names.contains(&NamedGroup::X25519MLKEM768),
        "expected aws-lc-rs default KX groups to include X25519MLKEM768; got {group_names:?}"
    );
    assert_eq!(
        group_names,
        kx_group_names(&crypto::aws_lc_rs::default_provider()),
        "installed provider did not match aws-lc-rs key exchange group fingerprint"
    );
}

fn provider_fingerprint(provider: &CryptoProvider) -> Vec<String> {
    provider
        .cipher_suites
        .iter()
        .map(|suite| format!("{suite:?}"))
        .collect()
}

fn kx_group_names(provider: &CryptoProvider) -> Vec<NamedGroup> {
    provider
        .kx_groups
        .iter()
        .map(|group| group.name())
        .collect()
}
