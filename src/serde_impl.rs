use crate::{NonEmptyBytes, NonEmptyBytesMut};
use bytes::{Bytes, BytesMut};
use serde::de::{Deserialize, Deserializer, Error};
use serde::ser::{Serialize, Serializer};

impl Serialize for NonEmptyBytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(self)
    }
}

impl Serialize for NonEmptyBytesMut {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(self)
    }
}

impl<'de> Deserialize<'de> for NonEmptyBytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bytes = Bytes::deserialize(deserializer)?;
        Self::new(bytes).ok_or_else(|| D::Error::invalid_length(0, &"at least one byte"))
    }
}

impl<'de> Deserialize<'de> for NonEmptyBytesMut {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bytes = BytesMut::deserialize(deserializer)?;
        Self::new(bytes).ok_or_else(|| D::Error::invalid_length(0, &"at least one byte"))
    }
}
