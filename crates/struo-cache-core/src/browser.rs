use wasm_bindgen::prelude::*;

use crate::{ActionKeyBuilder, Digest};

#[wasm_bindgen]
pub fn digest(bytes: &[u8]) -> String {
    Digest::of_bytes(bytes).to_hex()
}

#[wasm_bindgen]
pub fn version() -> String {
    option_env!("STRUO_RELEASE_VERSION")
        .unwrap_or(env!("CARGO_PKG_VERSION"))
        .to_owned()
}

#[wasm_bindgen]
pub struct ActionKey(ActionKeyBuilder);

#[wasm_bindgen]
impl ActionKey {
    #[wasm_bindgen(constructor)]
    pub fn new(namespace: &str) -> Self {
        Self(ActionKeyBuilder::new(namespace))
    }

    #[wasm_bindgen(js_name = addBytes)]
    pub fn add_bytes(&mut self, label: &str, bytes: &[u8]) {
        self.0.push_bytes(label, bytes);
    }

    #[wasm_bindgen(js_name = addDigest)]
    pub fn add_digest(&mut self, label: &str, digest: &str) -> Result<(), JsError> {
        let digest = Digest::from_hex(digest)
            .ok_or_else(|| JsError::new("expected a lowercase BLAKE3 hex digest"))?;
        self.0.push_digest(label, digest);
        Ok(())
    }

    pub fn digest(&self) -> String {
        self.0.clone().finish().to_hex()
    }
}
