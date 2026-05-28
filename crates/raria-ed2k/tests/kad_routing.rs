use raria_ed2k::kad::{
    KadContact, KadEndpoint, KadRoutingNodeState, KadRoutingTable, KadTransactionPurpose,
    KadTransactionTable, validate_routing_contact,
};
use raria_ed2k::opcode::KadOpcode;

fn id(value: u8) -> [u8; 16] {
    let mut id = [0; 16];
    id[0] = value;
    id
}

fn contact(value: u8, host: &str) -> KadContact {
    KadContact {
        id: id(value),
        host: host.to_string(),
        udp_port: 4672,
        tcp_port: 4662,
        version: 8,
        udp_key: Some(0x1122_3344),
        verified: true,
    }
}

#[test]
fn validates_routing_contacts_before_bucket_insert() {
    let self_id = id(0x11);
    assert!(validate_routing_contact(&contact(0x22, "203.0.113.1"), &self_id).is_ok());

    let mut same_id = contact(0x11, "203.0.113.2");
    assert!(validate_routing_contact(&same_id, &self_id).is_err());

    same_id.id = id(0x22);
    same_id.host = "0.0.0.0".to_string();
    assert!(validate_routing_contact(&same_id, &self_id).is_err());

    same_id.host = "203.0.113.2".to_string();
    same_id.udp_port = 0;
    assert!(validate_routing_contact(&same_id, &self_id).is_err());

    same_id.udp_port = 4672;
    same_id.version = 1;
    assert!(validate_routing_contact(&same_id, &self_id).is_err());

    same_id.version = 5;
    same_id.udp_port = 53;
    assert!(validate_routing_contact(&same_id, &self_id).is_err());
}

#[test]
fn keeps_confirmed_live_contacts_and_promotes_replacements() {
    let self_id = id(0x00);
    let mut table = KadRoutingTable::new(self_id, 2);
    let first = contact(0x80, "203.0.113.1");
    let second = contact(0x81, "203.0.113.2");
    let replacement = contact(0x82, "203.0.113.3");

    table.node_seen(first.clone(), 10).unwrap();
    table.node_seen(second.clone(), 11).unwrap();
    table.heard_about(replacement.clone(), 12).unwrap();

    let bucket = table.bucket_for(&first.id).expect("bucket");
    assert_eq!(bucket.live.len(), 2);
    assert_eq!(bucket.replacements.len(), 1);
    assert!(
        bucket
            .live
            .iter()
            .all(|node| node.state == KadRoutingNodeState::Confirmed)
    );

    table.node_failed(&first.id, 13);

    let bucket = table.bucket_for(&replacement.id).expect("bucket");
    assert_eq!(bucket.live.len(), 2);
    assert!(
        bucket
            .live
            .iter()
            .any(|node| node.contact.id == replacement.id)
    );
    assert!(bucket.replacements.is_empty());
}

#[test]
fn finds_closest_contacts_and_excludes_requester() {
    let self_id = id(0x00);
    let mut table = KadRoutingTable::new(self_id, 4);
    let closest = contact(0x10, "203.0.113.10");
    let excluded = contact(0x11, "203.0.113.11");
    let far = contact(0xf0, "203.0.113.12");

    table.node_seen(far.clone(), 10).unwrap();
    table.node_seen(excluded.clone(), 11).unwrap();
    table.node_seen(closest.clone(), 12).unwrap();

    let found = table.find_closest(&id(0x10), 3, false);
    assert_eq!(found[0].id, closest.id);

    let found = table.find_closest_excluding(&id(0x10), &excluded.id, 3, false);
    assert!(found.iter().all(|contact| contact.id != excluded.id));
    assert_eq!(found[0].id, closest.id);
}

#[test]
fn bootstrap_and_refresh_cadence_is_deterministic() {
    let self_id = id(0x00);
    let mut table = KadRoutingTable::new(self_id, 2);

    assert!(table.needs_bootstrap(30));
    table.record_bootstrap(30);
    assert!(!table.needs_bootstrap(59));
    assert!(table.needs_bootstrap(60));

    table.node_seen(contact(0x80, "203.0.113.1"), 100).unwrap();
    assert!(!table.needs_bootstrap(130));
    assert!(table.needs_refresh(&self_id, 1_000));
    table.record_refresh(&self_id, 1_000);
    assert!(!table.needs_refresh(&self_id, 1_044));
    assert!(table.needs_refresh(&self_id, 1_945));
}

#[test]
fn transaction_table_completes_and_expires_by_endpoint_opcode_and_target() {
    let mut table = KadTransactionTable::default();
    let endpoint = KadEndpoint::new("203.0.113.10", 4672);
    let target = id(0x42);

    table.add(
        endpoint.clone(),
        KadOpcode::RequestV2,
        Some(target),
        KadTransactionPurpose::Lookup,
        100,
    );
    table.add(
        KadEndpoint::new("203.0.113.11", 4672),
        KadOpcode::HelloRequestV2,
        None,
        KadTransactionPurpose::Hello,
        105,
    );

    let completed = table.complete_with_target(&endpoint, KadOpcode::RequestV2, &target);
    assert!(completed.is_some());
    assert!(
        table
            .complete_with_target(&endpoint, KadOpcode::RequestV2, &target)
            .is_none()
    );

    let expired = table.expire(136, 30);
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].purpose, KadTransactionPurpose::Hello);
}

#[test]
fn snapshot_restore_preserves_routing_state_and_maintenance_times() {
    let self_id = id(0x00);
    let mut table = KadRoutingTable::new(self_id, 2);
    let live = contact(0x80, "203.0.113.1");
    let second_live = contact(0x82, "203.0.113.3");
    let replacement = contact(0x81, "203.0.113.2");

    table.node_seen(live.clone(), 10).unwrap();
    table.node_seen(second_live, 11).unwrap();
    table.heard_about(replacement.clone(), 12).unwrap();
    table.record_bootstrap(30);
    table.record_refresh(&self_id, 1_000);

    let snapshot = table.snapshot();
    let restored = KadRoutingTable::restore(snapshot).expect("snapshot restores");

    assert_eq!(restored.self_id(), &self_id);
    assert_eq!(restored.last_bootstrap_seconds(), Some(30));
    assert_eq!(restored.last_self_refresh_seconds(), Some(1_000));
    assert_eq!(restored.find_closest(&live.id, 1, true)[0].id, live.id);
    assert_eq!(
        restored
            .bucket_for(&replacement.id)
            .expect("bucket")
            .replacements[0]
            .contact
            .id,
        replacement.id
    );
}
