use anyhow::{Error, anyhow};
use std::{path::Path, process::Command};
pub const IMAGES_DIR: &str = "/var/lib/lys/images";

pub fn run_command(command: &str, args: &[&str]) -> Result<bool, Error> {
    Ok(Command::new(command)
        .args(args)
        .current_dir(".")
        .spawn()?
        .wait()?
        .success())
}

pub fn build_img(name: &str) -> Result<(), Error> {
    if Path::new(IMAGES_DIR).is_dir().eq(&false) {
        run_command("sudo", &["mkdir", "-p", IMAGES_DIR])?;
    }

    let img_path: String = format!("{IMAGES_DIR}/{name}.img");

    if run_command("sudo", &["mkdir", "-p", format!("/mnt/{name}").as_str()])?
        && run_command(
            "sudo",
            &["qemu-img", "create", "-f", "raw", &img_path, "50G"],
        )?
        && run_command("sudo", &["mkfs.ext4", &img_path])?
        && run_command(
            "sudo",
            &["mount", &img_path, format!("/mnt/{name}").as_str()],
        )?
    {
        return Ok(());
    }

    Err(anyhow!("failed to build image"))
}
