use git2::{Buf, Cred, FetchOptions, Oid, RemoteCallbacks, Repository, build::CheckoutBuilder};
use std::env;
use std::fs;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

pub const BASE_REPO_PATH: &str = "/var/db/forge";
pub const BASE_CONFIG_PATH: &str = "/etc/forge/packages";
pub type PackageList = Vec<(String, PathBuf, PathBuf)>;

pub fn collect_packages() -> Result<PackageList, String> {
    let pkgs: PackageList = fs::read_dir(BASE_CONFIG_PATH)
        .map_err(|e| format!("failed to iterate package directory: {}", e))?
        .map(|p| {
            let entry = p.map_err(|e| e.to_string())?;
            let path = entry.path();

            let pkgname = path
                .file_stem()
                .ok_or_else(|| format!("invalid filename: {:?}", path))?
                .to_string_lossy()
                .into_owned();

            let path = PathBuf::from(BASE_REPO_PATH).join(&pkgname);
            let cfg_path = PathBuf::from(BASE_CONFIG_PATH).join(format!("{}.toml", &pkgname));

            if !path.exists() || !cfg_path.exists() {
                Err(format!("no installed package: {}", pkgname))
            } else {
                Ok((pkgname, path, cfg_path))
            }
        })
        .collect::<Result<_, _>>()?;

    Ok(pkgs)
}

pub fn collect_named_packages(packages: Vec<String>) -> Result<PackageList, String> {
    let pkgs: PackageList = packages
        .into_iter()
        .map(|p| {
            let path = PathBuf::from(BASE_REPO_PATH).join(&p);
            let cfg_path = PathBuf::from(BASE_CONFIG_PATH).join(format!("{}.toml", p));
            if !path.exists() || !cfg_path.exists() {
                Err(format!("no installed package: {}", p))
            } else {
                Ok((p, path, cfg_path))
            }
        })
        .collect::<Result<_, _>>()?;

    Ok(pkgs)
}

pub fn dir_size(path: &Path) -> std::io::Result<u64> {
    let mut size = 0;
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if metadata.is_file() {
                size += metadata.len();
            } else if metadata.is_dir() {
                size += dir_size(&entry.path())?;
            }
        }
    }
    Ok(size)
}

pub fn get_commit_hash_full(path: &Path) -> Result<Oid, git2::Error> {
    let repo = Repository::open(path)?;
    let head = repo.head()?;

    let commit = head.peel_to_commit()?;
    Ok(commit.id())
}

pub fn get_commit_hash_short(path: &Path) -> Result<Buf, git2::Error> {
    let repo = Repository::open(path)?;
    let head = repo.head()?;

    let commit = head.peel_to_commit()?;
    Ok(repo.find_object(commit.id(), None)?.short_id()?)
}

pub fn get_editor() -> String {
    env::var("VISUAL")
        .or_else(|_| env::var("EDITOR"))
        .unwrap_or_else(|_| "nano".to_string())
}

pub fn get_remote_url(path: &Path) -> Result<String, git2::Error> {
    let repo = Repository::open(path)?;

    let remote = repo.find_remote("origin")?;

    if let Some(url) = remote.url() {
        Ok(url.to_string())
    } else {
        Err(git2::Error::from_str("Remote 'origin' has no URL"))
    }
}

pub fn open_in_editor(editor: &str, file: &str) -> Result<(), String> {
    let status = Command::new(editor)
        .arg(file)
        .status()
        .map_err(|e| format!("failed to execute editor: {}", e))?;

    if !status.success() {
        return Err(format!("editor exited with non-zero status: {}", status));
    }

    Ok(())
}

pub fn print_collected_packages(packages: &PackageList, message: &str) {
    println!(
        "{message} ({}): {}\n",
        packages.len(),
        packages
            .iter()
            .map(|(p, _, _)| p.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

pub fn pull_latest_tag(path: &Path) -> Result<(), git2::Error> {
    let repo = Repository::open(path)?;

    let head = repo.head()?;
    let branch = head
        .shorthand()
        .ok_or_else(|| git2::Error::from_str("Could not determine current branch"))?
        .to_string();

    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, _allowed| {
        Cred::ssh_key_from_agent(username_from_url.unwrap())
    });

    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);

    let mut remote = repo.find_remote("origin")?;
    remote.fetch(&["refs/tags/*:refs/tags/*"], Some(&mut fetch_options), None)?;

    let tag_names = repo.tag_names(None)?;

    let mut latest_commit = None;
    let mut latest_time = 0;

    for name in tag_names.iter().flatten() {
        let obj = repo.revparse_single(&format!("refs/tags/{}", name))?;
        let commit = obj.peel_to_commit()?;

        let time = commit.time().seconds();
        if time > latest_time {
            latest_time = time;
            latest_commit = Some(commit);
        }
    }

    let latest_commit = latest_commit.ok_or_else(|| git2::Error::from_str("No tags found"))?;

    let annotated = repo.find_annotated_commit(latest_commit.id())?;
    let (analysis, _) = repo.merge_analysis(&[&annotated])?;

    if analysis.is_fast_forward() {
        let refname = format!("refs/heads/{}", branch);
        let mut reference = repo.find_reference(&refname)?;
        reference.set_target(latest_commit.id(), "Fast-Forward to latest tag")?;
        repo.set_head(&refname)?;
        repo.checkout_head(Some(CheckoutBuilder::default().force()))?;
    } else if !analysis.is_up_to_date() {
        println!("Cannot fast-forward to latest tag.");
    }

    Ok(())
}
pub fn pull_repo(path: &Path) -> Result<(), git2::Error> {
    let repo = Repository::open(path)?;

    let head = repo.head()?;
    let branch = head
        .shorthand()
        .ok_or_else(|| git2::Error::from_str("Could not determine current branch"))?;

    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, _allowed| {
        Cred::ssh_key_from_agent(username_from_url.unwrap())
    });

    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);

    let mut remote = repo.find_remote("origin")?;
    remote.fetch(&[branch], Some(&mut fetch_options), None)?;

    let fetch_head = repo.find_reference("FETCH_HEAD")?;
    let fetch_commit = repo.reference_to_annotated_commit(&fetch_head)?;

    let (analysis, _pref) = repo.merge_analysis(&[&fetch_commit])?;

    if analysis.is_fast_forward() {
        let refname = format!("refs/heads/{}", branch);
        let mut reference = repo.find_reference(&refname)?;
        reference.set_target(fetch_commit.id(), "Fast-Forward")?;
        repo.set_head(&refname)?;
        repo.checkout_head(Some(CheckoutBuilder::default().force()))?;
    } else if !analysis.is_up_to_date() {
        println!("Non fast-forward merge required (manual merge needed).");
    }
    Ok(())
}

pub fn yn_prompt(prompt: &str) -> bool {
    print!("{} [y/n]: ", prompt);
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();

    let input = input.trim().to_lowercase();

    match input.as_str() {
        "y" | "yes" | "" => true,
        _ => false,
    }
}
