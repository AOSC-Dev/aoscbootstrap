use anyhow::{Context, Result, anyhow};
use libaosc::packages::Packages as PackagesManifest;
use rayon::prelude::*;
use reqwest::blocking::Client;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use std::{fs::File, io::Write};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use std::{
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    thread::sleep,
    time::Duration,
};
use url::Url;

use crate::DEFAULT_MIRROR;
use crate::{fs::sha256sum, solv::PackageMeta};

fn sha256sum_file(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;

    sha256sum(&mut f)
}

pub(crate) fn sha256sum_file_tag(path: &Path) -> Result<()> {
    let mut f = File::create(format!("{}.sha256sum", path.to_string_lossy()))?;
    f.write_all(
        format!(
            "{} *{}\n",
            sha256sum_file(path)?,
            path.file_name()
                .context("Failed to get file name")?
                .to_string_lossy()
        )
        .as_bytes(),
    )?;

    Ok(())
}

pub fn make_new_client() -> Result<Client> {
    Ok(Client::builder().user_agent("oma/1.14.514").build()?)
}

pub fn fetch_url(client: &Client, url: &str, path: &Path) -> Result<()> {
    let mut f = File::create(path)?;
    let mut resp = client.get(url).send()?;
    resp.error_for_status_ref()?;
    resp.copy_to(&mut f)?;

    Ok(())
}

#[inline]
fn combination<'a, 'b>(a: &'a [&str], b: &'b [String]) -> Vec<(&'a str, &'b str)> {
    let mut ret = Vec::new();
    for i in a {
        for j in b {
            ret.push((*i, j.as_str()));
        }
    }

    ret
}

pub fn fetch_manifests(
    client: &Client,
    mirror: &str,
    branch: &str,
    topics: &[String],
    arches: &[&str],
    comps: Vec<String>,
    root: &Path,
) -> Result<Vec<String>> {
    let manifests = Arc::new(Mutex::new(Vec::new()));
    let manifests_clone = manifests.clone();
    let manifests_clone_2 = manifests.clone();
    let combined = combination(arches, &comps);
    combined
        .par_iter()
        .try_for_each(move |(arch, comp)| -> Result<()> {
            let url = format!(
                "{}/dists/{}/{}/binary-{}/Packages",
                mirror, branch, comp, arch
            );
            let parsed = Url::parse(&url)?;
            let manifest_name = parsed.host_str().unwrap_or_default().to_string() + parsed.path();
            let manifest_name = manifest_name.replace('/', "_");

            fetch_url(
                client,
                &url,
                &root.join("var/lib/apt/lists").join(manifest_name.clone()),
            )?;
            manifests_clone.lock().unwrap().push(manifest_name);

            Ok(())
        })?;

    topics.par_iter().try_for_each(move |topic| -> Result<()> {
        // Always use AOSC OS Repo for topics
        let url = format!("{}/dists/{}/InRelease", DEFAULT_MIRROR, topic);

        let inrelease = client.get(&url).send()?.error_for_status()?.text()?;
        let inrelease = oma_repo_verify::verify_inrelease_by_sysroot(&inrelease, None, "/", false)?;
        let inrelease = oma_debcontrol::parse_str(&inrelease).map_err(|e| anyhow!("{e}"))?;
        let inrelease = inrelease.first().context("InRelease is empty")?;

        let sha256 = &inrelease
            .fields
            .iter()
            .find(|x| x.name == "SHA256")
            .context("Illage InRelease")?
            .value;

        for i in sha256.trim().lines() {
            let name = i
                .split_ascii_whitespace()
                .next_back()
                .context("Illage InRelease")?;

            if arches
                .iter()
                .any(|arch| name.ends_with(&format!("binary-{}/Packages", arch)))
            {
                let url = format!("{}/dists/{}/{}", DEFAULT_MIRROR, topic, name);
                let url = Url::parse(&url)?;
                let manifest_name = url.host_str().unwrap_or_default().to_string() + url.path();
                let manifest_name = manifest_name.replace('/', "_");

                fetch_url(
                    client,
                    url.as_str(),
                    &root.join("var/lib/apt/lists").join(manifest_name.clone()),
                )?;
                manifests_clone_2.lock().unwrap().push(manifest_name);
            }
        }

        Ok(())
    })?;

    Ok(Arc::try_unwrap(manifests).unwrap().into_inner().unwrap())
}

pub struct SelectMirror {
    url_pkgs: Vec<(String, PackagesManifest)>,
}

impl SelectMirror {
    pub fn new(url_path: Vec<(String, PathBuf)>) -> Result<Self> {
        let mut url_pkgs = vec![];
        for (url, p) in url_path {
            let s = fs::read_to_string(p)?;
            let manifest = PackagesManifest::from_str(&s)?;
            url_pkgs.push((url, manifest));
        }

        Ok(Self { url_pkgs })
    }

    pub fn mirror_url<'a>(&'a self, pkg: &PackageMeta) -> Option<&'a str> {
        for (url, manifest) in &self.url_pkgs {
            if manifest.0.iter().any(|p| {
                p.package == pkg.name && p.version == pkg.version && p.architecture == pkg.arch
            }) {
                let (s, _) = url.split_once("dists/")?;
                return Some(s);
            }
        }

        None
    }
}

pub enum Mirror<'a> {
    Single(&'a str),
    List(SelectMirror),
}

impl<'a> Mirror<'a> {
    fn mirror_url(&'a self, pkg: &PackageMeta) -> Option<&'a str> {
        match self {
            Mirror::Single(s) => Some(if pkg.in_topic { DEFAULT_MIRROR } else { s }),
            Mirror::List(s) => s.mirror_url(pkg),
        }
    }
}

pub fn batch_download(pkgs: &[PackageMeta], root: &Path, m: Mirror) -> Result<()> {
    for i in 1..=3 {
        if batch_download_inner(pkgs, root, &m).is_ok() {
            return Ok(());
        }
        eprintln!("[{}/3] Retrying ...", i);
        sleep(Duration::from_secs(2));
    }

    Err(anyhow!("Failed to download packages"))
}

fn batch_download_inner(pkgs: &[PackageMeta], root: &Path, m: &Mirror) -> Result<()> {
    let client = make_new_client()?;
    let total = pkgs.len() * 2;
    let count = AtomicUsize::new(0);
    let error = AtomicBool::new(false);
    pkgs.par_iter().for_each_init(
        move || client.clone(),
        |client, pkg| {
            let filename = pkg.file_name();
            count.fetch_add(1, Ordering::SeqCst);
            println!(
                "[{}/{}] Downloading {}...",
                count.load(Ordering::SeqCst),
                total,
                pkg.name
            );

            let path = root.join(filename);

            let mirror = match m.mirror_url(&pkg) {
                Some(m) => m,
                None => {
                    error.store(true, Ordering::SeqCst);
                    eprintln!("Download failed: {}: failed to get mirror", pkg.name);
                    return;
                }
            };

            if !path.is_file()
                && fetch_url(client, &format!("{}/{}", mirror, pkg.path), &path).is_err()
            {
                error.store(true, Ordering::SeqCst);
                eprintln!("Download failed: {}", pkg.name);
                return;
            }
            count.fetch_add(1, Ordering::SeqCst);
            println!(
                "[{}/{}] Verifying {}...",
                count.load(Ordering::SeqCst),
                total,
                pkg.name
            );
            if !sha256sum_file(&path)
                .map(|x| x == pkg.sha256)
                .unwrap_or(false)
            {
                std::fs::remove_file(path).ok();
                error.store(true, Ordering::SeqCst);
                eprintln!("Verification failed: {}", pkg.name);
            }
        },
    );

    if error.load(Ordering::SeqCst) {
        return Err(anyhow!("Unable to download files"));
    }

    Ok(())
}
