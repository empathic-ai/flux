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
    /// Requires a record-specific rule installed with `RecordPolicies::set_read_rule`.
    Authorized,
    ServerOnly,
}

#[derive(Clone, Copy, Debug)]
pub struct RecordPolicy {
    pub read: RecordReadAccess,
    pub client_writes: bool,
    /// False for service-owned records persisted explicitly before publishing an ECS update.
    pub automatic_persistence: bool,
}

pub(crate) fn accepts_legacy_record_snapshot(peer_id: Id) -> bool {
    cfg!(not(feature = "server")) && peer_id == Id::nil()
}

impl Default for RecordPolicy {
    fn default() -> Self {
        Self {
            read: RecordReadAccess::ServerOnly,
            client_writes: false,
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
    /// Preflight only: `Authorized` still requires `RecordPolicies::can_read_record`
    /// after fetching the record and before sending any data.
    pub fn can_read(self, record_id: Id, principal: Option<Id>) -> bool {
        match self.read {
            RecordReadAccess::Public => true,
            RecordReadAccess::OwnerByRecordId => principal == Some(record_id),
            RecordReadAccess::Authorized => principal.is_some(),
            RecordReadAccess::ServerOnly => false,
        }
    }
}

type ReadRule = Box<dyn Fn(Id, Id, &dyn Reflect) -> bool + Send + Sync>;

#[derive(Resource, Default)]
pub struct RecordPolicies {
    policies: HashMap<String, RecordPolicy>,
    read_rules: HashMap<String, ReadRule>,
}
impl RecordPolicies {
    pub fn set<T: FluxRecord>(&mut self, policy: RecordPolicy) {
        self.policies.insert(T::short_type_path().to_string(), policy);
    }
    pub fn get<T: FluxRecord>(&self) -> RecordPolicy {
        self.policies
            .get(T::short_type_path())
            .copied()
            .unwrap_or_default()
    }

    pub fn set_read_rule<T: FluxRecord>(
        &mut self,
        rule: impl Fn(Id, Id, &T) -> bool + Send + Sync + 'static,
    ) {
        self.read_rules.insert(
            T::short_type_path().to_string(),
            Box::new(move |record_id, principal, record| {
                record.downcast_ref::<T>().is_some_and(|record| rule(record_id, principal, record))
            }),
        );
    }

    pub fn can_read_record<T: FluxRecord>(&self, id: Id, principal: Option<Id>, record: &T) -> bool {
        let policy = self.get::<T>();
        if policy.read == RecordReadAccess::Authorized {
            return principal.is_some_and(|principal| {
                self.read_rules.get(T::short_type_path())
                    .is_some_and(|rule| rule(id, principal, record))
            });
        }
        policy.can_read(id, principal)
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
    /// Select recipients for policies that need only an ID. Record-specific policies
    /// fail closed here; use `authorized_readers` with the actual record instead.
    pub fn readers(&self, policy: RecordPolicy, record_id: Id) -> Vec<Id> {
        if policy.read == RecordReadAccess::Authorized {
            return Vec::new();
        }
        self.0
            .read()
            .expect("principal lock poisoned")
            .iter()
            .filter_map(|(peer, user)| policy.can_read(record_id, Some(*user)).then_some(*peer))
            .collect()
    }

    pub fn authorized_readers<T: FluxRecord>(
        &self, policies: &RecordPolicies, record_id: Id, record: &T,
    ) -> Vec<Id> {
        self.0.read().expect("principal lock poisoned").iter()
            .filter_map(|(peer, principal)| {
                policies.can_read_record(record_id, Some(*principal), record).then_some(*peer)
            })
            .collect()
    }
}

/// Block the direct database path as well as the Flux transport path. Server credentials
/// must have schema permissions. Existing records are retained; only table access is changed.
#[cfg(feature = "surrealdb")]
pub async fn restrict_record_table<T: FluxRecord>(
    db: &surrealdb::Surreal<surrealdb::engine::any::Any>,
) -> anyhow::Result<()> {
    let table = crate::schema::database::record_table::<T>(T::short_type_path())?;
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
    fn unconfigured_records_deny_client_access_without_disabling_persistence() {
        let id = Id::from("10000000-0000-0000-0000-000000000001");
        let policy = RecordPolicy::default();
        assert!(!policy.can_read(id, None));
        assert!(!policy.can_read(id, Some(id)));
        assert!(!policy.client_writes);
        assert!(policy.automatic_persistence);
    }
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
