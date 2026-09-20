//! Record replication policies. Principals are supplied by a trusted server login path.
use crate::prelude::*;
use bevy::prelude::*;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordReadAccess {
    Public,
    /// For per-user tables whose record ID is the authenticated user's ID.
    OwnerByRecordId,
    ServerOnly,
}

#[derive(Clone, Copy, Debug)]
pub struct RecordPolicy {
    pub read: RecordReadAccess,
    pub client_writes: bool,
    /// False for service-owned records persisted explicitly before publishing an ECS update.
    pub automatic_persistence: bool,
}
impl Default for RecordPolicy {
    fn default() -> Self {
        Self {
            read: RecordReadAccess::Public,
            client_writes: true,
            automatic_persistence: true,
        }
    }
}
impl RecordPolicy {
    pub const fn owner_read_only() -> Self {
        Self {
            read: RecordReadAccess::OwnerByRecordId,
            client_writes: false,
            automatic_persistence: false,
        }
    }
    pub const fn server_only() -> Self {
        Self {
            read: RecordReadAccess::ServerOnly,
            client_writes: false,
            automatic_persistence: false,
        }
    }
    pub fn can_read(self, record_id: Id, principal: Option<Id>) -> bool {
        match self.read {
            RecordReadAccess::Public => true,
            RecordReadAccess::OwnerByRecordId => principal == Some(record_id),
            RecordReadAccess::ServerOnly => false,
        }
    }
}

#[derive(Resource, Default)]
pub struct RecordPolicies(HashMap<String, RecordPolicy>);
impl RecordPolicies {
    pub fn set<T: FluxRecord>(&mut self, policy: RecordPolicy) {
        self.0.insert(T::short_type_path().to_string(), policy);
    }
    pub fn get<T: FluxRecord>(&self) -> RecordPolicy {
        self.0
            .get(T::short_type_path())
            .copied()
            .unwrap_or_default()
    }
}

/// Never populate this map from a client-supplied record or network event.
#[derive(Resource, Default, Clone)]
pub struct AuthenticatedRecordPeers(Arc<RwLock<HashMap<Id, Id>>>);
impl AuthenticatedRecordPeers {
    pub fn bind(&self, peer: Id, user: Id) {
        self.0
            .write()
            .expect("principal lock poisoned")
            .insert(peer, user);
    }
    pub fn remove(&self, peer: Id) {
        self.0
            .write()
            .expect("principal lock poisoned")
            .remove(&peer);
    }
    pub fn principal(&self, peer: Id) -> Option<Id> {
        self.0
            .read()
            .expect("principal lock poisoned")
            .get(&peer)
            .copied()
    }
    pub fn readers(&self, policy: RecordPolicy, record_id: Id) -> Vec<Id> {
        self.0
            .read()
            .expect("principal lock poisoned")
            .iter()
            .filter_map(|(peer, user)| policy.can_read(record_id, Some(*user)).then_some(*peer))
            .collect()
    }
}

/// Block the direct database path as well as the Flux transport path. Server credentials
/// must have schema permissions. Existing records are retained; only table access is changed.
#[cfg(feature = "surrealdb")]
pub async fn restrict_record_table<T: FluxRecord>(
    db: &surrealdb::Surreal<surrealdb::engine::any::Any>,
) -> anyhow::Result<()> {
    let table = T::short_type_path();
    anyhow::ensure!(
        table.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "Invalid record table name"
    );
    db.query(format!("DEFINE TABLE IF NOT EXISTS {table} SCHEMALESS PERMISSIONS NONE; ALTER TABLE {table} PERMISSIONS NONE;"))
        .await?.check()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owner_policy_rejects_anonymous_and_other_accounts() {
        let owner = Id::from("10000000-0000-0000-0000-000000000001");
        let other = Id::from("10000000-0000-0000-0000-000000000002");
        let policy = RecordPolicy::owner_read_only();
        assert!(policy.can_read(owner, Some(owner)));
        assert!(!policy.can_read(owner, Some(other)));
        assert!(!policy.can_read(owner, None));
        assert!(!policy.client_writes);
        assert!(!policy.automatic_persistence);
        assert!(!RecordPolicy::server_only().can_read(owner, Some(owner)));
    }
    #[test]
    fn revocation_and_reauthentication_change_recipients() {
        let owner = Id::from("10000000-0000-0000-0000-000000000001");
        let peer = Id::from("10000000-0000-0000-0000-000000000002");
        let other = Id::from("10000000-0000-0000-0000-000000000003");
        let peers = AuthenticatedRecordPeers::default();
        peers.bind(peer, owner);
        assert_eq!(
            peers.readers(RecordPolicy::owner_read_only(), owner),
            vec![peer]
        );
        peers.bind(peer, other);
        assert!(
            peers
                .readers(RecordPolicy::owner_read_only(), owner)
                .is_empty()
        );
        peers.remove(peer);
        assert_eq!(peers.principal(peer), None);
    }
}
