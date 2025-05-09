use std::{
    collections::HashSet,
    fs::File,
    io::{BufWriter, Read, Write},
    path::Path,
};

use anyhow::{Result, anyhow};
use ar::Archive as ArArchive;
use liblzma::read::XzDecoder;
use serde::Deserialize;
use tar::Archive as TarArchive;
use tempfile::NamedTempFile;
use zstd::Decoder;

use crate::solv::PackageMeta;

const BOOTSTRAP_PACK: &[u8] = include_bytes!("../assets/etc-bootstrap.tar.xz");
const INSTALL_SCRIPT_TPL: &str = include_str!("../assets/bootstrap.sh");
const CLEANUP_SCRIPT: &[u8] = include_bytes!("../assets/cleanup.sh");

#[derive(Deserialize)]
pub struct Config {
    #[serde(rename = "stub-packages")]
    pub stub_packages: Vec<String>,
    #[serde(rename = "base-packages")]
    pub base_packages: Vec<String>,
}

#[inline]
pub fn decompress_tar_xz<R: Read>(reader: R, target: &Path) -> Result<()> {
    let decompress = XzDecoder::new(reader);
    let mut tar_processor = TarArchive::new(decompress);
    tar_processor.set_unpack_xattrs(true);
    tar_processor.set_preserve_permissions(true);
    tar_processor.unpack(target)?;

    Ok(())
}

#[inline]
pub fn decompress_tar_zst<R: Read>(reader: R, target: &Path) -> Result<()> {
    let decompress = Decoder::new(reader)?;
    let mut tar_processor = TarArchive::new(decompress);
    tar_processor.set_unpack_xattrs(true);
    tar_processor.set_preserve_permissions(true);
    tar_processor.unpack(target)?;

    Ok(())
}

pub fn extract_deb<R: Read>(reader: R, target: &Path) -> Result<()> {
    let mut deb = ArArchive::new(reader);
    while let Some(entry) = deb.next_entry() {
        if entry.is_err() {
            continue;
        }
        let entry = entry.unwrap();
        match entry.header().identifier() {
            b"data.tar.xz" => {
                decompress_tar_xz(entry, target)?;
                return Ok(());
            }
            b"data.tar.zst" => {
                decompress_tar_zst(entry, target)?;
                return Ok(());
            }
            _ => continue,
        }
    }

    Err(anyhow!("data archive not found or format unsupported"))
}

pub fn read_config<P: AsRef<Path>>(path: P) -> Result<Config> {
    let mut f = File::open(path)?;
    let mut content = String::new();
    content.reserve(4096);
    f.read_to_string(&mut content)?;
    let config = toml::from_str(&content)?;

    Ok(config)
}

pub fn extract_bootstrap_pack(target: &Path) -> Result<()> {
    let reader = std::io::Cursor::new(BOOTSTRAP_PACK);
    decompress_tar_xz(reader, target)?;

    Ok(())
}

fn generate_dpkg_install_script(packages: &[String]) -> String {
    let mut package_list = String::new();
    for package in packages {
        package_list.push_str(&format!("'{}' ", package));
    }

    INSTALL_SCRIPT_TPL.replacen("{}", &package_list, 1)
}

pub fn generate_apt_extended_state(
    target: &Path,
    manual_pkgs: &[String],
    all_packages: &[PackageMeta],
    main_arch: &str,
) -> Result<()> {
    let extended_state = File::create(target.join("var/lib/apt/extended_states"))?;
    let mut extended_state = BufWriter::new(extended_state);
    let mut manual_installed = HashSet::new();

    for p in manual_pkgs {
        manual_installed.insert(p);
    }

    for pkg in all_packages {
        if manual_installed.contains(&pkg.name) {
            continue;
        }
        writeln!(
            &mut extended_state,
            "Package: {}\nArchitecture: {}\nAuto-Installed: 1\n",
            pkg.name, main_arch
        )?;
    }

    Ok(())
}

pub fn write_install_script(
    packages: &[String],
    cleanup: bool,
    target: &Path,
) -> Result<NamedTempFile> {
    let mut f = NamedTempFile::new_in(target)?;
    f.write_all(generate_dpkg_install_script(packages).as_bytes())?;
    if cleanup {
        f.write_all(CLEANUP_SCRIPT)?;
    }

    Ok(f)
}
