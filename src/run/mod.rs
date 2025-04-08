use crate::build::{IMAGES_DIR, run_command};
use anyhow::{Error, anyhow};
use std::path::Path;

pub fn run(name: &str) -> Result<(), Error> {
    let x: String = format!("{IMAGES_DIR}/{name}.img");
    let p: String = format!("/mnt/{name}");
    if Path::new(x.as_str()).exists() {
        if run_command("sudo", &["mount", x.as_str(), p.as_str()])?
            && run_command("sudo", &["chroot", p.as_str()])?
            && run_command("sudo", &["umount", "-R", p.as_str()])?
        {
            return Ok(());
        }
    }
    Err(anyhow!("run lys init"))
}
