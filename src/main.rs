use action::Action;
use std::{env, fs};

use crate::config::TEMP_CONFIG_PATH;

mod action;
mod config;
mod lock;
mod util;

fn main() {
    let args: Vec<String> = env::args().collect();

    match Action::parse(&args) {
        Ok(action) => {
            if let Err(e) = action.execute() {
                eprintln!("forge: {}", e);
            }
        }
        Err(e) => eprintln!("forge: {}", e),
    }
    // eat the error because end user doesn't care about cleanup
    let _ = fs::remove_dir_all(TEMP_CONFIG_PATH);
}
