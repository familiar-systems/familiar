use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub campaign_data_dir: PathBuf,
}

impl Config {
    pub fn from_env() -> Self {
        let port: u16 = std::env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3000);
        let campaign_data_dir = std::env::var("CAMPAIGN_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/data/campaigns"));
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
    fn defaults_when_unset() {
        unsafe {
            std::env::remove_var("PORT");
            std::env::remove_var("CAMPAIGN_DATA_DIR");
        }
        let c = Config::from_env();
        assert_eq!(c.port, 3000);
        assert_eq!(c.campaign_data_dir, PathBuf::from("/data/campaigns"));
    }

    #[test]
    #[serial]
    fn reads_overrides_from_env() {
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
}
