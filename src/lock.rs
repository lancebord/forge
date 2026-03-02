use serde::{Deserialize, Serialize};
use std::fs;

pub const LOCK_PATH: &str = "/var/db/forge/forge.lock";

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Package {
    pub name: String,
    pub source: String,
    pub checksum: String,
}

#[derive(Deserialize, Serialize, Default)]
pub struct Lockfile {
    pub package: Vec<Package>,
}

impl Lockfile {
    pub fn new() -> Self {
        if let Ok(contents) = fs::read_to_string(LOCK_PATH) {
            toml::from_str(&contents).unwrap_or_default()
        } else {
            Lockfile::default()
        }
    }

    pub fn out_of_date(&self, update: Package) -> bool {
        if let Some(existing) = self.package.iter().find(|p| p.name == update.name) {
            return existing.checksum != update.checksum;
        }
        true
    }

    pub fn update_pkg(&mut self, package: Package) -> Result<(), String> {
        if let Some(existing) = self.package.iter_mut().find(|p| p.name == package.name) {
            *existing = package;
        } else {
            self.package.push(package);
        }

        let toml_string = toml::to_string_pretty(&self)
            .map_err(|e| format!("failed to serialize lockfile: {e}"))?;

        fs::write(LOCK_PATH, toml_string).map_err(|e| format!("failed to write to lockfile: {e}"))
    }
}
