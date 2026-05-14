use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub campaign_data_dir: PathBuf,
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
        Self {
            port,
            campaign_data_dir,
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
            ],
            || {
                let c = Config::from_env();
                assert_eq!(c.port, 3001);
                assert_eq!(c.campaign_data_dir, PathBuf::from("data/dev-campaigns"));
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
        }
        let _ = Config::from_env();
    }
}
