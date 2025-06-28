# AOSCBootstrap

## Dependencies

AOSCBootstrap requires the following libraries and utilities:

- liblzma
- nettle
- zlib
- arch-chroot

On AOSC OS, you may install these dependencies using the following command:

```bash
# oma install xz nettle zlib arch-chroot
```

## Usage

```
aoscbootstrap <branch> <path/to/target> --arch=<architecture> --config=<config> [--include=<additional packages>] [--include-files=<list of packages>] [mirror URL]
```

The `[mirror URL]` argument is optional, when omitted, the script defaults to `https://repo.aosc.io/debs`.
The `--include=` and `--include-files=` are optional, can be specified multiple times and can be specified together.

For example, to bootstrap a `amd64` architecture base system on the `stable` branch at `/root/aosc`, using `localhost` as the mirror:

```
aoscbootstrap stable /root/aosc http://localhost/debs/ --arch=amd64 --config=aosc-mainline.toml
```

If you want to include additional packages, for example, add `network-base` and `systemd-base`:

```
aoscbootstrap stable /root/aosc http://localhost/debs/ --arch=amd64 --config=aosc-mainline.toml --include="network-base systemd-base"
```

If you want to include even more packages, it is recommended to list them in a separate file like this:

```
network-base
systemd-base
editor-base
[...]
```

Assume you have saved the file as `base.lst`, then you can use AOSCBootstrap like this:

```
aoscbootstrap stable /root/aosc http://localhost/debs/ --arch=amd64 --config=aosc-mainline.toml --include-files=base.lst
```

### Additional Features

- Clean up installations (`ciel factory-reset` equivalent): `-x`
- Run additional scripts **after** cleaning up (if any): `-s <script>`
- Compress a `.tar.xz` tarball: `--export-tar <path/to/tarball>`
- Only runs up until Stage 1 (base filesystem): `-1`

### Using Recipes from `CIEL!`

A conversion script is provided in `recipes` directory. To use this script, you need Perl and Ciel to be installed.

To convert the recipes, simply execute the following command at the root directory of this repository:

```
perl recipes/convert.pl /usr/libexec/ciel-plugin/ciel-generate recipes/
```

And then you should be able to find all the "vanilla" AOSC OS recipes under the `recipes` folder.

## Usage with `CIEL!`

To use AOSCBootstrap with [CIEL!](https://github.com/AOSC-Dev/ciel-rs) and its plugins, you can follow these procedures below:

1. Create your work directory and `cd` into it.
1. Run `ciel init`.
1. Run `aoscbootstrap <branch> $(pwd)/.ciel/container/dist/ --arch=<architecture> [mirror URL]`.
1. When finished, you may proceed to other tasks you may want to perform such as `ciel generate` and `ciel release`.
