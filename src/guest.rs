use std::{ffi::CString, mem::MaybeUninit, process::{Child, Command, Stdio}, thread::sleep, time::Duration};

use anyhow::{anyhow, Result};
use libc::{c_char, c_int};
use libloading::{Library, Symbol};
use rand::random;

#[allow(non_camel_case_types)]
enum sd_bus {}

struct SystemdMachine<'a> {
    sd_bus_open_system_machine:
        Symbol<'a, unsafe extern "C" fn(*mut *mut sd_bus, *const c_char) -> c_int>,
    sd_bus_flush_close_unref: Symbol<'a, unsafe extern "C" fn(*mut sd_bus) -> *mut sd_bus>,
}

fn try_open_container_bus(lib: &SystemdMachine, ns_name: &str) -> Result<()> {
    // There are bunch of trickeries happening here
    // First we initialize an empty pointer
    let mut buf = MaybeUninit::uninit();
    // Convert the ns_name to C-style `const char*` (NUL-terminated)
    let ns_name = CString::new(ns_name)?;
    // unsafe: these functions are from libsystemd, which involving FFI calls
    let sd_bus_open_system_machine = &lib.sd_bus_open_system_machine;
    let sd_bus_flush_close_unref = &lib.sd_bus_flush_close_unref;
    unsafe {
        // Try opening a connection to the container
        if sd_bus_open_system_machine(buf.as_mut_ptr(), ns_name.as_ptr()) >= 0 {
            // If successful, just close the connection and drop the pointer
            sd_bus_flush_close_unref(buf.assume_init());
            return Ok(());
        }
    }

    Err(anyhow!("Could not open container bus"))
}

fn wait_for_container(child: &mut Child, ns_name: &str, retry: usize) -> Result<()> {
    let systemd_lib = unsafe { Library::new("libsystemd.so")? };
    let s1 = unsafe { systemd_lib.get(b"sd_bus_open_system_machine")? };
    let s2 = unsafe { systemd_lib.get(b"sd_bus_flush_close_unref")? };

    let lib = SystemdMachine {
        sd_bus_open_system_machine: s1,
        sd_bus_flush_close_unref: s2,
    };

    for i in 0..retry {
        let exited = child.try_wait()?;
        if let Some(status) = exited {
            return Err(anyhow!("nspawn exited too early! (Status: {})", status));
        }
        // why this is used: because PTY spawning can happen before the systemd in the container
        // is fully initialized. To spawn a new process in the container, we need the systemd
        // in the container to be fully initialized and listening for connections.
        // One way to resolve this issue is to test the connection to the container's systemd.
        if try_open_container_bus(&lib, ns_name).is_ok() {
            return Ok(());
        }
        // wait for a while, sleep time follows a natural-logarithm distribution
        sleep(Duration::from_secs_f32(((i + 1) as f32).ln().ceil()));
    }

    Err(anyhow!("Timeout waiting for container {}", ns_name))
}

fn chroot_do(target: &str, args: &[&str]) -> Result<()> {
    let status = Command::new("chroot").arg(target).args(args).status()?;

    if !status.success() {
        return Err(anyhow!("chroot exited with status {}", status));
    }

    Ok(())
}

#[inline]
/// Execute a command in the container
fn execute_container_command(ns_name: &str, args: &[&str]) -> Result<i32> {
    let exit_code = Command::new("systemd-run")
        .args(&["-M", ns_name, "-qt", "--"])
        .args(args)
        .spawn()?
        .wait()?
        .code()
        .unwrap_or(127);

    Ok(exit_code)
}

fn nspawn_do(target: &str, args: &[&str]) -> Result<()> {
    let ns_name = format!("bootstrap-{:x}", random::<u32>());
    let mut child = Command::new("systemd-nspawn")
        .args(&["-qbD", target, "-M", &ns_name, "--"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    eprintln!("Waiting for the container ...");
    wait_for_container(&mut child, &ns_name, 10)?;
    let status = execute_container_command(&ns_name, args)?;

    eprintln!("Powering off the container ...");
    Command::new("machinectl").args(&["poweroff", &ns_name]).status()?;

    if status != 0 {
        return Err(anyhow!("nspawn exited with status {}", status));
    }

    Ok(())
}

pub fn run_in_guest(target: &str, args: &[&str]) -> Result<()> {
    if which::which("systemd-nspawn").is_ok() {
        return nspawn_do(target, args);
    } else if which::which("chroot").is_ok() {
        return chroot_do(target, args);
    }

    Err(anyhow!("Neither chroot nor systemd-nspawn is available"))
}
