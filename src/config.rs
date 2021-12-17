use std::{convert::TryInto, fs, path::PathBuf, str::FromStr};

use serde_json::Value;

#[derive(Debug)]
pub struct Config {
    pub port: u16,
    pub img_folder: PathBuf,
    pub mirror: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            port: 30000,
            img_folder: PathBuf::from_str(".").unwrap(),
            mirror: None,
        }
    }
}

impl Config {
    pub fn init_from_environment() -> Config {
        let config_file: Value =
            serde_json::from_str(&fs::read_to_string("config.json").unwrap_or("{}".to_string()))
                .unwrap();
        let mut config = Config::default();
        let mut args = pico_args::Arguments::from_env();

        if let Ok(folder) = args.value_from_str("--folder") {
            config.img_folder = folder;
        } else if let Some(Value::String(folder)) = config_file.get("folder") {
            config.img_folder = PathBuf::from_str(&folder).unwrap();
        }

        if let Ok(port) = args.value_from_str("--port") {
            config.port = port;
        } else if let Some(Value::Number(port)) = config_file.get("port") {
            // * gazes longlingly at https://github.com/rust-lang/rust/issues/31436 *
            if let Some(port) = port.as_u64() {
                if let Ok(port) = port.try_into() {
                    config.port = port;
                }
            }
        }

        if let Ok(mirror) = args.value_from_str("--mirror") {
            config.mirror = Some(mirror);
        } else if let Some(Value::String(mirror)) = config_file.get("mirror") {
            config.mirror = Some(mirror.clone());
        }

        config
    }
}
