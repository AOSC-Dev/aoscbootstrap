mod ffi;
use std::path::PathBuf;

use anyhow::{bail, Result};
pub use ffi::{Pool, Queue, Repo, Solver, Transaction, SOLVER_FLAG_BEST_OBEY_POLICY};

#[derive(Clone, Debug)]
pub struct PackageMeta {
    pub name: String,
    pub version: String,
    pub sha256: String,
    pub path: String,
    pub arch: String,
    pub in_topic: bool,
}

impl PackageMeta {
    /// Return apt-style file name for this package
    pub fn file_name(&self) -> String {
        let package = &self.name;
        let version = &self.version.replace(':', "%3a");
        let arch = &self.arch;
        format!("{package}_{version}_{arch}.deb").replace("%2b", "+")
    }
}

/// Simulate the apt dependency resolution
pub fn calculate_deps(pool: &mut Pool, names: &[String]) -> Result<Transaction> {
    let mut q = Queue::new();
    for name in names {
        q = pool.match_package(name, q)?;
    }
    q.mark_all_for_install();
    let mut solver = Solver::new(pool);
    solver.set_flag(SOLVER_FLAG_BEST_OBEY_POLICY, 1)?;

    if let Err(e) = solver.solve(&mut q) {
        eprintln!("{e}");
        bail!("{}", solver.get_problems()?.join("\n"));
    }

    let trans = solver.create_transaction()?;
    trans.order(0);

    Ok(trans)
}

/// Populate the packages pool with metadata
pub fn populate_pool(pool: &mut Pool, paths: &[PathBuf]) -> Result<()> {
    let mut repo = Repo::new(pool, "stable")?;
    for path in paths {
        repo.add_debpackages(path)?;
    }
    pool.createwhatprovides();

    Ok(())
}
