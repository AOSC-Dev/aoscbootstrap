use anyhow::{anyhow, Context, Result};
use rayon::prelude::*;
use reqwest::blocking::Client;
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

use crate::{fs::sha256sum, solv::PackageMeta};

fn sha256sum_file(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;

    sha256sum(&mut f)
}

pub(crate) fn sha256sum_file_tag(path: &Path) -> Result<()> {
    let mut f = File::create(format!("{}.sha256sum", path.to_string_lossy()))?;
    f.write_all(format!("{} *{}", sha256sum_file(path)?, path.to_string_lossy()).as_bytes())?;

    Ok(())
}

pub fn make_new_client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent("Wget/1.20.3 (linux-gnu)")
        .build()?)
}

pub fn fetch_url(client: &Client, url: &str, path: &Path) -> Result<()> {
    let mut f = File::create(path)?;
    let mut resp = client.get(url).send()?;
    resp.error_for_status_ref()?;
    resp.copy_to(&mut f)?;

    Ok(())
}

#[inline]
fn combination<'a, 'b>(a: &'a [&str], b: &'b [&str]) -> Vec<(&'a str, &'b str)> {
    let mut ret = Vec::new();
    for i in a {
        for j in b {
            ret.push((*i, *j));
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
    comps: &[&str],
    root: &Path,
) -> Result<Vec<String>> {
    let manifests = Arc::new(Mutex::new(Vec::new()));
    let manifests_clone = manifests.clone();
    let manifests_clone_2 = manifests.clone();
    let combined = combination(arches, comps);
    combined
        .par_iter()
        .try_for_each(move |(arch, comp)| -> Result<()> {
            let url = format!("{}/dists/{}/{}/binary-{}/Packages", mirror, branch, comp, arch);
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
        let url = format!("{}/dists/{}/InRelease", mirror, topic);

        let inrelease = client.get(&url).send()?.error_for_status()?.bytes()?;
        let inrelease = String::from_utf8_lossy(&inrelease);
        let inrelease = oma_refresh::verify::verify(&inrelease, None, "/")?;
        let inrelease = oma_debcontrol::parse_str(&inrelease).map_err(|e| anyhow!("{e}"))?;
        let inrelease = inrelease.first().context("InRelease is empty")?;

        let sha256 = &inrelease
            .fields
            .iter()
            .find(|x| x.name == "SHA256")
            .context("Illage InRelease")?
            .value;

        for i in sha256.split('\n') {
            let name = i
                .split_ascii_whitespace()
                .next_back()
                .context("Illage InRelease")?;

            if name.ends_with("/Packages") {
                let url = format!("{}/dists/{}/{}", mirror, topic, name);
                let parsed = Url::parse(&url)?;
                let manifest_name =
                    parsed.host_str().unwrap_or_default().to_string() + parsed.path();
                let manifest_name = manifest_name.replace('/', "_");

                fetch_url(client, &url, &root.join(name))?;
                manifests_clone_2.lock().unwrap().push(manifest_name);
            }
        }

        Ok(())
    })?;

    Ok(Arc::try_unwrap(manifests).unwrap().into_inner().unwrap())
}

pub fn batch_download(pkgs: &[PackageMeta], mirror: &str, root: &Path) -> Result<()> {
    for i in 1..=3 {
        if batch_download_inner(pkgs, mirror, root).is_ok() {
            return Ok(());
        }
        eprintln!("[{}/3] Retrying ...", i);
        sleep(Duration::from_secs(2));
    }

    Err(anyhow!("Failed to download packages"))
}

fn batch_download_inner(pkgs: &[PackageMeta], mirror: &str, root: &Path) -> Result<()> {
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
