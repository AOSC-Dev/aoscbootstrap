use anyhow::Result;
use nix::sys::stat::{makedev, mknod, Mode, SFlag};
use nix::unistd::mkdir;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::{
    fs::{create_dir_all, write, File},
    io::Read,
};

pub fn bootstrap_apt(root: &Path, mirror: &str, branch: &str) -> Result<()> {
    create_dir_all(root.join("var/lib/dpkg"))?;
    create_dir_all(root.join("etc/apt"))?;
    create_dir_all(root.join("var/lib/apt/lists"))?;
    File::create(root.join("var/lib/dpkg/available"))?;
    File::create(root.join("var/lib/dpkg/status"))?;
    write(root.join("etc/locale.conf"), b"LANG=C.UTF-8\n")?;
    write(root.join("etc/shadow"), b"root:x:1:0:99999:7:::\n")?;
    write(
        root.join("etc/apt/sources.list"),
        format!("deb {} {} main\n", mirror, branch),
    )?;

    Ok(())
}

/// Calculate the Sha256 checksum of the given stream
pub fn sha256sum<R: Read>(mut reader: R) -> Result<String> {
    let mut hasher = Sha256::new();
    std::io::copy(&mut reader, &mut hasher)?;

    Ok(format!("{:x}", hasher.finalize()))
}

pub fn make_device_nodes(root: &Path) -> Result<()> {
    let permission = Mode::S_IRGRP
        | Mode::S_IRUSR
        | Mode::S_IROTH
        | Mode::S_IWGRP
        | Mode::S_IWUSR
        | Mode::S_IWOTH;
    mknod(
        &root.join("dev/null"),
        SFlag::S_IFCHR,
        permission,
        makedev(1, 3),
    )?;
    mknod(
        &root.join("dev/console"),
        SFlag::S_IFCHR,
        permission,
        makedev(5, 1),
    )?;
    mkdir(
        &root.join("dev/shm"),
        Mode::S_IRWXG | Mode::S_IRWXO | Mode::S_IRWXU | Mode::S_ISVTX,
    )?;

    Ok(())
}
