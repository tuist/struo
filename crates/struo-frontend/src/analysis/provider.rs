use std::borrow::Borrow;
use std::io::{self, BufWriter, Write};
use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;
use serde_json::Value;
use sha2::{Digest, Sha256};
use starlark::eval::Evaluator;

use super::values::provider_json_to_value;

mod materialization;
mod serialized;
use materialization::Template;

pub(super) trait ProviderValue {
    fn materialize<'v>(
        &self,
        eval: &Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<starlark::values::Value<'v>>;
}

impl ProviderValue for Value {
    fn materialize<'v>(
        &self,
        eval: &Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<starlark::values::Value<'v>> {
        provider_json_to_value(eval, self)
    }
}

/// An immutable provider whose content fingerprint is shared by all consumers.
#[derive(Clone, Debug)]
pub struct SharedProvider(Arc<Provider>);

#[derive(Debug)]
struct Provider {
    value: OnceLock<Value>,
    serialized: Option<Box<RawValue>>,
    fingerprint: OnceLock<Option<[u8; 32]>>,
    template: OnceLock<Template>,
}

impl From<Value> for SharedProvider {
    fn from(value: Value) -> Self {
        Self(Arc::new(Provider {
            value: OnceLock::from(value),
            serialized: None,
            fingerprint: OnceLock::new(),
            template: OnceLock::new(),
        }))
    }
}

impl SharedProvider {
    pub fn transitive_sets_available(
        &self,
        workspace: &Path,
        sets: &super::TransitiveSets,
    ) -> bool {
        self.0.serialized.is_some() || sets.provider_available(workspace, self)
    }

    pub(super) fn fingerprint(&self) -> Option<&[u8; 32]> {
        self.0
            .fingerprint
            .get_or_init(|| {
                let mut writer = BufWriter::new(HashWriter(Sha256::new()));
                serde_json::to_writer(&mut writer, self.as_ref()).ok()?;
                Some(writer.into_inner().ok()?.0.finalize().into())
            })
            .as_ref()
    }
}

impl Deref for SharedProvider {
    type Target = Value;

    fn deref(&self) -> &Value {
        self.0.value.get_or_init(|| {
            serde_json::from_str(
                self.0
                    .serialized
                    .as_ref()
                    .expect("serialized provider")
                    .get(),
            )
            .expect("provider was validated before publication")
        })
    }
}

impl ProviderValue for SharedProvider {
    fn materialize<'v>(
        &self,
        eval: &Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<starlark::values::Value<'v>> {
        self.0
            .template
            .get_or_init(|| Template::new(self))
            .materialize(eval)
    }
}

impl AsRef<Value> for SharedProvider {
    fn as_ref(&self) -> &Value {
        self
    }
}

impl Borrow<Value> for SharedProvider {
    fn borrow(&self) -> &Value {
        self
    }
}

impl<'de> Deserialize<'de> for SharedProvider {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Box::<RawValue>::deserialize(deserializer)?;
        if let Ok(fingerprint) = serialized::fingerprint(raw.get()) {
            Ok(Self(Arc::new(Provider {
                value: OnceLock::new(),
                serialized: Some(raw),
                fingerprint: OnceLock::from(Some(fingerprint)),
                template: OnceLock::new(),
            })))
        } else {
            serde_json::from_str::<Value>(raw.get())
                .map(Self::from)
                .map_err(serde::de::Error::custom)
        }
    }
}

impl Serialize for SharedProvider {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_ref().serialize(serializer)
    }
}

struct HashWriter(Sha256);

impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_matches_json_bytes_across_buffer_boundaries() {
        let value = serde_json::json!({"nested": [null, true, -42, "λ\n\"\\".repeat(10_000)]});
        let expected: [u8; 32] = Sha256::digest(serde_json::to_vec(&value).unwrap()).into();
        let restored =
            serde_json::from_slice::<SharedProvider>(&serde_json::to_vec(&value).unwrap()).unwrap();
        for provider in [SharedProvider::from(value), restored] {
            std::thread::scope(|scope| {
                for _ in 0..8 {
                    let shared = provider.clone();
                    scope.spawn(move || assert_eq!(shared.fingerprint(), Some(&expected)));
                }
            });
            assert_eq!(provider.fingerprint(), Some(&expected));
        }
    }
}
