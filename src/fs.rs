use anyhow::{Result, anyhow};
use flate2::Compression;
use flate2::write::GzEncoder;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use liblzma::stream::{Filters, LzmaOptions, MtStreamBuilder, Stream};
use liblzma::write::XzEncoder;
use nix::fcntl::{OFlag, open};
use nix::sys::stat::{FchmodatFlags, Mode, fchmodat};
use nix::unistd::{close, sync};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::{
    fs::{File, create_dir_all, write},
    io::Read,
};
use tar::Builder;

use crate::tar_dir_size::get_tar_dir_size;

const LZMA_PRESET_EXTREME: u32 = 1 << 31;

pub enum MirrorOrSourceList<'a> {
    Mirror { mirror: &'a str, branch: &'a str },
    SourceList(&'a Path),
}

pub fn bootstrap_apt(root: &Path, m: MirrorOrSourceList<'_>) -> Result<()> {
    create_dir_all(root.join("var/lib/dpkg"))?;
    create_dir_all(root.join("etc/apt"))?;
    create_dir_all(root.join("var/lib/apt/lists"))?;
    write(root.join("etc/locale.conf"), b"LANG=C.UTF-8\n")?;
    write(root.join("etc/shadow"), b"root:x:1:0:99999:7:::\n")?;

    match m {
        MirrorOrSourceList::Mirror { mirror, branch } => {
            write(
                root.join("etc/apt/sources.list"),
                format!("deb {} {} main\n", mirror, branch),
            )?;
        }
        MirrorOrSourceList::SourceList(path) => {
            fs::copy(path, root.join("etc/apt/sources.list"))?;
        }
    }

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
pub fn archive_xz_tarball(
    root: &Path,
    target: &Path,
    threads: u32,
    no_progressbar: bool,
) -> Result<()> {
    let f = File::create(target)?;
    let xz = build_xz_encoder(threads)?;

    let pb = create_progress_bar(get_tar_dir_size(root, true, false, 512)?, no_progressbar)?;

    let builder = build_tarball_stream(pb.wrap_write(XzEncoder::new_stream(f, xz)), root)?;

    // into_inner 步骤包含了 finish() 的调用
    builder.into_inner()?;
    sync();

    Ok(())
}

/// Make a tarball (gz compressed)
pub fn archive_gz_tarball(root: &Path, target: &Path, no_progressbar: bool) -> Result<()> {
    let f = File::create(target)?;

    let pb = create_progress_bar(get_tar_dir_size(root, true, false, 512)?, no_progressbar)?;

    let builder =
        build_tarball_stream(pb.wrap_write(GzEncoder::new(f, Compression::best())), root)?;

    builder.into_inner()?;
    sync();

    Ok(())
}

fn create_progress_bar(size: u64, no_progressbar: bool) -> Result<ProgressBar> {
    let pb = ProgressBar::new(size).with_style(ProgressStyle::with_template(
        "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} {bytes_per_sec}",
    )?);

    if no_progressbar {
        pb.set_draw_target(ProgressDrawTarget::hidden());
    }

    Ok(pb)
}

fn build_tarball_stream<W: Write>(stream: W, root: &Path) -> Result<Builder<W>, anyhow::Error> {
    let mut builder = Builder::new(stream);
    builder.sparse(false); // otherwise some docker version may complain: Unhandled tar header type 83
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
