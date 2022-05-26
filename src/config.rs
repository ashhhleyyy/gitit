use std::{path::Path, fs, collections::HashMap};

use serde::Deserialize;

use crate::errors::{Result, GititError};

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    pub server: ListenConfig,
    pub repos: HashMap<String, RepoConfig>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ListenConfig {
    pub address: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RepoConfig {
    pub url: String,
    pub title: String,
    #[serde(default = "default_head")]
    pub head: String,
}

fn default_head() -> String {
    "main".to_owned()
}

pub(super) fn load() -> Result<Config> {
    let path = Path::new("gitit.toml");
    if path.exists() {
        let content = fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    } else {
        Err(GititError::MissingConfig)
    }
}
