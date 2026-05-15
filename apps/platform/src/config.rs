#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub hanko_api_url: String,
    pub port: u16,
    pub cors_origins: Vec<String>,
    /// Shared bearer for `/internal/*` traffic. Required: every internal call
    /// (platform↔campaign) carries this in the `Authorization` header and the
    /// receiving middleware constant-time compares against the configured
    /// values. Sourced from Scaleway Secrets Manager in deployed
    /// environments; `mise.toml` exports a fixed dev string locally.
    pub internal_bearer_primary: String,
    /// Optional secondary bearer to support zero-downtime rotation.
    /// During rotation, both primary and secondary are accepted; once
    /// every caller has switched to the new value, the operator removes
    /// secondary and re-deploys. See the rotation contract in
    /// `infra/pulumi-cloud/CLAUDE.md`.
    pub internal_bearer_secondary: Option<String>,
    /// Base URL of the campaign-tier shard the platform routes new
    /// campaigns to. v0 uses one shard; round-robin/load-aware comes later.
    pub campaign_shard_url: String,
}

impl Config {
    pub fn from_env() -> Self {
        let hanko_api_url = std::env::var("HANKO_API_URL")
            .expect("HANKO_API_URL is required. Set it in mise.toml [tasks.\"dev:platform\"].env or in the deployment manifest.");
        let database_url =
            std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite::memory:".to_string());
        let port: u16 = std::env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3000);
        // Required even in same-origin deployments (SPA and platform share an
        // apex through Caddy/ingress), where the CorsLayer is a no-op in practice.
        // Keeping it required forces split-service self-hosts (SPA at
        // `app.example.com`, this binary at `api.example.com`) to set it at
        // deploy time rather than silently failing in the browser later. See
        // routes/mod.rs for the matching layer-level note.
        let cors_origins = std::env::var("CORS_ORIGINS")
            .expect("CORS_ORIGINS is required (comma-separated list of allowed origins)")
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let internal_bearer_primary = std::env::var("INTERNAL_BEARER_PRIMARY").expect(
            "INTERNAL_BEARER_PRIMARY is required. \
             Sourced from Scaleway SM in deployed envs; \
             mise.toml exports a dev value locally.",
        );
        let internal_bearer_secondary = std::env::var("INTERNAL_BEARER_SECONDARY")
            .ok()
            .filter(|s| !s.is_empty());
        let campaign_shard_url = std::env::var("CAMPAIGN_SHARD_URL").expect(
            "CAMPAIGN_SHARD_URL is required. \
             Set in mise.toml [tasks.\"dev:platform\"].env or deployment manifest.",
        );
        Self {
            database_url,
            hanko_api_url,
            port,
            cors_origins,
            internal_bearer_primary,
            internal_bearer_secondary,
            campaign_shard_url,
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
    fn parses_required_env_vars() {
        with_env(
            &[
                ("HANKO_API_URL", "https://x.hanko.io"),
                (
                    "CORS_ORIGINS",
                    "http://localhost:5173, https://app.familiar.systems",
                ),
                ("INTERNAL_BEARER_PRIMARY", "primary-token"),
                ("CAMPAIGN_SHARD_URL", "http://localhost:3001"),
            ],
            || {
                let c = Config::from_env();
                assert_eq!(
                    c.cors_origins,
                    vec!["http://localhost:5173", "https://app.familiar.systems"]
                );
                assert_eq!(c.port, 3000);
                assert_eq!(c.database_url, "sqlite::memory:");
                assert_eq!(c.internal_bearer_primary, "primary-token");
                assert_eq!(c.internal_bearer_secondary, None);
                assert_eq!(c.campaign_shard_url, "http://localhost:3001");
            },
        );
    }

    #[test]
    #[serial]
    fn picks_up_optional_secondary_bearer() {
        with_env(
            &[
                ("HANKO_API_URL", "https://x.hanko.io"),
                ("CORS_ORIGINS", "http://localhost:5173"),
                ("INTERNAL_BEARER_PRIMARY", "primary"),
                ("INTERNAL_BEARER_SECONDARY", "secondary"),
                ("CAMPAIGN_SHARD_URL", "http://localhost:3001"),
            ],
            || {
                let c = Config::from_env();
                assert_eq!(c.internal_bearer_secondary, Some("secondary".into()));
            },
        );
    }

    #[test]
    #[serial]
    fn empty_secondary_bearer_treated_as_unset() {
        with_env(
            &[
                ("HANKO_API_URL", "https://x.hanko.io"),
                ("CORS_ORIGINS", "http://localhost:5173"),
                ("INTERNAL_BEARER_PRIMARY", "primary"),
                ("INTERNAL_BEARER_SECONDARY", ""),
                ("CAMPAIGN_SHARD_URL", "http://localhost:3001"),
            ],
            || {
                let c = Config::from_env();
                assert_eq!(c.internal_bearer_secondary, None);
            },
        );
    }

    #[test]
    #[serial]
    #[should_panic(expected = "HANKO_API_URL is required")]
    fn panics_on_missing_hanko_url() {
        unsafe {
            std::env::remove_var("HANKO_API_URL");
        }
        let _ = Config::from_env();
    }

    #[test]
    #[serial]
    #[should_panic(expected = "INTERNAL_BEARER_PRIMARY is required")]
    fn panics_on_missing_internal_bearer() {
        unsafe {
            std::env::remove_var("INTERNAL_BEARER_PRIMARY");
        }
        with_env(
            &[
                ("HANKO_API_URL", "https://x.hanko.io"),
                ("CORS_ORIGINS", "http://localhost:5173"),
                ("CAMPAIGN_SHARD_URL", "http://localhost:3001"),
            ],
            || {
                let _ = Config::from_env();
            },
        );
    }

    #[test]
    #[serial]
    #[should_panic(expected = "CAMPAIGN_SHARD_URL is required")]
    fn panics_on_missing_campaign_shard_url() {
        unsafe {
            std::env::remove_var("CAMPAIGN_SHARD_URL");
        }
        with_env(
            &[
                ("HANKO_API_URL", "https://x.hanko.io"),
                ("CORS_ORIGINS", "http://localhost:5173"),
                ("INTERNAL_BEARER_PRIMARY", "primary"),
            ],
            || {
                let _ = Config::from_env();
            },
        );
    }
}
