mod fs;
mod guest;
mod install;
mod network;
mod solv;
mod tar_dir_size;
mod topics;

use anyhow::{Context, Result, anyhow};
use bytesize::ByteSize;
use clap::Parser;
use libaosc::arch::get_arch_name;
use network::{Mirror, SelectMirror};
use nix::unistd::Uid;
use oma_fetch::Event;
use oma_refresh::db::OmaRefresh;
use owo_colors::colored::*;
use reqwest::ClientBuilder;
use solv::PackageMeta;
use std::{
    borrow::Cow,
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::exit,
};
use topics::{Topic, fetch_topics, filter_topics};

const DEFAULT_MIRROR: &str = "https://repo.aosc.io/debs";

#[derive(Parser, Debug)]
#[clap(about, version, author)]
struct Args {
    /// Sets a custom config file
    #[clap(short, long)]
    config: String,
    /// Clean up (factory-reset) the bootstrapped environment
    #[clap(short = 'x', long)]
    clean: bool,
    /// Run specified custom scripts during stage 2 (after clean up, if any)
    #[clap(short, long, num_args = 1..)]
    scripts: Option<Vec<String>>,
    /// CPU architectures to consider
    #[clap(short, long, num_args = 1..)]
    arch: Vec<String>,
    /// Extra packages to include
    #[clap(short, long, num_args = 1..)]
    include: Vec<String>,
    /// Extra packages to include (read from files)
    #[clap(short = 'f', long = "include-files", num_args = 1..)]
    include_files: Option<Vec<String>>,
    /// Only downloads packages, do not progress further
    #[clap(short = 'g', long = "download-only")]
    download_only: bool,
    /// Only finishes stage 1, do not progress further
    #[clap(short = '1', long = "stage1-only")]
    stage1: bool,
    /// Add additional components
    #[clap(long, num_args = 1.., conflicts_with = "sources_list")]
    comps: Option<Vec<String>>,
    /// Limit the number of parallel jobs
    #[clap(short = 'j', long)]
    jobs: Option<usize>,
    /// Allow existing target directory
    #[clap(long = "force", default_value = "false")]
    force: bool,
    /// Export a xz compressed tar archive
    #[clap(long = "export-tar-xz")]
    tar_xz: Option<String>,
    /// Export a gz compressed tar archive
    #[clap(long = "export-tar-gz")]
    tar_gz: Option<String>,
    /// Export a xz compressed squashfs archive
    #[clap(long = "export-squashfs")]
    squashfs: Option<String>,
    /// Path to the destination
    target: String,
    /// Branch to use
    #[clap(conflicts_with = "sources_list")]
    branch: Option<String>,
    /// Mirror to be used
    #[clap(conflicts_with = "sources_list", default_value = DEFAULT_MIRROR)]
    mirror: Option<String>,
    /// Include topics
    #[clap(short, long, num_args = 1..)]
    topics: Option<Vec<String>>,
    /// Disable Progress bar
    #[clap(long)]
    no_progressbar: bool,
    /// Use sources.list to fetch packages
    #[clap(long)]
    sources_list: Option<PathBuf>,
}

fn get_default_arch() -> Vec<String> {
    let mut arches = vec!["all".to_string()];
    if let Some(arch) = get_arch_name() {
        arches.push(arch.to_string());
    }

    arches
}

fn extract_packages(packages: &[PackageMeta], target: &Path, archive_path: &Path) -> Result<()> {
    let mut count = 0usize;
    for package in packages {
        count += 1;
        let filename = package.file_name();
        eprintln!(
            "[{}/{}] Extracting {} ...",
            count,
            packages.len(),
            package.name.cyan()
        );
        let f = File::open(archive_path.join(filename))?;
        install::extract_deb(f, target)?;
    }

    Ok(())
}

fn collect_packages_from_lists(paths: &[String]) -> Result<Vec<String>> {
    let mut packages = Vec::with_capacity(1024);

    for path in paths {
        collect_packages_from_list(path, &mut packages, 0)?;
    }

    Ok(packages)
}

fn collect_packages_from_list<P: AsRef<Path>>(
    path: P,
    packages: &mut Vec<String>,
    depth: usize,
) -> Result<()> {
    if depth > 32 {
        return Err(anyhow!("Recursion limit exceeded. Is there a loop?"));
    }
    let f = File::open(path.as_ref())
        .context(format!("Failed to open file: {}", path.as_ref().display()))?;
    let reader = BufReader::new(f);
    for line in reader.lines() {
        let line = line?;
        if let Some(inc) = line.strip_prefix("%include ") {
            let real_path = path.as_ref().canonicalize()?;
            let real_path = real_path.parent().ok_or_else(|| anyhow!("Invalid path"))?;
            collect_packages_from_list(real_path.join(inc.trim()), packages, depth + 1)?;
        }
        // skip comment
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        // trim whitespace
        let trimmed = line.trim();
        packages.push(trimmed.to_owned());
    }

    Ok(())
}

#[inline]
fn collect_filenames(packages: &[PackageMeta]) -> Result<Vec<String>> {
    let mut output = Vec::new();
    for package in packages {
        output.push(package.file_name());
    }

    Ok(output)
}

fn include_extra_scripts<W: Write>(
    extra_scripts: &Option<Vec<String>>,
    output: &mut W,
) -> Result<()> {
    if let Some(scripts) = extra_scripts {
        eprintln!("Including {} extra scripts ...", scripts.len().bold());
        output.write_all(b"\necho 'Running additional scripts ...';")?;
        for s in scripts {
            let mut f = File::open(s)?;
            output.write_all(format!("\n# === {}\n", &s).as_bytes())?;
            std::io::copy(&mut f, output)?;
        }
    }

    Ok(())
}

fn check_disk_usage(required: u64, target: &Path) -> Result<()> {
    use fs3::available_space;

    let available = available_space(target)?;
    if (available / 1024) < required {
        return Err(anyhow!(
            "It's not possible to continue, disk space not enough: {} required, but only {} is available. You need at least {} more.",
            ByteSize::kb(required),
            ByteSize::b(available),
            ByteSize::kb(required - (available / 1024))
        ));
    }

    Ok(())
}

fn do_stage1(
    st: solv::Transaction,
    target_path: &Path,
    args: &Args,
    archive_path: std::path::PathBuf,
    all_packages: Vec<PackageMeta>,
    topics: Vec<Topic>,
) -> Result<Option<tempfile::NamedTempFile>> {
    check_disk_usage(st.get_size_change() as u64, target_path)?;
    let stub_install = st.create_metadata()?;
    eprintln!("Stage 1: Creating filesystem skeleton ...");
    std::fs::create_dir_all(target_path.join("dev"))?;
    if let Some((mirror, branch)) = args.mirror.as_ref().zip(args.branch.as_ref()) {
        fs::bootstrap_apt(
            target_path,
            fs::MirrorOrSourceList::Mirror { mirror, branch },
        )
        .context("when preparing apt files")?;
    } else {
        fs::bootstrap_apt(
            target_path,
            fs::MirrorOrSourceList::SourceList(args.sources_list.as_ref().unwrap()),
        )
        .context("when preparing apt files")?;
    }
    topics::save_topics(target_path, topics)?;
    install::extract_bootstrap_pack(target_path).context("when extracting base files")?;
    eprintln!("Stage 1: Extracting packages ...");
    extract_packages(&stub_install, target_path, &archive_path)?;
    let names: Vec<String> = collect_filenames(&all_packages)?;
    let mut script = install::write_install_script(&names, args.clean, target_path)?;
    include_extra_scripts(&args.scripts, &mut script).context("when including extra scripts")?;
    nix::unistd::sync();
    if args.stage1 {
        let (_, path) = script.keep().context("when persisting the script file")?;
        eprintln!("Stage 1 finished.");
        eprintln!(
            "If you want to continue stage 2, you can run `bash {:?}` inside the container.",
            path.file_name().unwrap().underline()
        );
        return Ok(None);
    }

    Ok(Some(script))
}

fn do_stage2(
    t: solv::Transaction,
    target_path: &Path,
    script: tempfile::NamedTempFile,
    target: &str,
    args: &Args,
    threads: usize,
) -> Result<()> {
    eprintln!("Stage 2: Installing packages ...");
    check_disk_usage(t.get_size_change() as u64, target_path)?;
    let script_file = script.path().file_name().unwrap().to_string_lossy();
    guest::run_in_guest(target, &["/usr/bin/bash", "-e", &script_file])
        .context("when running install scripts in the container")?;
    drop(script);
    nix::unistd::sync();
    eprintln!("{}", "Stage 2 finished.\nBase system ready!".green().bold());
    if let Some(ref xz) = args.tar_xz {
        eprintln!("Compressing the xz tarball, please wait patiently ...");
        let path = Path::new(&xz);
        fs::archive_xz_tarball(target_path, path, threads as u32, args.no_progressbar)?;
        network::sha256sum_file_tag(path)?;
        eprintln!("Tarball available at {}", path.display().cyan());
    }
    if let Some(ref gz) = args.tar_gz {
        eprintln!("Compressing the gz tarball, please wait patiently ...");
        let path = Path::new(&gz);
        fs::archive_gz_tarball(target_path, path, args.no_progressbar)?;
        network::sha256sum_file_tag(path)?;
        eprintln!("Tarball available at {}", path.display().cyan());
    }
    if let Some(ref squashfs) = args.squashfs {
        eprintln!("Compressing the squashfs, please wait patiently ...");
        let path = Path::new(&squashfs);
        fs::archive_squashfs(target_path, path, threads as u32)?;
        network::sha256sum_file_tag(path)?;
        eprintln!("SquashFS available at {}", path.display().cyan());
    }

    Ok(())
}

enum Manifests {
    Single(Vec<String>),
    List(HashMap<String, String>),
}

impl Manifests {
    fn paths(&self, target_path: &Path) -> Vec<PathBuf> {
        match self {
            Manifests::Single(items) => items
                .iter()
                .map(|p| target_path.join("var/lib/apt/lists").join(p))
                .collect(),
            Manifests::List(hash_map) => hash_map
                .iter()
                .map(|(_, p)| target_path.join("var/lib/apt/lists").join(p))
                .collect(),
        }
    }
}

fn main() {
    let args = Args::parse();

    if !Uid::current().is_root() {
        eprintln!("aoscbootstrap must be run as root.");
        exit(1);
    }

    let target = &args.target;
    let mirror = &args.mirror;
    if args.squashfs.is_some() && which::which("mksquashfs").is_err() {
        eprintln!("Cannot find mksquashfs binary!");
        exit(1)
    }
    let mut arches = if args.arch.is_empty() {
        get_default_arch()
    } else {
        args.arch.clone()
    };
    let config_path = &args.config;
    let config = install::read_config(config_path)
        .context(format!("when reading configuration file '{}'", config_path))
        .unwrap();
    let client = network::make_new_client().unwrap();
    let target_path = Path::new(target);
    let force = args.force;
    let archive_path = target_path.join("var/cache/apt/archives");
    let threads = args.jobs.unwrap_or_else(num_cpus::get);
    if target_path.exists() && !force {
        panic!(
            "{}",
            "Target already exists. Please remove it first."
                .red()
                .bold()
        );
    }
    if let Some(jobs) = args.jobs {
        rayon::ThreadPoolBuilder::new()
            .num_threads(jobs)
            .build_global()
            .unwrap();
    }
    let mut extra_packages = args.include.clone();
    if let Some(ref extra_files) = args.include_files {
        let extras = collect_packages_from_lists(extra_files).unwrap();
        eprintln!(
            "Read {} extra packages from the lists.",
            extras.len().cyan().bold()
        );
        extra_packages.extend(extras);
        extra_packages.retain(|x| !x.starts_with("%include "));
    }
    // append the `noarch` architecture if it does not exist.
    // this is to avoid confusing issues with dependency resolving.
    if !arches.contains(&"all".to_string()) {
        arches.push("all".to_string());
    }

    let comps = if let Some(comps) = &args.comps {
        let mut comps = comps.to_owned();
        comps.push("main".to_string());
        Some(comps)
    } else {
        Some(vec!["main".to_string()])
    };

    std::fs::create_dir_all(target_path.join("var/lib/apt/lists")).unwrap();
    std::fs::create_dir_all(&archive_path).unwrap();
    eprintln!("Downloading manifests ...");
    let arches = arches.iter().map(|a| a.as_str()).collect::<Vec<_>>();

    let topics = if let Some(ref t) = args.topics {
        Cow::Borrowed(t)
    } else {
        Cow::Owned(vec![] as Vec<String>)
    };
    let all_topics = fetch_topics().unwrap();
    let filtered = if !topics.is_empty() {
        filter_topics(topics.to_vec(), all_topics).unwrap()
    } else {
        Vec::new()
    };

    let manifests = match &args.sources_list {
        Some(path) => Manifests::List(fetch_manifest_from_sources_list(
            target_path,
            &arches,
            vec![path.to_path_buf()],
        )),
        None => Manifests::Single(
            network::fetch_manifests(
                &client,
                mirror.as_ref().unwrap(),
                args.branch.as_ref().unwrap(),
                &topics,
                &arches,
                comps.as_ref().unwrap(),
                target_path,
            )
            .unwrap(),
        ),
    };

    let paths = manifests.paths(target_path);

    eprintln!("Resolving dependencies ...");
    let mut all_stages = config.stub_packages.clone();
    all_stages.extend(config.base_packages);
    all_stages.extend(extra_packages);

    let mut pool = solv::Pool::new();
    solv::populate_pool(&mut pool, &paths).unwrap();
    let t = solv::calculate_deps(&mut pool, &all_stages).unwrap();
    let all_packages = t.create_metadata().unwrap();

    eprintln!(
        "Total installed size: {}",
        ByteSize::kb(t.get_size_change().unsigned_abs())
            .cyan()
            .bold()
    );
    check_disk_usage(t.get_size_change() as u64, target_path).unwrap();
    eprintln!("Downloading packages ...");
    network::batch_download(
        &all_packages,
        &archive_path,
        if let Some(m) = &args.mirror {
            Mirror::Single(m)
        } else {
            Mirror::List(if let Manifests::List(map) = manifests {
                SelectMirror::new(
                    map.into_iter()
                        .map(|(url, file_name)| {
                            (url, target_path.join("var/lib/apt/lists").join(file_name))
                        })
                        .collect(),
                )
                .context("Failed to read package manifests")
                .unwrap()
            } else {
                unreachable!()
            })
        },
    )
    .unwrap();
    nix::unistd::sync();
    if args.download_only {
        eprintln!("{}", "Download finished.".green().bold());
        return;
    }

    let st = solv::calculate_deps(&mut pool, &config.stub_packages).unwrap();
    let main_arch = arches
        .iter()
        .find(|a| **a != "all")
        .expect("Did not find the main architecture");
    install::generate_apt_extended_state(target_path, &all_stages, &all_packages, main_arch)
        .expect("Unable to generate APT extended state");
    let script =
        match do_stage1(st, target_path, &args, archive_path, all_packages, filtered).unwrap() {
            Some(value) => value,
            None => return,
        };

    do_stage2(t, target_path, script, target, &args, threads).unwrap();
}

fn fetch_manifest_from_sources_list(
    target_path: &Path,
    arches: &[&str],
    paths: Vec<PathBuf>,
) -> HashMap<String, String> {
    let client = ClientBuilder::new()
        .user_agent("oma/1.14.514")
        .build()
        .unwrap();

    let mut map = HashMap::new();
    map.insert(
        "MetaKey".to_string(),
        "$(COMPONENT)/binary-$(ARCHITECTURE)/Packages".to_string(),
    );

    let lists = target_path.join("var/lib/apt/lists");
    let success_list = OmaRefresh::builder()
        .download_dir(lists.to_path_buf())
        .arch(arches.iter().find(|a| **a != "all").unwrap().to_string())
        .client(&client)
        .manifest_config(vec![map])
        .source("/".into())
        .topic_msg("")
        .sources_lists_paths(paths)
        .build()
        .start_blocking(async |e| {
            if let oma_refresh::db::Event::DownloadEvent(Event::Failed { file_name, error }) = e {
                eprintln!("Download file {file_name} with error: {error}");
            }
        })
        .unwrap();

    let mut names = HashMap::new();

    for i in success_list {
        if i.file_name.ends_with("_Packages") {
            names.insert(i.url.clone(), i.file_name.clone());
        }
    }

    names
}
