#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub hanko_api_url: String,
    pub port: u16,
    pub cors_origins: Vec<String>,
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
        let cors_origins = std::env::var("CORS_ORIGINS")
            .expect("CORS_ORIGINS is required (comma-separated list of allowed origins)")
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Self {
            database_url,
            hanko_api_url,
            port,
            cors_origins,
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
    fn parses_cors_origins_csv() {
        with_env(
            &[
                ("HANKO_API_URL", "https://x.hanko.io"),
                (
                    "CORS_ORIGINS",
                    "http://localhost:5173, https://app.familiar.systems",
                ),
            ],
            || {
                let c = Config::from_env();
                assert_eq!(
                    c.cors_origins,
                    vec!["http://localhost:5173", "https://app.familiar.systems"]
                );
                assert_eq!(c.port, 3000);
                assert_eq!(c.database_url, "sqlite::memory:");
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
}
