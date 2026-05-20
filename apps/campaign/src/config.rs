use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageBackend {
    Local,
    S3,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub storage_backend: StorageBackend,
    pub port: u16,
    pub campaign_data_dir: PathBuf,
    /// Shared bearer for `/internal/*` traffic. Same value used by every
    /// peer; rotation contract documented in `infra/pulumi-cloud/CLAUDE.md`.
    pub internal_bearer_primary: String,
    pub internal_bearer_secondary: Option<String>,
    /// Base URL of the platform binary, used for outbound callbacks
    /// (`POST /internal/platform/...`).
    pub platform_url: String,
    /// How long a campaign supervisor stays in memory after its last
    /// message before self-evicting. Tunable via
    /// `CAMPAIGN_IDLE_TIMEOUT_SECS`.
    ///
    /// In the target design (`docs/plans/2026-05-04-campaign-actor-domain-design.md`),
    /// the supervisor periodically writes its `.db` back to object
    /// storage while running, and idle eviction is the trigger for the
    /// final writeback + release: upload the current state, delete the
    /// local file, drop from RAM. The bucket is the source of truth;
    /// local disk is a working copy.
    ///
    /// TODO At the time of writing there is no object-storage path yet,
    /// so eviction degenerates into "drop from RAM, leave the `.db` on
    /// local disk." The next request reopens the same file. That's a
    /// stepping stone, not the contract.
    pub idle_timeout: Duration,
    /// How often each supervisor checks whether its `idle_timeout` has
    /// elapsed. Independent of `idle_timeout`; observed eviction lag
    /// is at most `idle_timeout + eviction_check_interval`. Tunable via
    /// `CAMPAIGN_EVICTION_CHECK_INTERVAL_SECS`. Tests pin this small
    /// (tens of milliseconds) to exercise eviction quickly.
    pub eviction_check_interval: Duration,
}

impl Config {
    pub fn from_env() -> Self {
        let storage_backend = match std::env::var("CAMPAIGN_STORAGE_BACKEND")
            .expect(
                "CAMPAIGN_STORAGE_BACKEND is required (\"local\" or \"s3\"). \
                 Set it in mise.toml [tasks.\"dev:campaign\"].env or in the deployment manifest.",
            )
            .as_str()
        {
            "local" => StorageBackend::Local,
            "s3" => StorageBackend::S3,
            other => panic!(
                "CAMPAIGN_STORAGE_BACKEND must be \"local\" or \"s3\", got \"{other}\""
            ),
        };
        let port: u16 = std::env::var("PORT")
            .expect(
                "PORT is required. Set it in mise.toml [tasks.\"dev:campaign\"].env or in the deployment manifest.",
            )
            .parse()
            .expect("PORT must be a valid u16");
        let campaign_data_dir = std::env::var("CAMPAIGN_DATA_DIR")
            .map(PathBuf::from)
            .expect(
                "CAMPAIGN_DATA_DIR is required. Set it in mise.toml [tasks.\"dev:campaign\"].env or in the deployment manifest.",
            );
        let internal_bearer_primary = std::env::var("INTERNAL_BEARER_PRIMARY").expect(
            "INTERNAL_BEARER_PRIMARY is required. \
             Sourced from Scaleway SM in deployed envs; \
             mise.toml exports a dev value locally.",
        );
        let internal_bearer_secondary = std::env::var("INTERNAL_BEARER_SECONDARY")
            .ok()
            .filter(|s| !s.is_empty());
        let platform_url = std::env::var("PLATFORM_URL").expect(
            "PLATFORM_URL is required. Set it in mise.toml [tasks.\"dev:campaign\"].env or in the deployment manifest.",
        );
        let idle_timeout_secs: u64 = std::env::var("CAMPAIGN_IDLE_TIMEOUT_SECS")
            .expect(
                "CAMPAIGN_IDLE_TIMEOUT_SECS is required. Set it in mise.toml [tasks.\"dev:campaign\"].env or in the deployment manifest.",
            )
            .parse()
            .expect("CAMPAIGN_IDLE_TIMEOUT_SECS must be a non-negative integer (seconds)");
        let eviction_check_interval_secs: u64 = std::env::var("CAMPAIGN_EVICTION_CHECK_INTERVAL_SECS")
            .expect(
                "CAMPAIGN_EVICTION_CHECK_INTERVAL_SECS is required. Set it in mise.toml [tasks.\"dev:campaign\"].env or in the deployment manifest.",
            )
            .parse()
            .expect("CAMPAIGN_EVICTION_CHECK_INTERVAL_SECS must be a non-negative integer (seconds)");
        Self {
            storage_backend,
            port,
            campaign_data_dir,
            internal_bearer_primary,
            internal_bearer_secondary,
            platform_url,
            idle_timeout: Duration::from_secs(idle_timeout_secs),
            eviction_check_interval: Duration::from_secs(eviction_check_interval_secs),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn with_env<F: FnOnce()>(vars: &[(&str, &str)], f: F) {
        for (k, v) in vars {
            unsafe {
                std::env::set_var(k, v);
            }
        }
        f();
        for (k, _) in vars {
            unsafe {
                std::env::remove_var(k);
            }
        }
    }

    fn full_env() -> Vec<(&'static str, &'static str)> {
        vec![
            ("CAMPAIGN_STORAGE_BACKEND", "local"),
            ("PORT", "3001"),
            ("CAMPAIGN_DATA_DIR", "data/dev-campaigns"),
            ("INTERNAL_BEARER_PRIMARY", "primary"),
            ("PLATFORM_URL", "http://localhost:3000"),
            ("CAMPAIGN_IDLE_TIMEOUT_SECS", "1800"),
            ("CAMPAIGN_EVICTION_CHECK_INTERVAL_SECS", "60"),
        ]
    }

    #[test]
    #[serial]
    fn reads_required_env() {
        with_env(&full_env(), || {
            let c = Config::from_env();
            assert_eq!(c.storage_backend, StorageBackend::Local);
            assert_eq!(c.port, 3001);
            assert_eq!(c.campaign_data_dir, PathBuf::from("data/dev-campaigns"));
            assert_eq!(c.internal_bearer_primary, "primary");
            assert_eq!(c.internal_bearer_secondary, None);
            assert_eq!(c.platform_url, "http://localhost:3000");
            assert_eq!(c.idle_timeout, Duration::from_secs(1800));
            assert_eq!(c.eviction_check_interval, Duration::from_secs(60));
        });
    }

    #[test]
    #[serial]
    fn picks_up_optional_secondary_bearer() {
        let mut env = full_env();
        env.push(("INTERNAL_BEARER_SECONDARY", "secondary"));
        with_env(&env, || {
            let c = Config::from_env();
            assert_eq!(c.internal_bearer_secondary, Some("secondary".into()));
        });
    }

    fn with_partial_env(skip: &str) -> Vec<(&'static str, &'static str)> {
        full_env().into_iter().filter(|(k, _)| *k != skip).collect()
    }

    #[test]
    #[serial]
    fn s3_backend_is_parsed() {
        let mut env = full_env();
        env.retain(|(k, _)| *k != "CAMPAIGN_STORAGE_BACKEND");
        env.push(("CAMPAIGN_STORAGE_BACKEND", "s3"));
        with_env(&env, || {
            let c = Config::from_env();
            assert_eq!(c.storage_backend, StorageBackend::S3);
        });
    }

    #[test]
    #[serial]
    #[should_panic(expected = "CAMPAIGN_STORAGE_BACKEND is required")]
    fn panics_on_missing_storage_backend() {
        unsafe {
            std::env::remove_var("CAMPAIGN_STORAGE_BACKEND");
        }
        with_env(&with_partial_env("CAMPAIGN_STORAGE_BACKEND"), || {
            let _ = Config::from_env();
        });
    }

    #[test]
    #[serial]
    #[should_panic(expected = "must be \"local\" or \"s3\"")]
    fn panics_on_invalid_storage_backend() {
        let mut env = with_partial_env("CAMPAIGN_STORAGE_BACKEND");
        env.push(("CAMPAIGN_STORAGE_BACKEND", "gcs"));
        with_env(&env, || {
            let _ = Config::from_env();
        });
    }

    #[test]
    #[serial]
    #[should_panic(expected = "PORT is required")]
    fn panics_on_missing_port() {
        unsafe {
            std::env::remove_var("PORT");
        }
        with_env(&with_partial_env("PORT"), || {
            let _ = Config::from_env();
        });
    }

    #[test]
    #[serial]
    #[should_panic(expected = "CAMPAIGN_DATA_DIR is required")]
    fn panics_on_missing_campaign_data_dir() {
        unsafe {
            std::env::remove_var("CAMPAIGN_DATA_DIR");
        }
        with_env(&with_partial_env("CAMPAIGN_DATA_DIR"), || {
            let _ = Config::from_env();
        });
    }

    #[test]
    #[serial]
    #[should_panic(expected = "INTERNAL_BEARER_PRIMARY is required")]
    fn panics_on_missing_internal_bearer() {
        unsafe {
            std::env::remove_var("INTERNAL_BEARER_PRIMARY");
        }
        with_env(&with_partial_env("INTERNAL_BEARER_PRIMARY"), || {
            let _ = Config::from_env();
        });
    }

    #[test]
    #[serial]
    #[should_panic(expected = "PLATFORM_URL is required")]
    fn panics_on_missing_platform_url() {
        unsafe {
            std::env::remove_var("PLATFORM_URL");
        }
        with_env(&with_partial_env("PLATFORM_URL"), || {
            let _ = Config::from_env();
        });
    }

    #[test]
    #[serial]
    #[should_panic(expected = "CAMPAIGN_IDLE_TIMEOUT_SECS is required")]
    fn panics_on_missing_idle_timeout() {
        unsafe {
            std::env::remove_var("CAMPAIGN_IDLE_TIMEOUT_SECS");
        }
        with_env(&with_partial_env("CAMPAIGN_IDLE_TIMEOUT_SECS"), || {
            let _ = Config::from_env();
        });
    }

    #[test]
    #[serial]
    #[should_panic(expected = "CAMPAIGN_IDLE_TIMEOUT_SECS must be a non-negative integer")]
    fn panics_on_invalid_idle_timeout() {
        let mut env = with_partial_env("CAMPAIGN_IDLE_TIMEOUT_SECS");
        env.push(("CAMPAIGN_IDLE_TIMEOUT_SECS", "thirty-minutes"));
        with_env(&env, || {
            let _ = Config::from_env();
        });
    }

    #[test]
    #[serial]
    #[should_panic(expected = "CAMPAIGN_EVICTION_CHECK_INTERVAL_SECS is required")]
    fn panics_on_missing_eviction_check_interval() {
        unsafe {
            std::env::remove_var("CAMPAIGN_EVICTION_CHECK_INTERVAL_SECS");
        }
        with_env(
            &with_partial_env("CAMPAIGN_EVICTION_CHECK_INTERVAL_SECS"),
            || {
                let _ = Config::from_env();
            },
        );
    }

    #[test]
    #[serial]
    #[should_panic(
        expected = "CAMPAIGN_EVICTION_CHECK_INTERVAL_SECS must be a non-negative integer"
    )]
    fn panics_on_invalid_eviction_check_interval() {
        let mut env = with_partial_env("CAMPAIGN_EVICTION_CHECK_INTERVAL_SECS");
        env.push(("CAMPAIGN_EVICTION_CHECK_INTERVAL_SECS", "one-minute"));
        with_env(&env, || {
            let _ = Config::from_env();
        });
    }
}
