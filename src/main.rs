mod fs;
mod guest;
mod install;
mod network;
mod solv;

use anyhow::{anyhow, Context, Result};
use bytesize::ByteSize;
use clap::Parser;
use owo_colors::colored::*;
use solv::PackageMeta;
use std::{
    fs::File,
    io::{BufRead, BufReader, Write},
    path::Path,
};

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
    #[clap(short, long)]
    scripts: Option<Vec<String>>,
    /// CPU architectures to consider
    #[clap(short, long)]
    arch: Vec<String>,
    /// Extra packages to include
    #[clap(short, long)]
    include: Vec<String>,
    /// Extra packages to include (read from files)
    #[clap(short = 'f', long = "include-files")]
    include_files: Option<Vec<String>>,
    /// Only downloads packages, do not progress further
    #[clap(short = 'g', long = "download-only")]
    download_only: bool,
    /// Only finishes stage 1, do not progress further
    #[clap(short = '1', long = "stage1-only")]
    stage1: bool,
    /// Add additional components
    #[clap(short = 'm', long)]
    comps: Vec<String>,
    /// Limit the number of parallel jobs
    #[clap(short = 'j', long)]
    jobs: Option<usize>,
    /// Export a xz compressed tar archive
    #[clap(long = "export-tar")]
    tar: Option<String>,
    /// Export a xz compressed squashfs archive
    #[clap(long = "export-squashfs")]
    squashfs: Option<String>,
    /// Branch to use
    branch: String,
    /// Path to the destination
    target: String,
    /// Mirror to be used
    #[clap(default_value = DEFAULT_MIRROR)]
    mirror: String,
}

/// AOSC OS specific architecture mapping for ppc64
#[cfg(target_arch = "powerpc64")]
#[inline]
fn get_arch_name() -> Option<&'static str> {
    let mut endian: libc::c_int = -1;
    let result;
    unsafe {
        result = libc::prctl(libc::PR_GET_ENDIAN, &mut endian as *mut libc::c_int);
    }
    if result < 0 {
        return None;
    }
    match endian {
        libc::PR_ENDIAN_LITTLE | libc::PR_ENDIAN_PPC_LITTLE => Some("ppc64el"),
        libc::PR_ENDIAN_BIG => Some("ppc64"),
        _ => None,
    }
}

/// AOSC OS specific architecture mapping table
#[cfg(not(target_arch = "powerpc64"))]
#[inline]
fn get_arch_name() -> Option<&'static str> {
    use std::env::consts::ARCH;

    match ARCH {
        "x86_64" => Some("amd64"),
        "x86" => Some("i486"),
        "powerpc" => Some("powerpc"),
        "aarch64" => Some("arm64"),
        "mips64" => Some("loongson3"),
        _ => None,
    }
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
        let filename = Path::new(&package.path)
            .file_name()
            .ok_or_else(|| anyhow!("Unable to determine package filename"))?;
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
    let mut packages = Vec::new();
    packages.reserve(1024);

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
    let f = File::open(path.as_ref())?;
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
        output.push(
            Path::new(&package.path)
                .file_name()
                .ok_or_else(|| anyhow!("Unable to determine package filename"))?
                .to_string_lossy()
                .to_string(),
        );
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
            let mut f = File::open(&s)?;
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
        return Err(anyhow!("It's not possible to continue, disk space not enough: {} required, but only {} is available. You need at least {} more.", ByteSize::kb(required), ByteSize::b(available),  ByteSize::kb(required - (available / 1024))));
    }

    Ok(())
}

fn do_stage1(
    st: solv::Transaction,
    target_path: &Path,
    mirror: &String,
    args: &Args,
    archive_path: std::path::PathBuf,
    all_packages: Vec<PackageMeta>,
) -> Result<Option<tempfile::NamedTempFile>> {
    check_disk_usage(st.get_size_change() as u64, target_path)?;
    let stub_install = st.create_metadata()?;
    eprintln!("Stage 1: Creating filesystem skeleton ...");
    std::fs::create_dir_all(target_path.join("dev"))?;
    fs::bootstrap_apt(target_path, mirror, &args.branch).context("when preparing apt files")?;
    install::extract_bootstrap_pack(target_path).context("when extracting base files")?;
    fs::make_device_nodes(target_path)?;
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
    target: &String,
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
    if let Some(ref tar) = args.tar {
        eprintln!("Compressing the tarball, please wait patiently ...");
        let path = Path::new(&tar);
        fs::archive_tarball(target_path, path, threads as u32)?;
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

fn main() {
    let args = Args::parse();
    let target = &args.target;
    let mirror = &args.mirror;
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
    let archive_path = target_path.join("var/cache/apt/archives");
    let threads = args.jobs.unwrap_or_else(|| num_cpus::get());
    if target_path.exists() {
        panic!(
            "{}",
            "Target already exists. Please remove it first."
                .red()
                .bold()
        );
    }
    if let Some(jobs) = args.jobs {
        std::env::set_var("RAYON_NUM_THREADS", jobs.to_string());
    }
    let mut extra_packages = args.include.clone();
    if let Some(ref extra_files) = args.include_files {
        let extras = collect_packages_from_lists(&extra_files).unwrap();
        eprintln!(
            "Read {} extra packages from the lists.",
            extras.len().cyan().bold()
        );
        extra_packages.extend(extras);
    }
    // append the `noarch` architecture if it does not exist.
    // this is to avoid confusing issues with dependency resolving.
    if !arches.contains(&"all".to_string()) {
        arches.push("all".to_string());
    }
    let mut comps = args.comps.clone();
    comps.push("main".to_string());
    let comps_str = comps.iter().map(|s| s.as_str()).collect::<Vec<_>>();

    std::fs::create_dir_all(target_path.join("var/lib/apt/lists")).unwrap();
    std::fs::create_dir_all(&archive_path).unwrap();
    eprintln!("Downloading manifests ...");
    let arches = arches.iter().map(|a| a.as_str()).collect::<Vec<_>>();
    let manifests = network::fetch_manifests(
        &client,
        mirror,
        &args.branch,
        &arches,
        &comps_str,
        target_path,
    )
    .unwrap();
    let mut paths = Vec::new();
    for p in manifests {
        paths.push(target_path.join("var/lib/apt/lists").join(p));
    }
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
        ByteSize::kb(t.get_size_change().abs() as u64).cyan().bold()
    );
    check_disk_usage(t.get_size_change() as u64, target_path).unwrap();
    eprintln!("Downloading packages ...");
    network::batch_download(&all_packages, mirror, &archive_path).unwrap();
    nix::unistd::sync();
    if args.download_only {
        eprintln!("{}", "Download finished.".green().bold());
        return;
    }

    let st = solv::calculate_deps(&mut pool, &config.stub_packages).unwrap();
    let script =
        match do_stage1(st, target_path, mirror, &args, archive_path, all_packages).unwrap() {
            Some(value) => value,
            None => return,
        };

    do_stage2(t, target_path, script, target, &args, threads).unwrap();
}
