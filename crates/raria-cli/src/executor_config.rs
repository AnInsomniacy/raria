use raria_core::config::GlobalConfig;
use raria_range::executor::ExecutorConfig;

pub(crate) fn apply_global_retry_policy(
    mut executor_config: ExecutorConfig,
    global_config: &GlobalConfig,
) -> ExecutorConfig {
    executor_config.max_retries = match global_config.max_tries {
        0 => u32::MAX,
        n => n,
    };

    // aria2's `--retry-wait` is expressed in seconds. We map it to the executor's
    // retry base delay in milliseconds.
    //
    // NOTE: We intentionally do not allow a 0ms base delay because it would turn
    // retry loops into a busy loop under failure. When `retry_wait` is 0 (default),
    // we keep the executor's internal default.
    if global_config.retry_wait > 0 {
        executor_config.retry_base_delay_ms = (global_config.retry_wait as u64).saturating_mul(1000);
    }

    executor_config.lowest_speed_limit_bps = global_config.lowest_speed_limit;
    executor_config.max_file_not_found = global_config.max_file_not_found;

    executor_config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_tries_maps_to_executor_max_retries() {
        let global = GlobalConfig {
            max_tries: 3,
            ..Default::default()
        };
        let executor = apply_global_retry_policy(ExecutorConfig::default(), &global);
        assert_eq!(executor.max_retries, 3);
    }

    #[test]
    fn max_tries_zero_maps_to_infinite_retries() {
        let global = GlobalConfig {
            max_tries: 0,
            ..Default::default()
        };
        let executor = apply_global_retry_policy(ExecutorConfig::default(), &global);
        assert_eq!(executor.max_retries, u32::MAX);
    }

    #[test]
    fn retry_wait_overrides_retry_base_delay_ms() {
        let global = GlobalConfig {
            retry_wait: 2,
            ..Default::default()
        };
        let executor = apply_global_retry_policy(ExecutorConfig::default(), &global);
        assert_eq!(executor.retry_base_delay_ms, 2000);
    }

    #[test]
    fn retry_wait_zero_keeps_executor_default_delay() {
        let global = GlobalConfig::default();
        let executor = apply_global_retry_policy(ExecutorConfig::default(), &global);
        assert_eq!(executor.retry_base_delay_ms, ExecutorConfig::default().retry_base_delay_ms);
    }

    #[test]
    fn lowest_speed_limit_maps_to_executor_config() {
        let global = GlobalConfig {
            lowest_speed_limit: 1234,
            ..Default::default()
        };
        let executor = apply_global_retry_policy(ExecutorConfig::default(), &global);
        assert_eq!(executor.lowest_speed_limit_bps, 1234);
    }

    #[test]
    fn max_file_not_found_maps_to_executor_config() {
        let global = GlobalConfig {
            max_file_not_found: 3,
            ..Default::default()
        };
        let executor = apply_global_retry_policy(ExecutorConfig::default(), &global);
        assert_eq!(executor.max_file_not_found, 3);
    }
}
