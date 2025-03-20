//! The tar format
//! ==============
//!
//! Here, we deal with the exact size of a tar archive.
//! We assume the GNU format is used (tar -H gnu), and no sparse file is
//! present in the tar file, without xattrs (ACL, SELinux and other xattrs)
//!
//! tar is a block based archive format, each block is defined as 512 bytes.
//! A tar file contains a series of archived files. Each archived file
//! contains a header block and a series of content blocks. The contents
//! are padded to 512-byte blocks, i.e. a 1-byte fileoccupies 2 blocks in
//! the archive file: one header block and one content block. A 513-byte
//! file will occupy three blocks.
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
//!    char mode[8];        /* 100 */
//!    char uid[8];        /* 108 */
//!    char gid[8];        /* 116 */
//!    char size[12];        /* 124 */
//!    char mtime[12];        /* 136 */
//!    char chksum[8];        /* 148 */
//!    char typeflag;        /* 156 */
//!    char linkname[100];    /* 157 */
//!    char magic[6];        /* 257 */
//!    char version[2];    /* 263 */
//!    char uname[32];        /* 265 */
//!    char gname[32];        /* 297 */
//!    char devmajor[8];    /* 329 */
//!    char devminor[8];    /* 337 */
//!    char prefix[155];    /* 345 */
//!                /* 500 */
//! };
//! // And then, the old GNU header. The GNU format is almost the same
//! // as the "Old" GNU format (see src/tar.h and src/create.c).
//! struct oldgnu_header
//! {                /* byte offset */
//!    char unused_pad1[345];    /*   0 */
//!    char atime[12];        /* 345 Incr. archive: atime of the file */
//!    char ctime[12];        /* 357 Incr. archive: ctime of the file */
//!    char offset[12];        /* 369 Multivolume archive: the offset of
//!                     the start of this volume */
//!    char longnames[4];        /* 381 Not used */
//!    char unused_pad2;        /* 385 */
//!    struct sparse sp[SPARSES_IN_OLDGNU_HEADER];
//!                      /* 386 */
//!    char isextended;        /* 482 Sparse file: Extension sparse header
//!                     follows */
//!    char realsize[12];        /* 483 Sparse file: Real size*/
//!                      /* 495 */
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
//! If the name of a regular file exceeds this limit, an additional "file" is
//! inserted before the actual file record. This additional file record will
//! have `1 + pad512(name_len)` blocks long. The file type of this record
//! will be `'L'`, indicating the next record will have a long name that is
//! stored in this record.
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
//! one additional record is made. If both of the names exceed this limit,
//! then two additional records are made.
//!
//! If the link itself has a long name, then an type `'L'` record will be
//! inserted before the record of the link itself; If the link target has
//! a long name, then an type `'K'`` record will be inserted before the
//! record of the link it self.
//!
//! If both names are long, then the type `'L'` record comes first, then
//! goes the type `'K'` record, then the link record itself (it has the
//! type `'2'`).
//!
//! Directories, FIFOs and Device Nodes
//! -----------------------------------
//!
//! These types either does not have any contents, or the size of the
//! "content" is known, such as device major/minor number.
//! When the name of these files exceeds the limit, an additional type
//! type `'L'` record will be inserted before the record of the actual file
//! (in this instance it is directory, FIFO or devicve node), just like the
//! regular file.
//!
//! Since these files do not contain content, there will not be any content
//! blocks following the header.

use anyhow::Result;
use std::{
    collections::HashMap,
    fs::{Metadata, read_link},
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

/// The maximum filename length in the tar header.
const NAME_FIELD_SIZE: usize = 100;
/// The size of a basic block.
const BLOCK_SIZE: u64 = 512;
/// The record size is `BLOCKING_FACTOR * BLOCK_SIZE`.
///
/// The entire tar archive must pad to this size, since it is the unit size of
/// one read/write operation from/to the medium, similar to the `bs=` option
/// in dd(1) utility.
///
/// The default (compiled in) is 20, thus resulting a minimum 10KiB of tar file.
const BLOCKING_FACTOR: u64 = 20;
const RECORD_SIZE: u64 = BLOCKING_FACTOR * BLOCK_SIZE;

fn pad_512_blocksize(size: u64) -> u64 {
    let padded_bl = size.div_ceil(BLOCK_SIZE);
    let padded_size = padded_bl * BLOCK_SIZE;
    assert!(
        padded_size - size < BLOCK_SIZE,
        "Invalid padding result: {}, was {}",
        padded_size,
        size
    );
    padded_bl
}

fn get_size_in_blocks(file: &dyn AsRef<Path>, metadata: &Metadata) -> Result<u64> {
    let name = file.as_ref();
    let namelen = name.as_os_str().len();
    let ftype = metadata.file_type();
    let mut size_in_blocks = 1; // Header block
    if ftype.is_file() {
        let file_length = metadata.len();
        size_in_blocks += pad_512_blocksize(file_length);
        // debug!(
        //     "File: {} is a regular file, padded to {} bytes (was {})",
        //     name.display(),
        //     size_in_blocks * BLOCK_SIZE,
        //     file_length
        // );
    } else if ftype.is_dir() || ftype.is_block_device() || ftype.is_char_device() || ftype.is_fifo()
    {
        // debug!(
        //     "File {} is a directory, FIFO or device node",
        //     name.display()
        // );
        // Do nothing, as we've considered the long names below.
    } else if ftype.is_symlink() {
        // debug!("File {} is a symbolic link", name.display());
        let link_tgt = read_link(file)?;
        // debug!("This symbol link is linked to {}", &link_tgt.display());
        let link_tgt_len = link_tgt.as_os_str().len();
        if link_tgt_len > NAME_FIELD_SIZE {
            // Here, if the link target has a long name, then there will be
            // additional "file" that contains this long name. The name in
            // its header will be "./.@LongLink", and the file type is 'K'
            // indicating that the next file will have a long link target.
            // debug!("This link target exceeds 100 char limit!");
            size_in_blocks += 1 + pad_512_blocksize(link_tgt_len as u64);
        }
    } else if ftype.is_socket() {
        // info!("File {} is a socket, ignoring.", name.display());
        size_in_blocks = 0;
    }
    if namelen > NAME_FIELD_SIZE {
        // debug!("This file exceeds 100 char limit!");
        size_in_blocks += 1 + pad_512_blocksize(namelen as u64);
    };
    // debug!("Reporting as {} blocks", size_in_blocks);
    Ok(size_in_blocks)
}

pub fn get_tar_dir_size(root: &Path) -> Result<u64> {
    let mut ino_hashmap: HashMap<u64, PathBuf> = HashMap::new();
    let walkdir = WalkDir::new(root)
        .follow_links(false)
        .follow_root_links(false)
        .same_file_system(true);

    let mut total_size_in_blks = 0;
    for ent in walkdir.into_iter() {
        let ent = ent?;
        let path = ent.path();
        let metadata = ent.metadata()?;
        let ino = metadata.ino();
        if ino_hashmap.contains_key(&ino) {
            // info!(
            //     "File {} is a hard link to {}. Reporting as 1 block in size.",
            //     path.display(),
            //     ino_hashmap
            //         .get(&ino)
            //         .expect("Unable to find the duplicate")
            //         .display()
            // );
            total_size_in_blks += 1;
            continue;
        }
        ino_hashmap.insert(ino, path.to_path_buf());
        total_size_in_blks += get_size_in_blocks(&path, &metadata)?;
    }
    let total_size_in_bytes = total_size_in_blks * BLOCK_SIZE + 1024;
    let padded_records = total_size_in_bytes.div_ceil(RECORD_SIZE);
    let padded = padded_records * RECORD_SIZE;
    // println!(
    //     "Total estimated tar size: {} bytes ({}) in {} records",
    //     padded,
    //     ByteSize::b(padded),
    //     padded_records
    // );

    Ok(padded)
}
