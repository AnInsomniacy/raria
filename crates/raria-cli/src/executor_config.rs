use raria_core::config::GlobalConfig;
use raria_range::executor::ExecutorConfig;

pub(crate) fn apply_global_retry_policy(
    mut executor_config: ExecutorConfig,
    global_config: &GlobalConfig,
) -> ExecutorConfig {
    executor_config.max_retries = match global_config.retry_attempts {
        0 => u32::MAX,
        n => n,
    };

    // The native retry wait value is expressed in seconds. The range executor
    // stores its base delay in milliseconds.
    //
    // NOTE: We intentionally do not allow a 0ms base delay because it would turn
    // retry loops into a busy loop under failure. When `retry_delay_seconds` is 0 (default),
    // we keep the executor's internal default.
    if global_config.retry_delay_seconds > 0 {
        executor_config.retry_base_delay_ms =
            (global_config.retry_delay_seconds as u64).saturating_mul(1000);
    }

    executor_config.lowest_speed_limit_bps = global_config.min_speed;
    executor_config.max_file_not_found = global_config.max_not_found;

    executor_config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_attempts_maps_to_executor_max_retries() {
        let global = GlobalConfig {
            retry_attempts: 3,
            ..Default::default()
        };
        let executor = apply_global_retry_policy(ExecutorConfig::default(), &global);
        assert_eq!(executor.max_retries, 3);
    }

    #[test]
    fn retry_attempts_zero_maps_to_infinite_retries() {
        let global = GlobalConfig {
            retry_attempts: 0,
            ..Default::default()
        };
        let executor = apply_global_retry_policy(ExecutorConfig::default(), &global);
        assert_eq!(executor.max_retries, u32::MAX);
    }

    #[test]
    fn retry_delay_seconds_overrides_retry_base_delay_ms() {
        let global = GlobalConfig {
            retry_delay_seconds: 2,
            ..Default::default()
        };
        let executor = apply_global_retry_policy(ExecutorConfig::default(), &global);
        assert_eq!(executor.retry_base_delay_ms, 2000);
    }

    #[test]
    fn retry_delay_seconds_zero_keeps_executor_default_delay() {
        let global = GlobalConfig::default();
        let executor = apply_global_retry_policy(ExecutorConfig::default(), &global);
        assert_eq!(
            executor.retry_base_delay_ms,
            ExecutorConfig::default().retry_base_delay_ms
        );
    }

    #[test]
    fn min_speed_maps_to_executor_config() {
        let global = GlobalConfig {
            min_speed: 1234,
            ..Default::default()
        };
        let executor = apply_global_retry_policy(ExecutorConfig::default(), &global);
        assert_eq!(executor.lowest_speed_limit_bps, 1234);
    }

    #[test]
    fn max_not_found_maps_to_executor_config() {
        let global = GlobalConfig {
            max_not_found: 3,
            ..Default::default()
        };
        let executor = apply_global_retry_policy(ExecutorConfig::default(), &global);
        assert_eq!(executor.max_file_not_found, 3);
    }
}
