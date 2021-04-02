use anyhow::{anyhow, Result};
use rayon::prelude::*;
use reqwest::blocking::Client;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::{fs::File, path::PathBuf};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use url::Url;

use crate::{fs::sha256sum, solv::PackageMeta};

fn sha256sum_file(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;

    sha256sum(&mut f)
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

pub fn fetch_manifests(
    client: &Client,
    mirror: &str,
    branch: &str,
    arches: &[&str],
    root: &Path,
) -> Result<Vec<String>> {
    let manifests = Arc::new(Mutex::new(Vec::new()));
    let manifests_clone = manifests.clone();
    arches.par_iter().try_for_each(move |arch| -> Result<()> {
        let url = format!("{}/dists/{}/main/binary-{}/Packages", mirror, branch, arch);
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

    Ok(Arc::try_unwrap(manifests).unwrap().into_inner().unwrap())
}

pub fn batch_download(pkgs: &[PackageMeta], mirror: &str, root: &Path) -> Result<()> {
    let client = make_new_client()?;
    let total = pkgs.len();
    let count = AtomicUsize::new(0);
    let error = AtomicBool::new(false);
    pkgs.par_iter().for_each_init(
        move || client.clone(),
        |client, pkg| {
            let filename = PathBuf::from(pkg.path.clone());
            count.fetch_add(1, Ordering::SeqCst);
            println!(
                "[{}/{}] Downloading {}...",
                count.load(Ordering::SeqCst),
                total,
                pkg.name
            );
            if let Some(filename) = filename.file_name() {
                let path = root.join(filename);
                if fetch_url(client, &format!("{}/{}", mirror, pkg.path), &path).is_err() {
                    error.store(true, Ordering::SeqCst);
                    return;
                }
                println!(
                    "[{}/{}] Verifying {}...",
                    count.load(Ordering::SeqCst),
                    total,
                    pkg.name
                );
                if sha256sum_file(&path).is_err() {
                    error.store(true, Ordering::SeqCst);
                    return;
                }
            } else {
                error.store(true, Ordering::SeqCst);
            }
        },
    );

    if error.load(Ordering::SeqCst) {
        return Err(anyhow!("Unable to download files"));
    }

    Ok(())
}
