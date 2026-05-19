use tokio::io::{AsyncRead,AsyncWrite, AsyncReadExt,AsyncWriteExt, BufReader, AsyncBufReadExt};
use tokio::net::TcpStream;
use framed_core::{MAX_PAYLOAD, encode_frame};
use std::io::{Result, Error, ErrorKind};
use rustls::ClientConfig;
use rustls::client::danger::{ServerCertVerifier, HandshakeSignatureValid, ServerCertVerified};
use rustls::pki_types::{ServerName, CertificateDer, UnixTime};
use rustls::DigitallySignedStruct;
use tokio_rustls::TlsConnector;
use std::sync::Arc;
use sha2::{Sha256, Digest};


#[derive(Debug)]
struct PinnedVerifier {
    expected_hash: Vec<u8>,
}

impl ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(&self, end_entity: &CertificateDer, _: &[CertificateDer], _: &ServerName, _: &[u8], _: UnixTime) -> std::result::Result<ServerCertVerified, rustls::Error> {
        let hash = Sha256::digest(end_entity.as_ref());
        if hash.as_slice() == self.expected_hash.as_slice() {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("cert hash mismatch".into()))
        }
    }

    fn verify_tls12_signature(&self, _: &[u8], _: &CertificateDer, _: &DigitallySignedStruct) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(&self, _: &[u8], _: &CertificateDer, _: &DigitallySignedStruct) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::aws_lc_rs::default_provider().signature_verification_algorithms.supported_schemes()
    }
}

fn make_tls_connector(expected_hash: Vec<u8>) -> TlsConnector {
    let config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedVerifier { expected_hash }))
        .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

async fn read_frames<S>(stream: &mut S) -> Result<(Vec<u8>)> 
where
    S: AsyncRead + Unpin,
{
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let payload_len = u32::from_be_bytes(len_buf);
    if payload_len > MAX_PAYLOAD {
        return Err(Error::new(
             ErrorKind::InvalidData,
            format!("frame too large: {payload_len} bytes (max {MAX_PAYLOAD})"),
        ));
    }
    let mut body = vec![0u8; payload_len as usize];
    stream.read_exact(&mut body).await?;
    Ok(body)
}


#[tokio::main]
async fn main() -> std::io::Result<()> {
    // args[1] = name, args[2] = message (optional)
    let args: Vec<String> = std::env::args().collect();
    let name = &args[1];
    let message = args.get(2);

    let stream = TcpStream::connect("127.0.0.1:7004").await?;
    let hash_hex = std::fs::read_to_string("cert.hash").unwrap();
    let expected_hash = hex::decode(hash_hex.trim()).expect("cert.hash should be hex-encoded");
    let connector = make_tls_connector(expected_hash);
    let server_name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
    let mut stream = connector.connect(server_name, stream).await?;
    let frame = encode_frame(format!("JOIN {name}").as_bytes());
    stream.write_all(&frame).await?;
    let resp = read_frames(&mut stream).await?;
    println!("JOIN: {}", String::from_utf8_lossy(&resp));
    if let Some(msg) = message {
        stream.write_all(&encode_frame(format!("SAY {msg}").as_bytes())).await?;
        let resp = read_frames(&mut stream).await?;
        println!("SAY: {}", String::from_utf8_lossy(&resp));
        return Ok(());
    }

    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();

    loop {
        tokio::select!{
            body = read_frames(&mut stream) =>{
                let body = match body {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("failed to read frame from server");
                        break Ok(());
                    }
                };
                println!("recv: {}", String::from_utf8_lossy(&body));
                stream.write_all(&encode_frame(b"ACK")).await?;
            }
            line = lines.next_line() => {
                let line = match line {
                    Ok(Some(l)) => l,
                    _ => break Ok(()),
                };
                stream.write_all(&encode_frame(format!("SAY {line}").as_bytes())).await?;
            }
        }
    }

}
