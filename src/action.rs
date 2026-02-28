use crate::config::{self, Config, ConfigCommand, TEMP_CONFIG_PATH};
use crate::util::{self, BASE_CONFIG_PATH, BASE_REPO_PATH, PackageList};
use git2::Repository;
use std::fs;
use std::path::PathBuf;

pub enum Action {
    Add { url: String },
    Update,
    Upgrade { packages: Vec<String> },
    Remove { packages: Vec<String> },
    List,
    Search { term: String },
    Clean { packages: Vec<String> },
    Show { package: String },
    Version,
}

impl Action {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let cmd = args.get(1).ok_or("no command provided")?.as_str();

        match cmd {
            "add" => {
                let url = args.get(2).ok_or("add requires <repo>")?.clone();
                Ok(Action::Add { url })
            }
            "update" => Ok(Action::Update),
            "upgrade" => {
                let packages = args[2..].to_vec();
                Ok(Action::Upgrade { packages })
            }
            "remove" => {
                let packages = args[2..].to_vec();

                if packages.is_empty() {
                    Err("remove requires a package".into())
                } else {
                    Ok(Action::Remove { packages })
                }
            }
            "list" => Ok(Action::List),
            "search" => {
                let term = args.get(2).ok_or("search requires <term>")?.clone();
                Ok(Action::Search { term })
            }
            "clean" => {
                let packages = args[2..].to_vec();
                Ok(Action::Clean { packages })
            }
            "show" => {
                let package = args.get(2).ok_or("show requires <package>")?.clone();
                Ok(Action::Show { package })
            }
            "--version" => Ok(Action::Version),
            _ => Err(format!("unknown command {}", cmd)),
        }
    }

    pub fn execute(self) -> Result<(), String> {
        match self {
            Action::Add { url } => add(url.as_str()),
            Action::Update => update(),
            Action::Upgrade { packages } => upgrade(packages),
            Action::Remove { packages } => remove(packages),
            Action::List => list(),
            Action::Search { term } => Ok(search(term)),
            Action::Clean { packages } => clean(packages),
            Action::Show { package } => show(package),
            Action::Version => Ok(version()),
        }
    }
}

fn add(url: &str) -> Result<(), String> {
    if !nix::unistd::geteuid().is_root() {
        return Err("add must be run as root".to_string());
    }

    let repo_name = {
        let last_segment = url.rsplit('/').next().unwrap_or(url);
        last_segment.strip_suffix(".git").unwrap_or(last_segment)
    };
    let config_name = format!("{repo_name}.toml");

    println!("Creating config: {}", config_name);
    config::create_config(repo_name)?;

    let editor = util::get_editor();
    let config_temp = format!("{}/{}", TEMP_CONFIG_PATH, config_name);
    util::open_in_editor(&editor, &config_temp)?;

    let clone_path = PathBuf::from(BASE_REPO_PATH).join(repo_name);
    let repo = Repository::clone(url, &clone_path)
        .map_err(|e| format!("failed to clone {}: {}", repo_name, e))?;

    let mut config_path = PathBuf::from(BASE_CONFIG_PATH);
    if !config_path.exists() {
        fs::create_dir_all(&config_path)
            .map_err(|e| format!("failed to create config directory: {}", e))?;
    }

    config_path.push(config_name);

    fs::rename(config_temp, &config_path)
        .map_err(|e| format!("failed to place config in system directory: {}", e))?;

    println!(
        "New package initialized at: {}\n",
        repo.path().to_str().unwrap()
    );

    if util::yn_prompt("Run build and install commands?") {
        println!("Building...");
        config::run_config_command(&config_path, &clone_path, ConfigCommand::Build)?;
        println!("Installing...");
        config::run_config_command(&config_path, &clone_path, ConfigCommand::Install)?;
    }

    Ok(())
}

fn update() -> Result<(), String> {
    if !nix::unistd::geteuid().is_root() {
        return Err("update must be run as root".to_string());
    }

    let package_paths = util::collect_packages()?;
    util::print_collected_packages(&package_paths, "Packages to update");

    if util::yn_prompt("Proceed with update?") {
        for (name, path, _) in package_paths {
            util::pull_repo(&path).map_err(|e| format!("failed to update repo: {e}"))?;
            println!("{} up to date.", name);
        }
    }

    Ok(())
}

fn upgrade(packages: Vec<String>) -> Result<(), String> {
    if !nix::unistd::geteuid().is_root() {
        return Err("upgrade must be run as root".to_string());
    }

    let package_paths: PackageList = if packages.is_empty() {
        util::collect_packages()?
    } else {
        util::collect_named_packages(packages)?
    };

    util::print_collected_packages(&package_paths, "Packages to upgrade");

    if util::yn_prompt("Proceed with upgrade?") {
        for (name, path, cfg_path) in package_paths {
            config::run_config_command(&cfg_path, &path, ConfigCommand::Build)?;
            config::run_config_command(&cfg_path, &path, ConfigCommand::Install)?;
            println!("Upgraded {}", name);
        }
    }

    Ok(())
}

fn remove(packages: Vec<String>) -> Result<(), String> {
    if !nix::unistd::geteuid().is_root() {
        return Err("remove must be run as root".to_string());
    }

    println!("Checking dependencies...\n");
    let package_paths: PackageList = util::collect_named_packages(packages)?;

    util::print_collected_packages(&package_paths, "Packages to remove");

    let total_size: u64 = package_paths
        .iter()
        .map(|(_, path, _)| util::dir_size(path).unwrap_or(0))
        .sum();

    println!(
        "Total remove size: {:.2} MB\n",
        total_size as f64 / (1024.0 * 1024.0)
    );

    if util::yn_prompt("Proceed with removal?") {
        for (name, path, cfg_path) in package_paths {
            config::run_config_command(&cfg_path, &path, ConfigCommand::Uninstall)?;
            fs::remove_dir_all(&path).map_err(|e| format!("failed to remove {}: {}", name, e))?;
            fs::remove_file(&cfg_path).map_err(|e| format!("failed to remove {}: {}", name, e))?;
            println!("Removed {}", name);
        }
    }

    Ok(())
}

fn list() -> Result<(), String> {
    if !nix::unistd::geteuid().is_root() {
        return Err("list must be run as root".to_string());
    }

    for entry in fs::read_dir(BASE_REPO_PATH)
        .map_err(|e| format!("failed to iterate package directory: {}", e))?
    {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            let oid = util::get_commit_hash(&path)
                .map_err(|e| format!("failed to get commit hash: {e}"))?;
            let oid = oid.as_str().unwrap();
            if let Some(stem) = path.file_stem() {
                println!("{} ({})", stem.to_string_lossy(), oid);
            }
        }
    }
    Ok(())
}

fn search(term: String) {
    println!("searching: {}", term);
}

fn clean(packages: Vec<String>) -> Result<(), String> {
    if !nix::unistd::geteuid().is_root() {
        return Err("clean must be run as root".to_string());
    }

    let package_paths: PackageList = if packages.is_empty() {
        util::collect_packages()?
    } else {
        util::collect_named_packages(packages)?
    };

    util::print_collected_packages(&package_paths, "Packages to clean");

    if util::yn_prompt("Proceed with cleanup?") {
        for (name, path, cfg_path) in package_paths {
            config::run_config_command(&cfg_path, &path, ConfigCommand::Clean)?;
            println!("Cleaned {}", name);
        }
    }

    Ok(())
}

fn show(package: String) -> Result<(), String> {
    let config_path = PathBuf::from(BASE_CONFIG_PATH).join(format!("{package}.toml"));
    let config = Config::new(&config_path).ok_or("config not found".to_string())?;
    config.log_config();
    Ok(())
}

fn version() {
    println!(
        r#"
.-------..___    Forge v{}
'-._     :_.-'   Copyright (C) 2026 Lance Borden
    ) _ (
   '-' '-'       This program is free software
                 under the MIT license.
    "#,
        env!("CARGO_PKG_VERSION")
    );
}
