# Record access policies

Existing add_record registrations preserve their previous public behavior. Use
add_record_with_policy for records needing a replication boundary:

    app.add_record_with_policy::<BillingAccount>(RecordPolicy::owner_read_only());
    app.add_record_with_policy::<PrivateCredentials>(RecordPolicy::server_only());

OwnerByRecordId supports per-user tables whose record ID is the authenticated
User ID. It permits reads and server-pushed updates only to peers whose trusted
principal matches that ID. ServerOnly forbids network reads. Both constructors
reject incoming client writes and disable automatic persistence on replicas.
They are intended for services that explicitly persist before publishing an ECS
record change. Public records can retain automatic persistence separately.

AuthenticatedRecordPeers is populated by the server's verified login code. Never
bind a principal from a record, asserted user ID, or unverified network event.
Remove the peer on logout/disconnect and replace its mapping on account changes.
The read check is repeated after asynchronous loading, immediately before sending.
Owner-only change replication also checks the current mapping for each update.
Already delivered data cannot be retracted from a recipient.

Client-side protected updates are accepted only from the authoritative server
peer (Id::nil()). Server-side protected updates reject all DbReceiveEvent writes;
services must publish through trusted ECS APIs after persistence. Transport
adapters must assign the sender ID themselves rather than trusting the envelope.

Replication restrictions do not replace database authorization. If clients have
direct database connections, call restrict_record_table::<T> with server credentials
to set table permissions to NONE, and route protected operations through the server.
The helper preserves records, requires schema permissions, and prevents ordinary
anonymous/record-user access. Administrative DB credentials bypass table permissions
and must never be exposed to clients. Broader role/relationship rules and revocable
client caches are future extensions; this API currently provides public,
owner-by-record-ID, and server-only visibility.

The policy tests cover anonymous/other-owner denial, server-only denial, blocked
client writes, account reassignment, and revocation of future recipients. The
server and web builds validate both feature configurations. The broader Flux
editing suite currently has 13 failures in unchanged tests whose setup omits
StatesPlugin; those are separate from these passing policy tests.
