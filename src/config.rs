use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const TEMP_CONFIG_PATH: &str = "/var/lib/forge/.tmp";

pub enum ConfigCommand {
    Build,
    Install,
    Uninstall,
    PostInstall,
    PostUninstall,
    Clean,
}

#[derive(Deserialize)]
pub struct Config {
    pub update: Option<String>,
    pub hooks: Option<Hooks>,
}

#[derive(Deserialize)]
pub struct Hooks {
    pub build: Option<String>,
    pub install: Option<String>,
    pub uninstall: Option<String>,
    pub post_install: Option<String>,
    pub post_uninstall: Option<String>,
    pub clean: Option<String>,
}

impl Config {
    pub fn new<P: AsRef<Path>>(filepath: P) -> Option<Self> {
        let contents = match fs::read_to_string(filepath) {
            Ok(c) => c,
            Err(_) => {
                eprintln!("no package config found");
                return None;
            }
        };
        let config: Config = toml::from_str(&contents).expect("failed to parse config");
        Some(config)
    }

    pub fn log_config(self) {
        if let Some(update) = &self.update {
            println!("update: {}", update);
        }

        if let Some(hooks) = &self.hooks {
            println!("hooks:");

            for (name, value) in [
                ("build", &hooks.build),
                ("install", &hooks.install),
                ("uninstall", &hooks.uninstall),
                ("post_install", &hooks.post_install),
                ("post_uninstall", &hooks.post_uninstall),
                ("clean", &hooks.clean),
            ] {
                if let Some(val) = value {
                    println!("  {}: {}", name, val);
                }
            }
        }
    }
}

pub fn create_config(package: &str) -> Result<(), String> {
    let filename = format!("{package}.toml");
    let mut path = PathBuf::from(TEMP_CONFIG_PATH);

    if !path.exists() {
        fs::create_dir_all(&path)
            .map_err(|e| format!("failed to create temp config directory: {}", e))?;
    }

    path.push(filename);

    let template = format!(
        r#"# {package} configuration
update = "live" # no | live | tagged

[hooks]
build = "make"
install = "make install"
uninstall = "make uninstall"
post_install = ""
post_uninstall = ""
clean = "make clean"
    "#
    );

    fs::write(path, template).map_err(|e| format!("failed to create config: {}", e))?;

    Ok(())
}

pub fn run_config_command(
    config_path: &Path,
    repo_path: &Path,
    command: ConfigCommand,
) -> Result<(), String> {
    let config = Config::new(config_path).ok_or("config not found".to_string())?;
    let hooks = config.hooks.ok_or("no hooks section".to_string())?;

    let cmd = match command {
        ConfigCommand::Build => hooks.build,
        ConfigCommand::Install => hooks.install,
        ConfigCommand::Uninstall => hooks.uninstall,
        ConfigCommand::PostInstall => hooks.post_install,
        ConfigCommand::PostUninstall => hooks.post_uninstall,
        ConfigCommand::Clean => hooks.clean,
    };

    if let Some(c) = cmd {
        let mut parts = c.split_whitespace();
        let Some(cmd_base) = parts.next() else {
            // Empty command — do nothing and exit successfully
            return Ok(());
        };
        let args: Vec<&str> = parts.collect();

        let status = Command::new(cmd_base)
            .args(&args)
            .current_dir(repo_path)
            .status()
            .map_err(|e| format!("failed to execute command: {}", e))?;

        if !status.success() {
            return Err(format!("command exited with non-zero status: {}", status));
        }
    }

    Ok(())
}
