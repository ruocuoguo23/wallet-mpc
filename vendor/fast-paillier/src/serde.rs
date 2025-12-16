use rug::Integer;

use crate::{DecryptionKey, EncryptionKey};

impl serde::Serialize for EncryptionKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.n().serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for EncryptionKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let n = Integer::deserialize(deserializer)?;
        Ok(EncryptionKey::from_n(n))
    }
}

impl serde::Serialize for DecryptionKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let pq = [self.p(), self.q()];
        pq.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for DecryptionKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let [p, q] = <[Integer; 2]>::deserialize(deserializer)?;
        DecryptionKey::from_primes(p, q)
            .map_err(|_| <D::Error as serde::de::Error>::custom("invalid paillier key"))
    }
}
