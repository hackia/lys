use anyhow::{Error, anyhow};
use std::fs::{create_dir_all, remove_file};
use std::process::Command;

const GENTOO_STAGE_URL: &str = "https://distfiles.gentoo.org/releases/amd64/autobuilds/20250406T165023Z/stage3-amd64-hardened-openrc-20250406T165023Z.tar.xz";
const GENTOO_STAGE_ARCHIVE: &str = "stage3-amd64-hardened-openrc-20250406T165023Z.tar.xz";
const MIN_DEPS: &[&str] = &["wget", "git"];

pub enum PlanOs {
    Debian,
    Gentoo,
    Alpine,
}

impl PlanOs {
    pub fn from(name: &str) -> Result<Self, Error> {
        match name {
            "debian" => Ok(Self::Debian),
            "gentoo" => Ok(Self::Gentoo),
            "alpine" => Ok(Self::Alpine),
            _ => Err(anyhow!("Unknown OS: {}", name)),
        }
    }

    pub fn init(&self, name: &str, enter: bool, tmp: bool) -> Result<(), Error> {
        create_dir_all(name)?;
        match self {
            Self::Debian => init_debian(name, enter, tmp),
            Self::Gentoo => init_gentoo(name, enter, tmp),
            Self::Alpine => init_alpine(name, enter, tmp),
        }
    }
}

fn install(name: &str, os: &str, deps: &[&str]) -> Result<(), Error> {
    match os {
        "debian" => {
            for dep in deps {
                Command::new("sudo")
                    .arg("chroot")
                    .arg(format!("/mnt/{name}").as_str())
                    .arg("apt")
                    .arg("install")
                    .arg(dep)
                    .arg("-y")
                    .spawn()?
                    .wait()?;
            }
        }
        "gentoo" => {
            // À compléter si tu veux gérer emerge
        }
        "alpine" => {
            for dep in deps {
                Command::new("sudo")
                    .arg("chroot")
                    .arg(format!("/{name}").as_str())
                    .arg("apk")
                    .arg("add")
                    .arg(dep)
                    .spawn()?
                    .wait()?;
            }
        }
        _ => return Err(anyhow!("Unsupported OS for install")),
    }
    Ok(())
}

fn init_debian(name: &str, enter: bool, tmp: bool) -> Result<(), Error> {
    Command::new("sudo")
        .arg("debootstrap")
        .arg("stable")
        .arg(name)
        .arg("https://deb.debian.org/debian")
        .spawn()?
        .wait()?;

    install(name, "debian", MIN_DEPS)?;
    if enter {
        Command::new("sudo")
            .arg("chroot")
            .arg(name)
            .spawn()?
            .wait()?;
        if tmp {
            Command::new("sudo")
                .arg("umount")
                .arg(name)
                .spawn()?
                .wait()?;
        }
    }
    Ok(())
}

fn init_gentoo(name: &str, enter: bool, tmp: bool) -> Result<(), Error> {
    Command::new("wget").arg(GENTOO_STAGE_URL).spawn()?.wait()?;

    Command::new("sudo")
        .arg("tar")
        .arg("vxpf")
        .arg(GENTOO_STAGE_ARCHIVE)
        .arg("-C")
        .arg(name)
        .spawn()?
        .wait()?;

    install(name, "gentoo", MIN_DEPS)?;
    remove_file(GENTOO_STAGE_ARCHIVE)?;
    if enter {
        Command::new("sudo")
            .arg("chroot")
            .arg(name)
            .spawn()?
            .wait()?;
        if tmp {
            Command::new("sudo")
                .arg("umount")
                .arg(name)
                .spawn()?
                .wait()?;
        }
    }
    Ok(())
}
fn init_void(name: &str, enter: bool, tmp: bool) -> Result<(), Error> {
    create_dir_all(name)?;
    Command::new("wget")
        .arg("https://repo-default.voidlinux.org/live/current/void-x86_64-musl-ROOTFS-20250202.tar.xz")
        .spawn()?
        .wait()?;
    Command::new("sudo")
        .arg("tar")
        .arg("vxpf")
        .arg("void-x86_64-musl-ROOTFS-20250202.tar.xz")
        .arg("-C")
        .arg(name)
        .spawn()?
        .wait()?;
    remove_file("void-x86_64-musl-ROOTFS-20250202.tar.xz")?;

    if enter {
        Command::new("sudo")
            .arg("chroot")
            .arg(name)
            .spawn()?
            .wait()?;
        if tmp {
            Command::new("sudo")
                .arg("rm")
                .arg("-rf")
                .arg(name)
                .spawn()?
                .wait()?;
        }
    }
    Ok(())
}

fn init_alpine(name: &str, enter: bool, tmp: bool) -> Result<(), Error> {
    Command::new("sudo")
        .arg("alpine-chroot-install")
        .arg("-d")
        .arg(format!("/{name}"))
        .arg("-p")
        .arg("build-base")
        .arg("-p")
        .arg("bash")
        .spawn()?
        .wait()?;

    install(name, "alpine", MIN_DEPS)?;
    if enter {
        Command::new(format!("/{name}/enter-chroot"))
            .spawn()?
            .wait()?;
        if tmp {
            Command::new("sudo")
                .arg(format!("/{name}/destroy"))
                .arg("--remove")
                .spawn()?
                .wait()?;
        }
    }
    Ok(())
}

pub fn init(os: &str, name: &str, enter: bool, tmp: bool) -> Result<(), Error> {
    match os {
        "debian" => init_debian(name, enter, tmp),
        "gentoo" => init_gentoo(name, enter, tmp),
        "alpine" => init_alpine(name, enter, tmp),
        "void" => init_void(name, enter, tmp),
        _ => Err(anyhow!("os not supported")),
    }
}
