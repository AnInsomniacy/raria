use std::time::Duration;

use raria_core::config::GlobalConfig;
use raria_core::native::TaskId;
use raria_ed2k::runtime::{Ed2kRuntimeConfig, Ed2kRuntimeContext, Ed2kRuntimeEventKind};

#[test]
fn runtime_context_projects_native_config_and_startup_status() {
    let config = GlobalConfig {
        ed2k_enabled: true,
        ed2k_enable_servers: true,
        ed2k_enable_kad: false,
        ed2k_listen_tcp_port: 48_001,
        ed2k_listen_udp_port: 48_002,
        ed2k_max_sources_per_task: 32,
        ed2k_max_upload_slots: 4,
        ed2k_share_completed: true,
        ..Default::default()
    };

    let task_id = TaskId::parse("task_00000000000000000000000000000001").expect("task id");
    let context = Ed2kRuntimeContext::new(
        task_id.clone(),
        Ed2kRuntimeConfig::from_global_config(&config),
    );

    assert_eq!(context.task_id(), &task_id);
    assert_eq!(context.identity_profile_id(), "default");
    assert_eq!(context.config().listen_tcp_port, 48_001);
    assert_eq!(context.config().listen_udp_port, 48_002);
    assert_eq!(context.config().max_sources_per_task, 32);
    assert_eq!(context.config().max_upload_slots, 4);
    assert!(context.config().share_completed);
    assert_eq!(context.state().source.known_sources, 0);
    assert_eq!(context.state().transfer.active_peers, 0);
    assert_eq!(context.state().sharing.shared_files, 0);

    let statuses = context.startup_statuses();
    assert!(statuses.iter().any(|status| {
        status.event_kind == Ed2kRuntimeEventKind::Source
            && status.category == "source"
            && status.state == "discovering"
            && status.metrics.get("knownSources") == Some(&0)
    }));
    assert!(statuses.iter().any(|status| {
        status.event_kind == Ed2kRuntimeEventKind::Kad
            && status.category == "kad"
            && status.state == "disabled"
    }));
}

#[test]
fn runtime_scheduler_ticks_without_placeholder_completion_claims() {
    let config = GlobalConfig {
        ed2k_enabled: true,
        ed2k_enable_servers: true,
        ed2k_enable_kad: true,
        ..Default::default()
    };

    let task_id = TaskId::parse("task_00000000000000000000000000000002").expect("task id");
    let mut context =
        Ed2kRuntimeContext::new(task_id, Ed2kRuntimeConfig::from_global_config(&config));

    let initial = context.tick(Duration::from_secs(0));
    assert!(initial.is_empty());

    let statuses = context.tick(Duration::from_secs(1));
    assert!(statuses.iter().any(|status| {
        status.event_kind == Ed2kRuntimeEventKind::Source
            && status.state == "discovering"
            && status.metrics.get("schedulerTicks") == Some(&1)
    }));
}
