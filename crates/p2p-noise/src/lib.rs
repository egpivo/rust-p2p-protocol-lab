use snow::{Builder, TransportState};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const NOISE_PATTERN: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";

pub fn generate_keypair() -> ([u8; 32], [u8; 32]) {
    let builder = Builder::new(NOISE_PATTERN.parse().unwrap());
    let keypair = builder.generate_keypair().unwrap();
    let private: [u8; 32] = keypair.private.try_into().unwrap();
    let public: [u8; 32] = keypair.public.try_into().unwrap();
    (private, public)
}

pub struct NoiseTransport {
    state: TransportState,
}

impl NoiseTransport {
    pub fn new(state: TransportState) -> Self {
        Self { state }
    }

    pub fn send(&mut self, plaintext: &[u8], buf: &mut [u8]) -> usize {
        self.state.write_message(plaintext, buf).unwrap()
    }

    pub fn recv<'a>(&mut self, ciphertext: &[u8], buf: &'a mut [u8]) -> &'a [u8] {
        let len = self.state.read_message(ciphertext, buf).unwrap();
        &buf[..len]
    }
    pub fn remote_static(&self) -> Option<&[u8]> {
        self.state.get_remote_static()
    }
}

pub async fn handshake_initiator<S>(
    stream: &mut S,
    static_private: &[u8],
) -> Result<NoiseTransport, Box<dyn std::error::Error + Send + Sync>>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let builder = snow::Builder::new(NOISE_PATTERN.parse()?);
    let mut noise = builder
        .local_private_key(static_private)
        .build_initiator()?;

    let mut buf = vec![0u8; 65535];
    let mut msg = vec![0u8; 65535];

    // -> e
    let len = noise.write_message(&[], &mut buf)?;
    stream.write_all(&(len as u16).to_be_bytes()).await?;
    stream.write_all(&buf[..len]).await?;

    // <- e, ee, s, ss
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).await?;
    let len = u16::from_be_bytes(len_buf) as usize;
    stream.read_exact(&mut msg[..len]).await?;
    noise.read_message(&msg[..len], &mut buf)?;

    // -> s, se
    let len = noise.write_message(&[], &mut buf)?;
    stream.write_all(&(len as u16).to_be_bytes()).await?;
    stream.write_all(&buf[..len]).await?;

    Ok(NoiseTransport::new(noise.into_transport_mode()?))
}

pub async fn handshake_responder<S>(
    stream: &mut S,
    static_private: &[u8],
) -> Result<NoiseTransport, Box<dyn std::error::Error + Send + Sync>>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let builder = snow::Builder::new(NOISE_PATTERN.parse()?);
    let mut noise = builder
        .local_private_key(static_private)
        .build_responder()?;

    let mut buf = vec![0u8; 65535];
    let mut msg = vec![0u8; 65535];

    // <- e
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).await?;
    let len = u16::from_be_bytes(len_buf) as usize;
    stream.read_exact(&mut msg[..len]).await?;
    noise.read_message(&msg[..len], &mut buf)?;

    // -> e, ee, s, es
    let len = noise.write_message(&[], &mut buf)?;
    stream.write_all(&(len as u16).to_be_bytes()).await?;
    stream.write_all(&buf[..len]).await?;

    // <- s, se
    stream.read_exact(&mut len_buf).await?;
    let len = u16::from_be_bytes(len_buf) as usize;
    stream.read_exact(&mut msg[..len]).await.ok();
    noise.read_message(&msg[..len], &mut buf)?;

    Ok(NoiseTransport::new(noise.into_transport_mode()?))
}
