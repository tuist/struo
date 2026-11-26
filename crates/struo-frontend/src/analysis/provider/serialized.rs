use std::fmt;
use std::io::{BufWriter, Write};

use serde::de::{DeserializeSeed, Error, MapAccess, SeqAccess, Visitor};
use serde::{Deserializer, Serialize};
use sha2::{Digest, Sha256};

use super::HashWriter;

pub(super) fn fingerprint(raw: &str) -> Result<[u8; 32], serde_json::Error> {
    let mut writer = BufWriter::new(HashWriter(Sha256::new()));
    let mut deserializer = serde_json::Deserializer::from_str(raw);
    Canonical {
        writer: &mut writer,
        separator: b"",
    }
    .deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(writer
        .into_inner()
        .map_err(|error| serde_json::Error::io(error.into_error()))?
        .0
        .finalize()
        .into())
}

struct Canonical<'a, W> {
    writer: &'a mut W,
    separator: &'static [u8],
}

impl<W: Write> Canonical<'_, W> {
    fn value<E: Error, T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), E> {
        serde_json::to_writer(&mut *self.writer, value).map_err(E::custom)
    }
}

impl<'de, W: Write> DeserializeSeed<'de> for Canonical<'_, W> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        self.writer
            .write_all(self.separator)
            .map_err(D::Error::custom)?;
        deserializer.deserialize_any(self)
    }
}

impl<'de, W: Write> Visitor<'de> for Canonical<'_, W> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a provider without references and with ordered unique object keys")
    }

    fn visit_unit<E: Error>(mut self) -> Result<(), E> {
        self.value(&())
    }

    fn visit_bool<E: Error>(mut self, value: bool) -> Result<(), E> {
        self.value(&value)
    }

    fn visit_i64<E: Error>(mut self, value: i64) -> Result<(), E> {
        self.value(&value)
    }

    fn visit_u64<E: Error>(mut self, value: u64) -> Result<(), E> {
        self.value(&value)
    }

    fn visit_f64<E: Error>(mut self, value: f64) -> Result<(), E> {
        self.value(&value)
    }

    fn visit_str<E: Error>(mut self, value: &str) -> Result<(), E> {
        self.value(value)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        self.writer.write_all(b"[").map_err(A::Error::custom)?;
        let mut separator: &'static [u8] = b"";
        while seq
            .next_element_seed(Canonical {
                writer: self.writer,
                separator,
            })?
            .is_some()
        {
            separator = b",";
        }
        self.writer.write_all(b"]").map_err(A::Error::custom)
    }

    fn visit_map<A: MapAccess<'de>>(mut self, mut map: A) -> Result<(), A::Error> {
        self.writer.write_all(b"{").map_err(A::Error::custom)?;
        let mut previous: Option<String> = None;
        while let Some(key) = map.next_key::<String>()? {
            // Reserved keys include references and Serde's arbitrary-precision numbers.
            if key.starts_with('$') || previous.as_ref().is_some_and(|last| last >= &key) {
                return Err(A::Error::custom("provider requires eager validation"));
            }
            if previous.is_some() {
                self.writer.write_all(b",").map_err(A::Error::custom)?;
            }
            self.value(&key)?;
            previous = Some(key);
            self.writer.write_all(b":").map_err(A::Error::custom)?;
            map.next_value_seed(Canonical {
                writer: self.writer,
                separator: b"",
            })?;
        }
        self.writer.write_all(b"}").map_err(A::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::super::SharedProvider;
    use super::*;

    #[test]
    fn restored_providers_keep_json_semantics_and_fingerprints() {
        for raw in [
            r#"{"a":[null,true,false,-42,18446744073709551615,-0.0,1.25e-8],"b":{"λ":"\u0061\n\"\\"}}"#,
            r#" { "a" : [[],{}], "b" : "λ" } "#,
            r#"{"z":1,"a":2,"z":3}"#,
            r#"{"a":{"z":1,"b":2}}"#,
            "[0,-0,1e10,1e-320,-9223372036854775809,18446744073709551616]",
            "1e999",
            r#"{"$struo_transitive_set":7,"other":true}"#,
        ] {
            let value: serde_json::Value = serde_json::from_str(raw).unwrap();
            let bytes = serde_json::to_vec(&value).unwrap();
            let expected: [u8; 32] = Sha256::digest(&bytes).into();
            let provider: SharedProvider = serde_json::from_str(raw).unwrap();
            assert_eq!(provider.fingerprint(), Some(&expected), "{raw}");
            assert_eq!(provider.as_ref(), &value, "{raw}");
            assert_eq!(serde_json::to_vec(&provider).unwrap(), bytes, "{raw}");
        }
        for raw in ["[true,]", r#""\ud800""#, "{}{}", "{\"a\":", "NaN"] {
            assert!(
                serde_json::from_str::<SharedProvider>(raw).is_err(),
                "{raw}"
            );
        }
        let too_deep = format!("{}0{}", "[".repeat(130), "]".repeat(130));
        assert!(serde_json::from_str::<SharedProvider>(&too_deep).is_err());
    }

    #[test]
    fn restored_providers_still_require_nested_transitive_sets() {
        let workspace = tempfile::TempDir::new().unwrap();
        let sets = crate::analysis::TransitiveSets::default();
        for raw in [
            r#"{"nested":[{"$struo_transitive_set":"missing"}]}"#,
            r#"{"nested":[{"\u0024struo_transitive_set":"missing"}]}"#,
            r#"{"a":0,"a":{"$struo_transitive_set":"missing"}}"#,
        ] {
            let provider: SharedProvider = serde_json::from_str(raw).unwrap();
            assert!(!provider.transitive_sets_available(workspace.path(), &sets));
        }
        let overwritten: SharedProvider =
            serde_json::from_str(r#"{"a":{"$struo_transitive_set":"missing"},"a":0}"#).unwrap();
        assert!(overwritten.transitive_sets_available(workspace.path(), &sets));
    }
}
