use std::fs::create_dir_all;
use std::path::Path;
use anyhow::{anyhow, Error};

pub fn init(name: &str) ->Result<(), Error > {
    let root = Path::new(name);
    if root.exists() {
        return Err(anyhow!("{name} already exists"));
    }
    create_dir_all(root.join("src"))?;
    create_dir_all(root.join("bin"))?;
    create_dir_all(root.join("lib"))?;
    create_dir_all(root.join("share"))?;
    create_dir_all(root.join("hooks"))?;
    create_dir_all(root.join("tests"))?;
    Ok(())
}

