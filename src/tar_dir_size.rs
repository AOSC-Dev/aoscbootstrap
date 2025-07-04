//! This module is used to accurately "estimate" the size of a tar archive
//! created from a given directory structure.
//!
//! The tar format
//! ==============
//!
//! Here, we deal with the exact size of a tar archive.
//!
//! We assume the GNU format is used (tar -H gnu), and no sparse file is
//! present in the tar file, without xattrs (ACL, SELinux and other custom
//! xattrs).
//!
//! tar is a block based archive format, each block is 512 bytes in size.
//! A tar file contains a series of archived files. It does not contain
//! metadata for the entire archive (i.e. directory structure, overall
//! length and etc.), the only metadata it contains is file metadata. The
//! archive contains one or more of file "entry," which contains a header
//! and its content. The entries are "recorded" or written in series, so
//! that it is easier to operate on sequential-accessed media like tape
//! drives, hence the name "tape archiver". It is also why it takes so
//! long to list files in the archive, as the archive has to be walked
//! through.
//!
//! Each archived file contains a header, which resides in its own block,
//! and a series of content blocks. The contents are padded to 512-byte
//! blocks, i.e. a 1-byte file occupies 2 blocks in the archive file: one
//! header block and one content block. A 513-byte file will occupy three
//! blocks.
//!
//! Another thing to consider is the "record". A record is the size of the
//! single unit that can be read from or written to the medium, similar to
//! the bs= option in the dd(1) utility. An archive must be multiple of
//! records in size.
//!
//! The blocking factor determines the record size, so that the record size
//! is always a multiple of 512. The default (also compiled in) is 20, so
//! the size of a tar archive must be multpile of 10KiB (20 * 512 = 10KiB).
//!
//! The header
//! ----------
//!
//! The GNU tar header is as follows:
//! ```c
//! // Reuses the POSIX header
//! struct posix_header
//! {                /* byte offset */
//!    char name[100];        /*   0 */
//!    char mode[8];          /* 100 */
//!    char uid[8];           /* 108 */
//!    char gid[8];           /* 116 */
//!    char size[12];         /* 124 */
//!    char mtime[12];        /* 136 */
//!    char chksum[8];        /* 148 */
//!    char typeflag;         /* 156 */
//!    char linkname[100];    /* 157 */
//!    char magic[6];         /* 257 */
//!    char version[2];       /* 263 */
//!    char uname[32];        /* 265 */
//!    char gname[32];        /* 297 */
//!    char devmajor[8];      /* 329 */
//!    char devminor[8];      /* 337 */
//!    char prefix[155];      /* 345 */
//!                           /* 500 */
//! };
//! // And then, the old GNU header. The GNU format is almost the same
//! // as the "Old" GNU format (see src/tar.h and src/create.c).
//! struct oldgnu_header
//! {                            /* byte offset */
//!    char unused_pad1[345];    /*   0 */
//!    char atime[12];           /* 345 Incr. archive: atime of the file */
//!    char ctime[12];           /* 357 Incr. archive: ctime of the file */
//!    char offset[12];          /* 369 Multivolume archive: the offset of
//!                                     the start of this volume */
//!    char longnames[4];        /* 381 Not used */
//!    char unused_pad2;         /* 385 */
//!    struct sparse sp[SPARSES_IN_OLDGNU_HEADER];
//!                              /* 386 */
//!    char isextended;          /* 482 Sparse file: Extension sparse header
//!                                     follows */
//!    char realsize[12];        /* 483 Sparse file: Real size*/
//!                              /* 495 */
//! };
//! ```
//!
//! As you can see the header fits in a 512-byte block.
//!
//! Some questions arise
//! ====================
//!
//! Filename length limitation
//! --------------------------
//!
//! As you can see, the file name must be less or equal than 100 characters
//! long. But when we archive the entire filesystem (for example, for backup
//! purposes), some of the file names may exceed this limit - the name field
//! stores the entire path including directory names. So, what happens when
//! the file name exceeds 100-characters limit?
//!
//! Regular files
//! -------------
//!
//! If the name of a regular file exceeds this limit, an additional "file"
//! is inserted before the actual file entry. This additional file entry
//! will have `1 + pad512(name_len + 1)` blocks long. The file type of this
//! entry will be `'L'`, indicating the next entry will have a long name
//! that is stored in this entry.
//!
//! If the name is equal or less than 100 bytes, they are stored in the
//! header and no extra "file" entry is needed. But if the name exceeds
//! this limit, the name will become a null-terminated string, thus the
//! actual bytes stored in the content block is `name_len + 1` bytes.
//! So, a file with 512-charactors long name will use two content blocks.
//!
//! So, to archive a file with super-long name will take additional blocks
//! to record the filename:
//!
//! ```plain
//!                            Block num
//! +----------------------+
//! | File header type 'L' |  n
//! +----------------------+
//! |      file name       |  n + 1
//! +----------------------+
//! X more if name > 512b  X  ...
//! +----------------------+
//! | File header type '0' |  n + (pad512(filename_len) / 512)
//! +----------------------+
//! | Content of the file  | n + (pad512(filename_len) / 512) + 1
//! +----------------------+
//! ```
//!
//! Symbolic and hard links
//! -----------------------
//!
//! Symbolic and hard links have two names: name of the link itself and the
//! target it links to. If one of the names exceeds this limit, then only
//! one additional entry is made. If both of the names exceed this limit,
//! then two additional entrys are made.
//!
//! If the link itself has a long name, then an type `'L'` entry will be
//! inserted before the entry of the link itself; If the link target has
//! a long name, then an type `'K'`` entry will be inserted before the
//! entry of the link it self.
//!
//! If both names are long, then the type `'L'` entry comes first, then
//! goes the type `'K'` entry, then the link entry itself (it has the
//! type `'2'`).
//!
//! Directories, FIFOs and Device Nodes
//! -----------------------------------
//!
//! These types either does not have any contents, or the size of the
//! "content" is known, such as device major/minor number.
//! When the name of these files exceeds the limit, an additional type
//! `'L'` entry will be inserted before the entry of the actual file
//! (in this instance it is directory, FIFO or devicve node), just like the
//! regular file.
//!
//! Since these files do not contain content, there will not be any content
//! blocks following the header.
//!
//! Directories however, their names must end with a path delimiter (`/`),
//! so we have to account for that too.
//!
//! Sockets
//! -------
//!
//! GNU tar does not support sockets, neither the other implementations do.
//!
//! Deviations from tar-rs
//! ======================
//!
//! Some behaviors of `tar-rs` are not the same as GNU ones, including:
//!
//! - The `./` prefix is stripped by default in `tar-rs`.
//! - `tar-rs` does not handle hard links.
//! - `tar-rs` uses blocking factor of 1 instead of 20, thus the entire
//!   archive is padded to 512-byte block.

use anyhow::{Context, Result, bail};
use std::{
    collections::HashMap,
    env::{current_dir, set_current_dir},
    fs::read_link,
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

/// The maximum filename length in the tar header.
const NAME_FIELD_SIZE: usize = 100;
/// The block size.
const BLOCK_SIZE: u64 = 512;
// The record size is `BLOCKING_FACTOR * BLOCK_SIZE`.
//
// The entire tar archive must pad to this size, since it is the unit size of
// one read/write operation from/to the medium, similar to the `bs=` option
// in dd(1) utility.
//
// The default (compiled in) is 20, thus resulting a minimum 10KiB of tar file.
//
// NOTE RECORD_SIZE is given by the get_tar_dir_size().
// const BLOCKING_FACTOR: u64 = 20;
// const RECORD_SIZE: u64 = BLOCKING_FACTOR * BLOCK_SIZE;

/// Pad the given size to 512-byte sized blocks. Returns the number of blocks.
fn pad_to_blocksize(size: u64) -> u64 {
    let padded_bl = size.div_ceil(BLOCK_SIZE);
    let padded_size = padded_bl * BLOCK_SIZE;
    assert!(
        padded_size - size < BLOCK_SIZE,
        "Invalid padding result: {padded_size}, was {size}"
    );
    padded_bl
}

/// Get the intended size occupied in the tar archive of a given file.
fn get_size_in_blocks(
    file: &dyn AsRef<Path>,
    ino_db: &mut HashMap<u64, PathBuf>,
    strip_prefix: bool,
    detect_hard_links: bool,
) -> Result<u64> {
    let file = file.as_ref();
    let mut namelen = file.as_os_str().len();
    let mut size_in_blocks = 1; // Header block
    // Since we are archiving, we have to treat each file as is, even if it
    // is a directory, symbolic link or other file type. We can not follow
    // symlinks.
    let metadata = file.symlink_metadata()?;
    let ftype = metadata.file_type();
    if detect_hard_links {
        let ino = metadata.ino();
        if ino_db.contains_key(&ino) {
            return Ok(1u64);
        }
        ino_db.insert(ino, file.to_path_buf());
    }
    if strip_prefix && file.to_string_lossy().starts_with("./") {
        namelen -= 2;
    }
    if ftype.is_file() {
        let file_length = metadata.len();
        size_in_blocks += pad_to_blocksize(file_length);
    } else if ftype.is_dir() {
        // Directory names must end with a slash.
        if !file.to_string_lossy().ends_with('/') {
            namelen += 1;
        }
    } else if ftype.is_symlink() {
        let link_tgt = read_link(file)?;
        let link_tgt_len = link_tgt.as_os_str().len();
        if link_tgt_len > NAME_FIELD_SIZE {
            // Here, if the link target has a long name, then there will be
            // additional "file" that contains this long name. The name in
            // its header will be "././@LongLink", and the file type is 'K'
            // indicating that the next file will have a long link target.
            size_in_blocks += 1 + pad_to_blocksize(link_tgt_len as u64 + 1);
        }
    } else if ftype.is_socket() {
        // tar can't handle sockets.
        return Ok(0);
    } else if ftype.is_block_device() || ftype.is_char_device() || ftype.is_fifo() {
        // Do nothing, as we've considered the long names, and they doesn't
        // have "contents" to store - device major:minor numbers are stored
        // in the header.
    } else {
        // Unknown file type, skip.
        return Ok(0);
    }
    // Additional blocks used to store the long name, this time it is a
    // null-terminated string.
    if namelen > NAME_FIELD_SIZE {
        size_in_blocks += 1 + pad_to_blocksize(namelen as u64 + 1);
    };
    // debug!("Reporting as {} blocks", size_in_blocks);
    Ok(size_in_blocks)
}

pub fn get_tar_dir_size(
    root: &Path,
    strip_prefix: bool,
    hardlinks: bool,
    record_size: u64,
) -> Result<u64> {
    if record_size < BLOCK_SIZE || record_size % BLOCK_SIZE != 0 {
        bail!("Record size must be a multiple of {}", BLOCK_SIZE);
    }
    // A hashmap with inode numbers as the key. Used to detect hard links.
    // Since a hard link is a feature implemented in the filesystem, we
    // can only rely on the inode number's uniqueness across a filesystem
    // to detect hard links.
    let mut ino_hashmap: HashMap<u64, PathBuf> = HashMap::new();
    // chdir to the system root first. This is necessary! Otherwise the
    // name calculation will be inaccurate, since names in the tar entries
    // must be reative.
    let cwd = current_dir()?;
    set_current_dir(root).context(format!(
        "Can not chdir() into system root {}.",
        &cwd.display()
    ))?;
    // We start with . to walk through the system root.
    let walkdir = WalkDir::new(".")
        .follow_links(false)
        .follow_root_links(false)
        .same_file_system(true);

    let mut total_size_in_blks = 0;
    for ent in walkdir.into_iter() {
        let ent = ent?;
        let path = ent.path();
        total_size_in_blks += get_size_in_blocks(&path, &mut ino_hashmap, strip_prefix, hardlinks)?;
    }

    set_current_dir(&cwd).context(format!(
        "Can not chdir() into the previous work directory '{}.",
        &cwd.display()
    ))?;
    // GNU tar has 1024 bytes of zeros as the EOF marker.
    let total_size_in_bytes = total_size_in_blks * BLOCK_SIZE + 1024;
    // Pad the archive size to the record size.
    let padded_records = total_size_in_bytes.div_ceil(record_size);
    let padded = padded_records * record_size;

    Ok(padded)
}

#[test]
fn test_est_tar_size() -> Result<()> {
    let path = option_env!("TARGET_DIR").context(
        "Target directory is required either by command line or TARGET_DIR environment variable.",
    )?;
    let path = Path::new(path);
    if !path.exists() {
        bail!("{} does not exist", path.display());
    }
    if !path.is_dir() {
        bail!("{} is not a directory", path.display());
    }
    let size = get_tar_dir_size(path, true, false, 512)?;
    eprintln!("{size}");
    Ok(())
}
