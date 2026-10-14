use super::{Channel, Handshake};
use wasm_bindgen::prelude::*;

fn error(value: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&value.to_string())
}

#[wasm_bindgen]
pub fn generate_private_key() -> Result<Vec<u8>, JsValue> {
    Ok(super::identity().map_err(error)?.private.to_vec())
}

#[wasm_bindgen]
pub struct NoiseClient {
    handshake: Option<Handshake>,
    channel: Option<Channel>,
}

#[wasm_bindgen]
impl NoiseClient {
    #[wasm_bindgen(constructor)]
    pub fn new(
        private_key: &[u8],
        host_key: &[u8],
        host_id: &str,
        channel_id: &str,
    ) -> Result<Self, JsValue> {
        let expected: [u8; 32] = host_key.try_into().map_err(error)?;
        Ok(Self {
            handshake: Some(
                Handshake::new(private_key, Some(expected), host_id, channel_id).map_err(error)?,
            ),
            channel: None,
        })
    }

    pub fn start(&mut self) -> Result<Vec<u8>, JsValue> {
        self.handshake
            .as_mut()
            .ok_or_else(|| error("invalid handshake state"))?
            .write(&[])
            .map_err(error)
    }

    pub fn authenticate(&mut self, frame: &[u8], credentials: &[u8]) -> Result<Vec<u8>, JsValue> {
        let mut handshake = self
            .handshake
            .take()
            .ok_or_else(|| error("invalid handshake state"))?;
        if !handshake.read(frame).map_err(error)?.is_empty() {
            return Err(error("invalid handshake payload"));
        }
        let last = handshake.write(credentials).map_err(error)?;
        self.channel = Some(handshake.finish().map_err(error)?);
        Ok(last)
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<js_sys::Array, JsValue> {
        let frames = self
            .channel
            .as_mut()
            .ok_or_else(|| error("handshake required"))?
            .encrypt(plaintext)
            .map_err(error)?;
        let output = js_sys::Array::new();
        for frame in frames {
            output.push(&js_sys::Uint8Array::from(frame.as_slice()));
        }
        Ok(output)
    }

    pub fn decrypt(&mut self, frame: &[u8]) -> Result<Option<Vec<u8>>, JsValue> {
        Ok(self
            .channel
            .as_mut()
            .ok_or_else(|| error("handshake required"))?
            .decrypt(frame)
            .map_err(error)?
            .map(|value| value.to_vec()))
    }
}
