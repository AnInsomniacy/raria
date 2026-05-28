//! Stable native ED2K client identity ownership.

use raria_core::native::{ED2K_CLIENT_IDENTITY_BYTES, NativeEd2kIdentityRow};
use raria_core::persist::Store;
use serde::{Deserialize, Serialize};

/// Stable native ED2K identity loaded from raria persistence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ed2kIdentity {
    /// Native ED2K identity profile id.
    pub profile_id: String,
    /// Stable ED2K client hash.
    pub client_hash: [u8; ED2K_CLIENT_IDENTITY_BYTES],
}

/// Load an ED2K identity from native persistence or create it once.
pub fn load_or_create_identity(store: &Store, profile_id: &str) -> anyhow::Result<Ed2kIdentity> {
    if let Some(row) = store.get_ed2k_identity(profile_id)? {
        return Ok(Ed2kIdentity {
            profile_id: row.profile_id,
            client_hash: row.client_hash,
        });
    }

    let client_hash = rand::random::<[u8; ED2K_CLIENT_IDENTITY_BYTES]>();
    let row = NativeEd2kIdentityRow::new(profile_id, client_hash);
    store.put_ed2k_identity(&row)?;
    Ok(Ed2kIdentity {
        profile_id: row.profile_id,
        client_hash: row.client_hash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_generated_once_and_reloaded_from_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("identity.redb");
        let store = raria_core::persist::Store::open(&db_path).expect("store");

        let first = load_or_create_identity(&store, "default").expect("first identity");
        let second = load_or_create_identity(&store, "default").expect("second identity");

        assert_eq!(first, second);
        assert_eq!(first.profile_id, "default");
        assert_ne!(first.client_hash, [0_u8; 16]);

        drop(store);
        let reopened = raria_core::persist::Store::open(&db_path).expect("reopened store");
        let third = load_or_create_identity(&reopened, "default").expect("third identity");

        assert_eq!(third, first);
    }
}
