use snow::{Builder, HandshakeState, TransportState};
use zeroize::Zeroizing;

pub const PATTERN: &str = "Noise_XX_25519_ChaChaPoly_SHA256";
pub const MAX_FRAME: usize = 65_535;
pub const FRAGMENT: usize = 60_000;
pub const MAX_MESSAGE: usize = 2 * 1024 * 1024 + 16 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("encrypted channel rejected")]
    Noise(#[from] snow::Error),
    #[error("invalid encrypted channel frame")]
    Frame,
    #[error("host identity mismatch; pair this device again locally")]
    Identity,
}

pub struct Identity {
    pub private: Zeroizing<Vec<u8>>,
    pub public: Vec<u8>,
}

pub fn identity() -> Result<Identity, Error> {
    let key = Builder::new(PATTERN.parse()?).generate_keypair()?;
    Ok(Identity {
        private: Zeroizing::new(key.private),
        public: key.public,
    })
}

pub struct Handshake {
    state: HandshakeState,
    expected_host: Option<[u8; 32]>,
    host_verified: bool,
}

impl Handshake {
    pub fn new(
        private: &[u8],
        expected_host: Option<[u8; 32]>,
        host_id: &str,
        channel_id: &str,
    ) -> Result<Self, Error> {
        if private.len() != 32
            || host_id.is_empty()
            || host_id.len() > 128
            || channel_id.is_empty()
            || channel_id.len() > 128
        {
            return Err(Error::Frame);
        }
        let prologue = format!("fabrials.remote.v1\0{host_id}\0{channel_id}");
        let builder = Builder::new(PATTERN.parse()?)
            .local_private_key(private)?
            .prologue(prologue.as_bytes())?;
        let state = if expected_host.is_some() {
            builder.build_initiator()?
        } else {
            builder.build_responder()?
        };
        Ok(Self {
            state,
            expected_host,
            host_verified: false,
        })
    }

    pub fn write(&mut self, payload: &[u8]) -> Result<Vec<u8>, Error> {
        if payload.len() > 4096
            || (self.expected_host.is_some() && !self.host_verified && !payload.is_empty())
        {
            return Err(Error::Frame);
        }
        let mut output = vec![0; MAX_FRAME];
        let n = self.state.write_message(payload, &mut output)?;
        output.truncate(n);
        Ok(output)
    }

    pub fn read(&mut self, frame: &[u8]) -> Result<Zeroizing<Vec<u8>>, Error> {
        if frame.len() > MAX_FRAME {
            return Err(Error::Frame);
        }
        let mut output = Zeroizing::new(vec![0; MAX_FRAME]);
        let n = self.state.read_message(frame, &mut output)?;
        if let Some(expected) = self.expected_host {
            if self.state.get_remote_static() != Some(expected.as_slice()) {
                return Err(Error::Identity);
            }
            self.host_verified = true;
        }
        if n > 4096 {
            return Err(Error::Frame);
        }
        output.truncate(n);
        Ok(output)
    }

    pub fn remote_static(&self) -> Option<[u8; 32]> {
        self.state.get_remote_static()?.try_into().ok()
    }

    pub fn finish(self) -> Result<Channel, Error> {
        if self.expected_host.is_some() && !self.host_verified {
            return Err(Error::Identity);
        }
        Ok(Channel {
            state: self.state.into_transport_mode()?,
            receive: Vec::new(),
            expected_size: None,
        })
    }
}

pub struct Channel {
    state: TransportState,
    receive: Vec<u8>,
    expected_size: Option<usize>,
}

impl Channel {
    pub fn encrypt(&mut self, message: &[u8]) -> Result<Vec<Vec<u8>>, Error> {
        if message.is_empty() || message.len() > MAX_MESSAGE {
            return Err(Error::Frame);
        }
        let mut frames = Vec::with_capacity(message.len().div_ceil(FRAGMENT));
        for (index, fragment) in message.chunks(FRAGMENT).enumerate() {
            let mut plaintext = Zeroizing::new(Vec::with_capacity(fragment.len() + 8));
            plaintext.extend_from_slice(&(message.len() as u32).to_be_bytes());
            plaintext.extend_from_slice(&((index * FRAGMENT) as u32).to_be_bytes());
            plaintext.extend_from_slice(fragment);
            let mut frame = vec![0; plaintext.len() + 16];
            let n = self.state.write_message(&plaintext, &mut frame)?;
            frame.truncate(n);
            frames.push(frame);
        }
        Ok(frames)
    }

    pub fn decrypt(&mut self, frame: &[u8]) -> Result<Option<Zeroizing<Vec<u8>>>, Error> {
        if frame.len() > MAX_FRAME || frame.len() < 25 {
            return Err(Error::Frame);
        }
        let mut plaintext = Zeroizing::new(vec![0; MAX_FRAME]);
        let n = self.state.read_message(frame, &mut plaintext)?;
        if n <= 8 {
            return Err(Error::Frame);
        }
        let size =
            u32::from_be_bytes(plaintext[..4].try_into().map_err(|_| Error::Frame)?) as usize;
        let offset =
            u32::from_be_bytes(plaintext[4..8].try_into().map_err(|_| Error::Frame)?) as usize;
        let body = &plaintext[8..n];
        if size == 0
            || size > MAX_MESSAGE
            || offset != self.receive.len()
            || offset + body.len() > size
            || body.len() > FRAGMENT
            || self.expected_size.is_some_and(|expected| expected != size)
            || (offset + body.len() < size && body.len() != FRAGMENT)
        {
            return Err(Error::Frame);
        }
        self.expected_size = Some(size);
        self.receive.extend_from_slice(body);
        if self.receive.len() != size {
            return Ok(None);
        }
        self.expected_size = None;
        Ok(Some(Zeroizing::new(std::mem::take(&mut self.receive))))
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm;

#[cfg(test)]
mod tests {
    use super::*;

    fn channels() -> (Channel, Channel) {
        let host = identity().unwrap();
        let device = identity().unwrap();
        let mut a = Handshake::new(
            &device.private,
            Some(host.public.try_into().unwrap()),
            "host",
            "channel",
        )
        .unwrap();
        let mut b = Handshake::new(&host.private, None, "host", "channel").unwrap();
        assert!(b.read(&a.write(&[]).unwrap()).unwrap().is_empty());
        assert!(a.read(&b.write(&[]).unwrap()).unwrap().is_empty());
        assert_eq!(
            &*b.read(&a.write(b"pairing secret").unwrap()).unwrap(),
            b"pairing secret"
        );
        (a.finish().unwrap(), b.finish().unwrap())
    }

    #[test]
    fn fragmented_roundtrip_and_replay_rejected() {
        let (mut a, mut b) = channels();
        let payload = vec![42; 2 * 1024 * 1024];
        let frames = a.encrypt(&payload).unwrap();
        let mut result = None;
        for frame in &frames {
            result = b.decrypt(frame).unwrap();
        }
        assert_eq!(&*result.unwrap(), &payload);
        assert!(b.decrypt(&frames[0]).is_err());
    }

    #[test]
    fn mutation_and_out_of_order_rejected() {
        let (mut a, mut b) = channels();
        let frames = a.encrypt(&vec![7; FRAGMENT + 1]).unwrap();
        assert!(b.decrypt(&frames[1]).is_err());
        let (mut a, mut b) = channels();
        let mut frame = a.encrypt(b"command").unwrap().remove(0);
        frame[20] ^= 1;
        assert!(b.decrypt(&frame).is_err());
    }

    #[test]
    fn wrong_host_never_receives_pairing_secret() {
        let host = identity().unwrap();
        let device = identity().unwrap();
        let mut a = Handshake::new(&device.private, Some([9; 32]), "host", "channel").unwrap();
        let mut b = Handshake::new(&host.private, None, "host", "channel").unwrap();
        assert!(a.write(b"secret").is_err());
        b.read(&a.write(&[]).unwrap()).unwrap();
        assert!(matches!(
            a.read(&b.write(&[]).unwrap()),
            Err(Error::Identity)
        ));
        assert!(a.write(b"secret").is_err());
    }
}
