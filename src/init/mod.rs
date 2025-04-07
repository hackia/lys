use anyhow::{Error, anyhow};
use std::fs::{create_dir_all, remove_file};
use std::process::Command;

const GENTOO_STAGE_URL: &str = "https://distfiles.gentoo.org/releases/amd64/autobuilds/20250406T165023Z/stage3-amd64-hardened-openrc-20250406T165023Z.tar.xz";
const GENTOO_STAGE_ARCHIVE: &str = "stage3-amd64-hardened-openrc-20250406T165023Z.tar.xz";
pub fn init(name: &str, os: &str, e: &str) -> Result<(), Error> {
    let enter = e.eq("yes") ;
    if os.eq("debian") {
        Command::new("sudo")
            .arg("debootstrap")
            .arg("stable")
            .arg(name)
            .arg("https://deb.debian.org/debian")
            .current_dir(".")
            .spawn()?
            .wait()?;
        if enter {
            Command::new("sudo")
                .arg("chroot")
                .arg(name)
                .current_dir(".")
                .spawn()?
                .wait()?;
        }
        Ok(())
    } else if os.eq("gentoo") {
        create_dir_all(name)?;
        Command::new("wget")
            .arg(GENTOO_STAGE_URL)
            .current_dir(".")
            .spawn()?
            .wait()?;
        Command::new("sudo")
            .arg("tar")
            .arg("vxpf")
            .arg(GENTOO_STAGE_ARCHIVE)
            .current_dir(".")
            .arg("-C")
            .arg(name)
            .spawn()?
            .wait()?;
        remove_file(GENTOO_STAGE_ARCHIVE)?;
        if enter {
            Command::new("sudo")
                .arg("chroot")
                .arg(name)
                .current_dir(".")
                .spawn()?
                .wait()?;
        }
        Ok(())
    } else if os.eq("alpine") {
        Command::new("sudo")
            .arg("alpine-chroot-install")
            .arg("-d")
            .arg(format!("/{name}").as_str())
            .arg("-p")
            .arg("build-base")
            .arg("-p")
            .arg("bash")
            .spawn()?
            .wait()?;
        if enter {
            Command::new(format!("/{name}/enter-chroot").as_str())
                .spawn()?
                .wait()?;
        }
        Ok(())
    } else {
        Err(anyhow!("{name} is not a valid os"))
    }
}
