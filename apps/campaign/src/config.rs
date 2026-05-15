use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub campaign_data_dir: PathBuf,
    /// Shared bearer for `/internal/*` traffic. Same value used by every
    /// peer; rotation contract documented in `infra/pulumi-cloud/CLAUDE.md`.
    pub internal_bearer_primary: String,
    pub internal_bearer_secondary: Option<String>,
    /// Base URL of the platform binary, used for outbound callbacks
    /// (`POST /internal/platform/...`).
    pub platform_url: String,
}

impl Config {
    pub fn from_env() -> Self {
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
        Self {
            port,
            campaign_data_dir,
            internal_bearer_primary,
            internal_bearer_secondary,
            platform_url,
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

    #[test]
    #[serial]
    fn reads_required_env() {
        with_env(
            &[
                ("PORT", "3001"),
                ("CAMPAIGN_DATA_DIR", "data/dev-campaigns"),
                ("INTERNAL_BEARER_PRIMARY", "primary"),
                ("PLATFORM_URL", "http://localhost:3000"),
            ],
            || {
                let c = Config::from_env();
                assert_eq!(c.port, 3001);
                assert_eq!(c.campaign_data_dir, PathBuf::from("data/dev-campaigns"));
                assert_eq!(c.internal_bearer_primary, "primary");
                assert_eq!(c.internal_bearer_secondary, None);
                assert_eq!(c.platform_url, "http://localhost:3000");
            },
        );
    }

    #[test]
    #[serial]
    fn picks_up_optional_secondary_bearer() {
        with_env(
            &[
                ("PORT", "3001"),
                ("CAMPAIGN_DATA_DIR", "data/dev-campaigns"),
                ("INTERNAL_BEARER_PRIMARY", "primary"),
                ("INTERNAL_BEARER_SECONDARY", "secondary"),
                ("PLATFORM_URL", "http://localhost:3000"),
            ],
            || {
                let c = Config::from_env();
                assert_eq!(c.internal_bearer_secondary, Some("secondary".into()));
            },
        );
    }

    #[test]
    #[serial]
    #[should_panic(expected = "PORT is required")]
    fn panics_on_missing_port() {
        unsafe {
            std::env::remove_var("PORT");
            std::env::set_var("CAMPAIGN_DATA_DIR", "data/dev-campaigns");
            std::env::set_var("INTERNAL_BEARER_PRIMARY", "p");
            std::env::set_var("PLATFORM_URL", "http://localhost:3000");
        }
        let _ = Config::from_env();
    }

    #[test]
    #[serial]
    #[should_panic(expected = "CAMPAIGN_DATA_DIR is required")]
    fn panics_on_missing_campaign_data_dir() {
        unsafe {
            std::env::set_var("PORT", "3000");
            std::env::remove_var("CAMPAIGN_DATA_DIR");
            std::env::set_var("INTERNAL_BEARER_PRIMARY", "p");
            std::env::set_var("PLATFORM_URL", "http://localhost:3000");
        }
        let _ = Config::from_env();
    }

    #[test]
    #[serial]
    #[should_panic(expected = "INTERNAL_BEARER_PRIMARY is required")]
    fn panics_on_missing_internal_bearer() {
        unsafe {
            std::env::set_var("PORT", "3000");
            std::env::set_var("CAMPAIGN_DATA_DIR", "data/dev-campaigns");
            std::env::remove_var("INTERNAL_BEARER_PRIMARY");
            std::env::set_var("PLATFORM_URL", "http://localhost:3000");
        }
        let _ = Config::from_env();
    }

    #[test]
    #[serial]
    #[should_panic(expected = "PLATFORM_URL is required")]
    fn panics_on_missing_platform_url() {
        unsafe {
            std::env::set_var("PORT", "3000");
            std::env::set_var("CAMPAIGN_DATA_DIR", "data/dev-campaigns");
            std::env::set_var("INTERNAL_BEARER_PRIMARY", "p");
            std::env::remove_var("PLATFORM_URL");
        }
        let _ = Config::from_env();
    }
}
