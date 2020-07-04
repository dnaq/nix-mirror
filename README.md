# nix-mirror: a tool for mirroring nix binary caches

## Background

It is possible to mirror a nix binary cache using the `nix` command line tool using a command sucn as:

```
xzcat store-paths.xz | nix copy --from https://cache.nixos.org --to file:///path/to/where/the/mirror/should/end/up
```

where store-paths.xz is a file containing the store-paths of all binaries in
the cache, e.g. https://channels.nixos.org/nixos-unstable/store-paths.xz

However that command will use a lot of system resources since it will unpack
all of the downloaded files and repack them before writing them to disk (if I
have understood the reasons for the high cpu usage correctly).

## Features

- [x] Parallel downloads
- [x] Atomic downloads
- [x] Sha256 verification of downloaded archives.


## Building

### With cargo
Make sure to have openssl and pkg-config installed and run `cargo build`.
If using nix you can use the provided `shell.nix` which is only a symlink to
`default.nix` to get the dependencies.

### With nix
Run `nix-build` as usual.

## Usage
- `nix-mirror --help` to show application help
- `nix-mirror store-paths.xz ./mirror` to download all the archives in
`store-paths.xz` and their transitive dependencies to the directory `./mirror`.

## License

Licensed under either of
- [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
- [MIT license](http://opensource.org/licenses/MIT)

at your option.
