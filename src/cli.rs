use clap::{crate_version, App, Arg};

pub fn build_cli() -> App<'static, 'static> {
    App::new("AOSC OS Bootstrapping Helper")
        .version(crate_version!())
        .about("Helper for bootstrapping AOSC OS from scratch")
        .arg(
            Arg::with_name("config")
                .short("c")
                .long("config")
                .takes_value(true)
                .required(true)
                .help("Sets a custom config file"),
        )
        .arg(
            Arg::with_name("clean")
                .short("x")
                .long("clean")
                .takes_value(false)
                .required(false)
                .help("Clean up (factory-reset) the bootstrapped environment"),
        )
        .arg(
            Arg::with_name("scripts")
                .short("s")
                .long("scripts")
                .takes_value(true)
                .min_values(1)
                .required(false)
                .help("Run specified custom scripts during stage 2 (after clean up, if any)"),
        )
        .arg(
            Arg::with_name("arch")
                .short("a")
                .long("arch")
                .takes_value(true)
                .min_values(1)
                .required(true)
                .help("CPU architectures to consider"),
        )
        .arg(
            Arg::with_name("include")
                .short("i")
                .long("include")
                .min_values(1)
                .help("Extra packages to include"),
        )
        .arg(
            Arg::with_name("include-files")
                .short("f")
                .long("include-files")
                .min_values(1)
                .help("Extra packages to include (read from files)"),
        )
        .arg(
            Arg::with_name("download-only")
                .short("g")
                .long("download-only")
                .conflicts_with("clean")
                .takes_value(false)
                .help("Only downloads packages, do not progress further"),
        )
        .arg(
            Arg::with_name("stage1")
                .short("1")
                .long("stage1-only")
                .conflicts_with("download-only")
                .takes_value(false)
                .help("Only finishes stage 1, do not progress further"),
        )
        .arg(
            Arg::with_name("comps")
                .short("-m")
                .takes_value(true)
                .min_values(1)
                .help("Add additional components"),
        )
        .arg(
            Arg::with_name("BRANCH")
                .required(true)
                .help("Branch to use"),
        )
        .arg(
            Arg::with_name("TARGET")
                .required(true)
                .help("Path to the destination"),
        )
        .arg(
            Arg::with_name("MIRROR")
                .required(false)
                .help("Mirror to be used"),
        )
}
