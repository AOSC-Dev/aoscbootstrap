use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};

use anyhow::{anyhow, Result};
use ar::Archive as ArArchive;
use serde::Deserialize;
use tar::Archive as TarArchive;
use tempfile::NamedTempFile;
use toml;
use xz2::read::XzDecoder;

const BOOTSTRAP_PACK: &[u8] = include_bytes!("../assets/etc-bootstrap.tar.xz");

const INSTALL_SCRIPT_TPL: &'static str = r#"#!/bin/bash
count=0
PACKAGES=(
{}
)
length=${#PACKAGES[@]}
for p in ${PACKAGES[@]}; do
count=$((count+1))
echo "[$count/$length] Installing ${p}..."
dpkg --force-depends --unpack "/var/cache/apt/archives/${p}"
done
count_c=1;length_c=$(dpkg -l | grep -c 'iU')
function dpkg_progress () {
    while read action step package; do
if [ "$action" = 'processing:' -a "$step" = 'configure:' ]; then
echo "[$count_c/$length_c] Configuring $package...";count_c=$(( $count_c + 1 ))
fi
    done
}
{ dpkg --status-fd=7 --configure --pending --force-configure-any --force-depends 7>&1 >&8 | dpkg_progress; } 8>&1
"#;

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

pub fn extract_deb<R: Read>(reader: R, target: &Path) -> Result<()> {
    let mut deb = ArArchive::new(reader);
    while let Some(entry) = deb.next_entry() {
        if entry.is_err() {
            continue;
        }
        let entry = entry.unwrap();
        if entry.header().identifier() == b"data.tar.xz" {
            decompress_tar_xz(entry, target)?;
            return Ok(());
        }
    }

    Err(anyhow!("data archive not found or format unsupported"))
}

pub fn read_config<P: AsRef<Path>>(path: P) -> Result<Config> {
    let mut f = File::open(path)?;
    let mut content = Vec::new();
    content.reserve(4096);
    f.read_to_end(&mut content)?;
    let config = toml::from_slice(&content)?;

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

pub fn write_install_script(packages: &[String], target: &Path) -> Result<NamedTempFile> {
    let mut f = NamedTempFile::new_in(target)?;
    f.write_all(generate_dpkg_install_script(packages).as_bytes())?;

    Ok(f)
}
