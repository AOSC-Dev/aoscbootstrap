use anyhow::{anyhow, Result};
use flate2::write::GzEncoder;
use flate2::Compression;
use nix::fcntl::{open, OFlag};
use nix::sys::stat::{fchmodat, makedev, mknod, FchmodatFlags, Mode, SFlag};
use nix::unistd::{close, mkdir};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::{
    fs::{create_dir_all, write, File},
    io::Read,
};
use tar::Builder;
use xz2::stream::{Filters, LzmaOptions, MtStreamBuilder, Stream};
use xz2::write::XzEncoder;

/// 定义 LZMA_PRESET_EXTREME（LZMA extreme 预设，"e"）的掩码，即最大的 32 位整数
const LZMA_PRESET_EXTREME: u32 = 1 << 31;

/// bootstrap_apt 函数，取三个参数：
///
/// - root（系统根，类型为路径）
/// - mirror（镜像源，类型为字符串）
/// - branch（系统分支，类型为字符串）
pub fn bootstrap_apt(root: &Path, mirror: &str, branch: &str) -> Result<()> {
    // 在系统根路径创建 /var/lib/dpkg（dpkg 状态路径）
    create_dir_all(root.join("var/lib/dpkg"))?;
    // 在系统根路径创建 /etc/apt（APT 配置路径）
    create_dir_all(root.join("etc/apt"))?;
    // 在系统根路径创建 /var/lib/apt/lists（APT 本地数据缓存路径）
    create_dir_all(root.join("var/lib/apt/lists"))?;
    // 在系统根路径创建 /etc/locale.conf（系统地域和语言设置）
    // 默认设置语言 (LANG) 为 C.UTF-8（即“无本地化，带 UTF-8 编码”），设置一个政治上中立的语言
    write(root.join("etc/locale.conf"), b"LANG=C.UTF-8\n")?;
    // 在系统根路径创建 /etc/shadow（密码数据库）
    // 写入 root 密码条目，默认不设置密码（AOSC OS 默认不打开 root 登录权限，鼓励用户使用 sudo）
    write(root.join("etc/shadow"), b"root:x:1:0:99999:7:::\n")?;
    // 根据 mirror 和 branch 参数生成 /etc/apt/sources.list（APT 主配置文件）
    write(
        root.join("etc/apt/sources.list"),
        // 此处使用 format! 是因为需要使用参数变量（以模板的形式生成文件）
        format!("deb {} {} main\n", mirror, branch),
    )?;

    // 在系统根路径创建几个 dpkg 的必要状态文件（权限位均为 0644）
    //
    // /var/lib/dpkg/available：当前可用的所有软件包和版本（似乎只是 debian-installer 和 dselect 要用，
    // AOSC OS 提供这一文件是为了避免 dpkg 报错
    //
    // /var/lib/dpkg/status：dpkg 的主状态文件，记录所有软件包的安装状态、依赖关系、版本等
    close(open(
        &root.join("var/lib/dpkg/available"),
        OFlag::O_CREAT,
        Mode::from_bits_truncate(0o644),
    )?)
    .ok();
    close(open(
        &root.join("var/lib/dpkg/status"),
        OFlag::O_CREAT,
        Mode::from_bits_truncate(0o644),
    )?)
    .ok();

    // 标准：/etc/shadow 文件权限位应为 0000
    fchmodat(
        None,
        &root.join("etc/shadow"),
        Mode::empty(),
        FchmodatFlags::NoFollowSymlink,
    )?;

    // 标准：/etc/apt/sources.list 文件权限位应为 0644
    //
    // FIXME: 为何需要这样定义？前面使用 write() 写入的文件应该是根据 umask 设置的，默认应为 0644
    fchmodat(
        None,
        &root.join("etc/apt/sources.list"),
        Mode::from_bits_truncate(0o644),
        FchmodatFlags::NoFollowSymlink,
    )?;

    Ok(())
}

/// 生成系统压缩包（xz 格式），取三个参数：
///
/// - root（tar 压缩包路径，类型为路径）
/// - target（目标文件，类型为路径）
/// - threads（压缩线程数，类型为 u32）
pub fn archive_xz_tarball(root: &Path, target: &Path, threads: u32) -> Result<()> {
    // f：使用 File 方式实现在目标路径 target 创建 xz 压缩包文件
    let f = File::create(target)?;
    // xz：使用 Stream 方式实现 xz 压缩流，利用本文件中定义的 build_xz_encoder 函数创建 xz 数据流
    // 此处亦使用本函数的 threads 参数定义压缩（编码）线程数
    let xz = build_xz_encoder(threads)?;
    // builder：使用 Builder<XzEncoder<File>> 方式实现 xz 压缩包生成流程
    // 此处取前面定义的 f（目标文件）和 xz（xz 数据流）作为参数，定义了目标文件路径和压缩线程数
    // 并取 root 参数定义系统根的路径作为压缩包的内容
    let builder: Builder<XzEncoder<File>> =
        build_tarball_stream(XzEncoder::new_stream(f, xz), root)?;
    // 执行压缩操作并对数据流完整性进行查验（以 ? 确保每一个步骤正确完成）：
    //
    // - 使用 into_inner() 确保压缩包内容完整
    // - 使用 finish() 确保写数据流正确关闭
    // - 使用 sync_all() 确保 I/O 操作结果同步到了磁盘
    builder.into_inner()?.finish()?.sync_all()?;

    Ok(())
}

/// 生成系统压缩包（gz 格式），取两个参数：
///
/// - root（tar 压缩包路径，类型为路径）
/// - target（目标文件，类型为路径）
///
/// 不同于 archive_xz_tarball 函数，由于 gzip 压缩不支持多线程，因此无需定义 threads 参数
pub fn archive_gz_tarball(root: &Path, target: &Path) -> Result<()> {
    // f：使用 File 方式实现在目标路径 target 创建 xz 压缩包文件
    let f = File::create(target)?;
    // builder: 使用 Builder<GzEncoder<File>> 方式实现 gz 压缩包生成流程
    // 此处去前面定义的 f（目标文件）作为参数，定义了目标文件路径
    // 并取 root 参数定义系统根的路径作为压缩包的内容
    //
    // 此处定义默认压缩级别为 best（最高压缩比）
    let builder: Builder<GzEncoder<File>> =
        build_tarball_stream(GzEncoder::new(f, Compression::best()), root)?;
    // 执行压缩操作并对数据流完整性进行查验（以 ? 确保每一个步骤正确完成）：
    //
    // - 使用 into_inner() 确保压缩包内容完整
    // - 使用 finish() 确保写数据流正确关闭
    // - 使用 sync_all() 确保 I/O 操作结果同步到了磁盘
    builder.into_inner()?.finish()?.sync_all()?;

    Ok(())
}

/// 生成 tar 包装的压缩包 (.tar.*) 数据流，取两个函数：
///
/// - stream（输入流，W - 循环读取输出直到读取完毕）
/// - root（系统根，类型为路径）
fn build_tarball_stream<W: Write>(stream: W, root: &Path) -> Result<Builder<W>, anyhow::Error> {
    // 使用 Builder<W> 方式实现 tar 压缩流程，即从输入流读取数据并包进 tar 包中
    let mut builder = Builder::new(stream);
    // 定义压缩模式为 Complete（保留所有受支持的元数据，如访问时间和所有者等）
    //
    // 另有 Deterministic 模式，不保存所有者和访问/修改时间，该模式不适用于存档系统文件，因为有如
    // SDDM 和 HTTPD 的守护模式根 (daemon home) 需要特定所有者和权限位，/usr/bin/sudo 等也需要特定的
    // SUID 标记，如使用该模式则均无法保存，上述程序亦无法正常使用
    builder.mode(tar::HeaderMode::Complete);
    // 禁用 follow_symlink 功能，即将符号链接作为符号链接存档，而非读取目标文件重复存档
    builder.follow_symlinks(false);
    // 从先前定义的 root 参数（系统根路径）读取文件，并保存到归档的根路径 (.)
    // FIXME: 此处 path 参数为何不使用 /？
    builder.append_dir_all(".", root)?;
    // 确保写数据流正确关闭
    builder.finish()?;

    Ok(builder)
}

/// 生成 xz 压缩的 SquashFS 归档用于快速安装（多线程解压），取三个参数：
///
/// - root（系统根，即文件源，类型为路径）
/// - target（目标文件，类型为路径）
/// - threads（压缩线程数，类型为 u32）
///
/// 与 archive_xz_tarball 函数类似，由于 SquashFS 支持多线程压缩，所以实现 threads 函数
pub fn archive_squashfs(root: &Path, target: &Path, threads: u32) -> Result<()> {
    // 运行 mksquashfs 命令
    //
    // - root 参数用于定义 SquashFS 取用文件的路径
    // - target 参数用于定义保存 SquashFS 文件的路径
    // - "-comp xz"：定义压缩方式为 xz，该格式压缩比好且性能较为良好
    // - "-processors _threads_"：此处取 threads 参数定义压缩线程数，将其 u32 类型转换为系统命令参数
    //   所期待的字符串类型 (to_string())
    // - .wait_with_output()：回显命令输出并等待其完成
    //
    // FIXME: 由于没有合适的 SquashFS 压缩库，因此直接执行系统命令
    let output = Command::new("mksquashfs")
        .arg(root)
        .arg(target)
        .arg("-comp")
        .arg("xz")
        .arg("-processors")
        .arg(threads.to_string())
        .spawn()?
        .wait_with_output()?;

    // 判断返回值并传递命令状态给 anyhow 处理错误
    if !output.status.success() {
        return Err(anyhow!("Failed to archive squashfs!"));
    }

    Ok(())
}

/// 创建 xz 压缩流并将函数返回内容作为数据流传送给 archive_xz_tarball，取一个参数：
///
/// - threads（压缩线程数，类型为 u32）
fn build_xz_encoder(threads: u32) -> Result<Stream> {
    // 定义 filter 变量，用于选择 LZMA Filter 算法
    let mut filter = Filters::new();
    // 定义 opts 变量，用于传递 xz 压缩参数（如压缩级别）
    // 此处通过 OR 运算，定义压缩级别为 -9e，即最高压缩级别（最高压缩比，最慢的解压速度，较高内存占用）
    // 这一选项对绝大多数设备来说都不会过于苛刻，亦可帮助缩减压缩包大小
    let mut opts = LzmaOptions::new_preset(9 | LZMA_PRESET_EXTREME)?;
    // 为 xz 压缩流补充一个选项 (opts)，即理想单位字典长度，此处设置为最大的 273 以最大化压缩比
    opts.nice_len(273);
    // 将 LZMA 压缩参数传递给 LZMA2 算法
    filter.lzma2(&opts);

    // 将上述参数传递给多线程压缩编码器 (MtStreamBuilder)
    //
    // - 传递上述定义的 LZMA2 filter（包含前面提到的压缩比和理想单位字典长度）
    // - 使用函数的 threads 参数定义编码线程数量
    //
    // 使用 encoder() 创建压缩进程
    Ok(MtStreamBuilder::new()
        .filters(filter)
        .threads(threads)
        .encoder()?)
}

/// 计算生成后压缩包的校验和 (SHA-256)，取一个参数
///
/// - reader（读取流，R - 循环读取直到读取完毕）
pub fn sha256sum<R: Read>(mut reader: R) -> Result<String> {
    // 定义 hasher 变量传递给 std::io::copy() 的 writer 参数，即 sha256
    let mut hasher = Sha256::new();
    // 读取文件并计算 SHA-256 校验和
    std::io::copy(&mut reader, &mut hasher)?;

    // 返回计算后的 SHA-256 校验和
    Ok(format!("{:x}", hasher.finalize()))
}

/// 创建预设设备节文件（device node），取一个参数：
///
/// - root（系统根，类型为路径）
///
/// FIXME: 该函数应该删除，新的系统不需要创建这些设备节亦可正常启动，而 chroot 环境下也鼓励挂载 /dev (devtmpfs)，
/// 否则某些程序，如 apt 和 grub-install 均无法正常工作；因此，无论如何都无需在系统包里就带上相关文件
pub fn make_device_nodes(root: &Path) -> Result<()> {
    // 定义默认权限：
    //
    // - S_IRGRP：允许文件所有组读取
    // - S_IRUSR：允许文件所有者读取
    // - S_IROTH：允许其他用户读取
    // - S_IWGRP：允许文件所有组写入
    // - S_IWUSR：允许文件所有者写入
    // - S_IWOTH：允许其他用户写入
    //
    // 运算结果为 0666（所有用户和组均可读写，但不可执行）
    let permission = Mode::S_IRGRP
        | Mode::S_IRUSR
        | Mode::S_IROTH
        | Mode::S_IWGRP
        | Mode::S_IWUSR
        | Mode::S_IWOTH;
    // 创建 /dev/null（字符设备），权限位 666，主设备号 1，次设备号 3
    // 设备号均为标准，死记硬背即可
    mknod(
        &root.join("dev/null"),
        SFlag::S_IFCHR,
        permission,
        makedev(1, 3),
    )?;
    // 创建 /dev/null（字符设备），权限位 666，主设备号 5，次设备号 1
    // 设备号均为标准，死记硬背即可
    // FIXME: 此处权限位应为 600（仅文件所有者可读写，其他身份均无读写权限）而非 666
    mknod(
        &root.join("dev/console"),
        SFlag::S_IFCHR,
        permission,
        makedev(5, 1),
    )?;
    // 此处创建 /dev/shm 目录，指代系统共享内存的虚拟设备，此处遵循较新 FHS 标准作为目录存放
    // 权限位为 1777（所有用户和组均可读、写和执行内容，但只有所有者可以删除相关数据）
    mkdir(
        &root.join("dev/shm"),
        Mode::S_IRWXG | Mode::S_IRWXO | Mode::S_IRWXU | Mode::S_ISVTX,
    )?;

    Ok(())
}
