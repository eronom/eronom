use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::OnceLock;

fn get_tls_config() -> &'static Arc<rustls::ClientConfig> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let mut root_store = rustls::RootCertStore::empty();
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

        let config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();
        Arc::new(config)
    })
}

pub fn fetch_tls(host: &str, request: &str, stream: &mut TcpStream) -> Result<Vec<u8>, String> {
    let config = get_tls_config();
    let server_name = host
        .to_string()
        .try_into()
        .map_err(|_| format!("Invalid server name for TLS: {}", host))?;

    let mut conn = rustls::ClientConnection::new(config.clone(), server_name)
        .map_err(|e| format!("TLS Connection error: {}", e))?;

    let mut tls_stream = rustls::Stream::new(&mut conn, stream);
    tls_stream.write_all(request.as_bytes()).map_err(|e| e.to_string())?;

    let mut response_bytes = Vec::new();
    tls_stream.read_to_end(&mut response_bytes).map_err(|e| e.to_string())?;
    Ok(response_bytes)
}
