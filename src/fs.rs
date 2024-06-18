use anyhow::{anyhow, Result};
use flate2::write::GzEncoder;
use flate2::Compression;
use nix::fcntl::{open, OFlag};
use nix::sys::stat::{fchmodat, FchmodatFlags, Mode};
use nix::unistd::close;
use sha2::{Digest, Sha256};
use std::io::Write;
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
pub fn archive_xz_tarball(root: &Path, target: &Path, threads: u32) -> Result<()> {
    let f = File::create(target)?;
    let xz = build_xz_encoder(threads)?;
    let builder = build_tarball_stream(XzEncoder::new_stream(f, xz), root)?;
    builder.into_inner()?.finish()?.sync_all()?;

    Ok(())
}

/// Make a tarball (gz compressed)
pub fn archive_gz_tarball(root: &Path, target: &Path) -> Result<()> {
    let f = File::create(target)?;
    let builder = build_tarball_stream(GzEncoder::new(f, Compression::best()), root)?;
    builder.into_inner()?.finish()?.sync_all()?;

    Ok(())
}

fn build_tarball_stream<W: Write>(stream: W, root: &Path) -> Result<Builder<W>, anyhow::Error> {
    let mut builder = Builder::new(stream);
    builder.mode(tar::HeaderMode::Complete);
    builder.follow_symlinks(false);
    builder.append_dir_all(".", root)?;
    builder.finish()?;

    Ok(builder)
}

/// Make a squashfs (xz compressed)
pub fn archive_squashfs(root: &Path, target: &Path, threads: u32) -> Result<()> {
    let output = Command::new("mksquashfs")
        .arg(root)
        .arg(target)
        .arg("-comp")
        .arg("xz")
        .arg("-processors")
        .arg(threads.to_string())
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
