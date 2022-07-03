use anyhow::{anyhow, Result};
use nix::fcntl::{open, OFlag};
use nix::sys::stat::{fchmodat, makedev, mknod, FchmodatFlags, Mode, SFlag};
use nix::unistd::{close, mkdir};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;
use std::{
    fs::{create_dir_all, write, File},
    io::Read,
};
use tar::Builder;
use xz2::stream::{Filters, LzmaOptions, MtStreamBuilder, Stream};
use xz2::write::XzEncoder;

const LZMA_PRESET_EXTREME: u32 = 1 << 31;

pub fn bootstrap_apt(root: &Path, mirror: &str, branch: &str) -> Result<()> {
    create_dir_all(root.join("var/lib/dpkg"))?;
    create_dir_all(root.join("etc/apt"))?;
    create_dir_all(root.join("var/lib/apt/lists"))?;
    write(root.join("etc/locale.conf"), b"LANG=C.UTF-8\n")?;
    write(root.join("etc/shadow"), b"root:x:1:0:99999:7:::\n")?;
    write(
        root.join("etc/apt/sources.list"),
        format!("deb {} {} main\n", mirror, branch),
    )?;

    close(open(
        &root.join("var/lib/dpkg/available"),
        OFlag::O_CREAT,
        Mode::from_bits_truncate(0o644),
    )?)
    .ok();
    close(open(
        &root.join("var/lib/dpkg/status"),
        OFlag::O_CREAT,
        Mode::from_bits_truncate(0o644),
    )?)
    .ok();
    // chmod 0000 /etc/shadow
    fchmodat(
        None,
        &root.join("etc/shadow"),
        Mode::empty(),
        FchmodatFlags::NoFollowSymlink,
    )?;
    // chmod 0644 /etc/apt/sources.list
    fchmodat(
        None,
        &root.join("etc/apt/sources.list"),
        Mode::from_bits_truncate(0o644),
        FchmodatFlags::NoFollowSymlink,
    )?;

    Ok(())
}

/// Make a tarball (xz compressed)
pub fn archive_tarball(root: &Path, target: &Path, threads: u32) -> Result<()> {
    let f = File::create(target)?;
    let xz = build_xz_encoder(threads)?;
    let mut builder = Builder::new(XzEncoder::new_stream(f, xz));
    builder.mode(tar::HeaderMode::Complete);
    builder.follow_symlinks(false);
    builder.append_dir_all(".", root)?;
    builder.finish()?;
    builder.into_inner()?.finish()?.sync_all()?;

    Ok(())
}

/// Make a squashfs (xz compressed)
pub fn archive_squashfs(root: &Path, target: &Path, threads: u32) -> Result<()> {
    let output = Command::new("mksquashfs")
        .arg(root)
        .arg(target)
        .arg("-comp")
        .arg("xz")
        .arg("-processors")
        .arg(format!("{}", threads))
        .spawn()?
        .wait_with_output()?;
    if !output.status.success() {
        return Err(anyhow!("Failed to archive squashfs!"));
    }

    Ok(())
}

fn build_xz_encoder(threads: u32) -> Result<Stream> {
    let mut filter = Filters::new();
    let mut opts = LzmaOptions::new_preset(9 | LZMA_PRESET_EXTREME)?;
    opts.nice_len(273);
    filter.lzma2(&opts);

    Ok(MtStreamBuilder::new()
        .filters(filter)
        .threads(threads)
        .encoder()?)
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
