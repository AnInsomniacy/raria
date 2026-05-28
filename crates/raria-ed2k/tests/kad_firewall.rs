use raria_ed2k::kad::{KadFirewallObservation, KadFirewallState, KadFirewallStatus};
use raria_ed2k::peer::{PeerCapabilities, ServerMediatedCallback};

#[test]
fn firewall_state_tracks_tcp_udp_reachability_and_check_cadence() {
    let mut state = KadFirewallState::new(false);

    assert_eq!(state.tcp_status(), KadFirewallStatus::Unknown);
    assert_eq!(state.udp_status(), KadFirewallStatus::Unknown);
    assert!(state.should_check(100, true, true));

    state.record_check_started(100);
    assert!(!state.should_check(200, true, true));

    state.record_observation(KadFirewallObservation::TcpOpen, 210);
    state.record_observation(KadFirewallObservation::UdpFirewalled, 211);

    assert_eq!(state.tcp_status(), KadFirewallStatus::Open);
    assert_eq!(state.udp_status(), KadFirewallStatus::Firewalled);
    assert!(state.allows_direct_source_publish());
    assert!(!state.udp_reachable());
    assert!(state.should_check(1_301, true, true));
}

#[test]
fn assumed_firewalled_state_blocks_direct_publish_until_observed_open() {
    let mut state = KadFirewallState::new(true);

    assert_eq!(state.tcp_status(), KadFirewallStatus::Firewalled);
    assert_eq!(state.udp_status(), KadFirewallStatus::Firewalled);
    assert!(!state.allows_direct_source_publish());
    assert!(!state.should_check(100, false, true));
    assert!(!state.should_check(100, true, false));

    state.record_observation(KadFirewallObservation::TcpOpen, 200);
    assert!(state.allows_direct_source_publish());
}

#[test]
fn unsupported_buddy_and_direct_callback_paths_stay_unadvertised() {
    let local = PeerCapabilities::local();

    assert_eq!(local.kad_version, 0);
    assert!(!local.supports_direct_udp_callback);
    assert!(!ServerMediatedCallback::supports_direct_udp_callback());
    assert!(!ServerMediatedCallback::supports_kad_buddy_callback());
    assert!(!ServerMediatedCallback::supports_required_crypt_callback());
}
