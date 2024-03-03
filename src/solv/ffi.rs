use super::PackageMeta;
use anyhow::{anyhow, Result};
use hex::encode;
use libc::{c_char, c_int};
use libsolv_sys::ffi;
use std::{convert::TryInto, ffi::CStr, os::unix::ffi::OsStrExt, slice};
use std::{ffi::CString, path::Path, ptr::null_mut};

pub const SELECTION_NAME: c_int = 1 << 0;
pub const SELECTION_FLAT: c_int = 1 << 10;
pub const SELECTION_ADD: c_int = 1 << 28;

pub const SOLVER_INSTALL: c_int = 0x100;

pub const SOLVER_FLAG_BEST_OBEY_POLICY: c_int = 12;

pub struct Pool {
    pool: *mut ffi::Pool,
}

macro_rules! cstr {
    ($s:expr) => {
        CString::new($s)?.as_ptr() as *const c_char
    };
}

#[inline]
fn solvable_to_meta(s: *mut ffi::Solvable) -> Result<PackageMeta> {
    let mut sum_type: ffi::Id = 0;
    let checksum = unsafe {
        ffi::solvable_lookup_bin_checksum(
            s,
            ffi::solv_knownid_SOLVABLE_CHECKSUM as i32,
            &mut sum_type,
        )
    };
    if sum_type != (ffi::solv_knownid_REPOKEY_TYPE_SHA256 as i32) {
        return Err(anyhow!("Unsupported checksum type: {}", sum_type));
    }
    let checksum = unsafe { slice::from_raw_parts(checksum, 32) };
    let name = unsafe {
        CStr::from_ptr(ffi::solvable_lookup_str(
            s,
            ffi::solv_knownid_SOLVABLE_NAME as i32,
        ))
    };
    let version = unsafe {
        CStr::from_ptr(ffi::solvable_lookup_str(
            s,
            ffi::solv_knownid_SOLVABLE_EVR as i32,
        ))
    };
    let path = unsafe {
        CStr::from_ptr(ffi::solvable_lookup_str(
            s,
            ffi::solv_knownid_SOLVABLE_MEDIADIR as i32,
        ))
    };
    let filename = unsafe {
        CStr::from_ptr(ffi::solvable_lookup_str(
            s,
            ffi::solv_knownid_SOLVABLE_MEDIAFILE as i32,
        ))
    };

    Ok(PackageMeta {
        name: name.to_string_lossy().to_string(),
        version: version.to_string_lossy().to_string(),
        sha256: encode(checksum),
        path: path.to_string_lossy().to_string() + "/" + &filename.to_string_lossy(),
    })
}

impl Pool {
    pub fn new() -> Pool {
        Pool {
            pool: unsafe { ffi::pool_create() },
        }
    }

    pub fn match_package(&self, name: &str, mut queue: Queue) -> Result<Queue> {
        if unsafe { (*self.pool).whatprovides.is_null() } {
            // we can't call createwhatprovides here because of how libsolv manages internal states
            return Err(anyhow!(
                "internal error: `createwhatprovides` needs to be called first."
            ));
        }
        unsafe {
            ffi::selection_make(
                self.pool,
                &mut queue.queue,
                cstr!(name),
                SELECTION_NAME | SELECTION_FLAT | SELECTION_ADD,
            );
        }

        Ok(queue)
    }

    pub fn createwhatprovides(&mut self) {
        unsafe { ffi::pool_createwhatprovides(self.pool) }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        unsafe { ffi::pool_free(self.pool) }
    }
}

pub struct Repo {
    repo: *mut ffi::Repo,
}

impl Repo {
    pub fn new(pool: &Pool, name: &str) -> Result<Repo> {
        let name = CString::new(name)?;
        Ok(Repo {
            repo: unsafe { ffi::repo_create(pool.pool, name.as_ptr()) },
        })
    }

    pub fn add_debpackages(&mut self, path: &Path) -> Result<()> {
        let mut path_buf = path.as_os_str().as_bytes().to_owned();
        path_buf.push(0);
        let fp = unsafe { libc::fopen(path_buf.as_ptr() as *const c_char, cstr!("rb")) };
        if fp.is_null() {
            return Err(anyhow!("Failed to open '{}'", path.display()));
        }
        let result = unsafe { ffi::repo_add_debpackages(self.repo, fp as *mut ffi::_IO_FILE, 0) };
        unsafe { libc::fclose(fp) };
        if result != 0 {
            return Err(anyhow!("Failed to add packages: {}", result));
        }

        Ok(())
    }
}

pub struct Queue {
    queue: ffi::Queue,
}

impl Queue {
    pub fn new() -> Queue {
        Queue {
            queue: ffi::Queue {
                elements: null_mut(),
                count: 0,
                alloc: null_mut(),
                left: 0,
            },
        }
    }

    pub fn mark_all_for_install(&mut self) {
        for item in (0..self.queue.count).step_by(2) {
            unsafe {
                let addr = self.queue.elements.offset(item.try_into().unwrap());
                (*addr) |= SOLVER_INSTALL;
            }
        }
    }
}

impl Drop for Queue {
    fn drop(&mut self) {
        unsafe { ffi::queue_free(&mut self.queue) }
    }
}

pub struct Transaction {
    t: *mut ffi::Transaction,
}

impl Transaction {
    pub fn get_size_change(&self) -> i64 {
        unsafe { ffi::transaction_calc_installsizechange(self.t) }
    }

    pub fn order(&self, flags: c_int) {
        unsafe { ffi::transaction_order(self.t, flags) }
    }

    pub fn create_metadata(&self) -> Result<Vec<PackageMeta>> {
        let mut results = Vec::new();
        unsafe {
            let steps = (*self.t).steps.elements;
            for i in 0..((*self.t).steps.count) {
                let p = *steps.offset(i as isize);
                let pool = (*self.t).pool;
                results.push(solvable_to_meta((*pool).solvables.offset(p as isize))?);
            }
        }

        Ok(results)
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        unsafe { ffi::transaction_free(self.t) }
    }
}

pub struct Solver {
    solver: *mut ffi::Solver,
}

impl Solver {
    pub fn new(pool: &Pool) -> Solver {
        Solver {
            solver: unsafe { ffi::solver_create(pool.pool) },
        }
    }

    pub fn set_flag(&mut self, flag: c_int, value: c_int) -> Result<()> {
        let result = unsafe { ffi::solver_set_flag(self.solver, flag, value) };
        if result != 0 {
            return Err(anyhow!("set_flag failed: {}", result));
        }

        Ok(())
    }

    pub fn create_transaction(&mut self) -> Result<Transaction> {
        let t = unsafe { ffi::solver_create_transaction(self.solver) };
        if t.is_null() {
            return Err(anyhow!("Failed to create transaction"));
        }

        Ok(Transaction { t })
    }

    pub fn solve(&self, queue: &mut Queue) -> Result<()> {
        let result = unsafe { ffi::solver_solve(self.solver, &mut queue.queue) };
        if result != 0 {
            return Err(anyhow!("Solve failed: {}", result));
        }

        Ok(())
    }

    pub fn get_problems(&self) -> Result<Vec<String>> {
        let mut problems = Vec::new();
        let count = unsafe { ffi::solver_problem_count(self.solver) };
        for i in 1..=count {
            let problem = unsafe { ffi::solver_problem2str(self.solver, i as c_int) };
            if problem.is_null() {
                return Err(anyhow!("problem2str failed: {}", i));
            }
            problems.push(unsafe { CStr::from_ptr(problem).to_string_lossy().to_string() });
        }

        Ok(problems)
    }
}

impl Drop for Solver {
    fn drop(&mut self) {
        unsafe { ffi::solver_free(self.solver) }
    }
}
