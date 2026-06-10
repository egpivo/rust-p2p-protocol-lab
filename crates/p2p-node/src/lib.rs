use p2p_core::{Message, NodeId};
use std::collections::HashSet;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use tokio_rustls::rustls::{DigitallySignedStruct, Error as TlsError, SignatureScheme};

pub type PeerList = Arc<Mutex<Vec<SocketAddr>>>;
pub type BlockList = Arc<Mutex<HashSet<IpAddr>>>;
pub const MAX_PEERS: usize = 8;

#[derive(Debug)]
pub struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer,
        _intermediates: &[CertificateDer],
        _server_name: &ServerName,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _: &[u8],
        _: &CertificateDer,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ED25519,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PSS_SHA256,
        ]
    }
}

pub async fn send_msg<W>(writer: &mut W, msg: &Message) -> std::io::Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    let mut line = serde_json::to_string(msg).unwrap();
    line.push('\n');
    writer.write_all(line.as_bytes()).await
}

pub async fn handle_inbound<S>(
    stream: S,
    peer_addr: SocketAddr,
    node_id: NodeId,
    port: u16,
    peers: PeerList,
    blocklist: BlockList,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    if blocklist.lock().unwrap().contains(&peer_addr.ip()) {
        return;
    }
    // receive their Hello first
    let (read_half, mut write_half) = tokio::io::split(stream);
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
        return;
    }

    let remote_id = match serde_json::from_str::<Message>(line.trim()) {
        Ok(Message::Hello {
            node_id: rid,
            listen_addr,
            peers: their_peers,
        }) => {
            let mut p = peers.lock().unwrap();
            if listen_addr.port() != 0 && p.len() < MAX_PEERS && !p.contains(&listen_addr) {
                p.push(listen_addr);
            }
            for addr in their_peers {
                if p.len() < MAX_PEERS && !p.contains(&addr) {
                    p.push(addr);
                }
            }
            rid
        }
        _ => return,
    };

    // reply with our Hello
    let known = peers.lock().unwrap().clone();
    let reply = Message::Hello {
        node_id,
        listen_addr: format!("127.0.0.1:{port}").parse().unwrap(),
        peers: known,
    };
    let mut reply_line = serde_json::to_string(&reply).unwrap();
    reply_line.push('\n');
    if write_half.write_all(reply_line.as_bytes()).await.is_err() {
        return;
    }

    println!(
        "[{}] <- handshake from {:?} at {}",
        node_id.0, remote_id, peer_addr
    );

    // keep connection alive. handle Ping/Pong/GetPeers
    loop {
        line.clear();
        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
            break;
        }
        match serde_json::from_str::<Message>(line.trim()) {
            Ok(Message::Ping) => {
                let mut pong = serde_json::to_string(&Message::Pong).unwrap();
                pong.push('\n');
                if write_half.write_all(pong.as_bytes()).await.is_err() {
                    break;
                }
            }
            Ok(Message::GetPeers) => {
                let known = peers.lock().unwrap().clone();
                let mut msg = serde_json::to_string(&Message::Peers(known)).unwrap();
                msg.push('\n');
                if write_half.write_all(msg.as_bytes()).await.is_err() {
                    break;
                }
            }
            Ok(Message::Tip { height, hash }) => {
                println!("[{}] tip: height={} hash={}", node_id.0, height, hash);
            }
            _ => {}
        }
    }

    println!("[{}] connection closed from {}", node_id.0, peer_addr);
}

pub fn make_tls_config() -> (ServerConfig, CertificateDer<'static>, Vec<u8>) {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    let key_der = PrivateKeyDer::try_from(cert.key_pair.serialize_der()).unwrap();

    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der.clone()], key_der)
        .unwrap();

    let cert_bytes = cert_der.to_vec();
    (server_config, cert_der, cert_bytes)
}
