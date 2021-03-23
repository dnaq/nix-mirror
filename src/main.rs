use nix_mirror::{handle_narinfo, store_path_to_narinfo_hash};

use std::collections::HashSet;
use std::path::PathBuf;

use futures::stream::{self, StreamExt};
use tokio::fs;

use eyre::Result;
use indicatif::ProgressBar;
use structopt::StructOpt;

/// An application to synchronize nix binary caches
#[derive(StructOpt)]
struct Opt {
    /// The path to a store-paths.xz file containing the store paths of the packages
    /// that should be synchronized, e.g a copy of http://channels.nixos.org/nixpkgs-unstable/store-paths.xz
    store_paths: PathBuf,

    /// The directory where the mirror should be stored
    mirror_dir: PathBuf,

    /// URL to the cache server that should be used
    #[structopt(short, long, default_value = "https://cache.nixos.org")]
    cache_url: String,

    /// Maximum number of concurrent downloads
    #[structopt(short, long, default_value = "8")]
    parallelism: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Opt::from_args();
    let nar_dir = opt.mirror_dir.join("nar");
    fs::create_dir_all(&nar_dir).await?;

    // read all store paths to memory, there aren't that many of
    // them, so we might as well read all of them into memory
    let store_paths = {
        use std::{fs::File, io::Read};
        let mut s = String::new();
        File::open(&opt.store_paths)
            .map(xz2::read::XzDecoder::new)
            .and_then(|mut rdr| rdr.read_to_string(&mut s))?;
        s
    };

    // a bit hacky, since we don't know how many files we need to process until
    // we're done, but it might still be nice to see some progress.
    let progress = ProgressBar::new(0);

    let client = reqwest::Client::new();

    // our initial set of narinfo hashes to process
    let mut current_narinfo_hashes = store_paths
        .lines()
        .map(|x| store_path_to_narinfo_hash(x).map(String::from))
        .collect::<Result<HashSet<_>>>()?;
    // all narinfo hashes that we have seen
    let mut processed_narinfo_hashes = HashSet::new();

    while !current_narinfo_hashes.is_empty() {
        let mut futures = Vec::new();
        for narinfo_hash in current_narinfo_hashes.drain() {
            futures.push(handle_narinfo(
                &client,
                &opt.cache_url,
                &opt.mirror_dir,
                narinfo_hash.clone(),
            ));
            processed_narinfo_hashes.insert(narinfo_hash);
            progress.inc_length(1);
        }

        // handle at most `opt.parallelism` concurrent futures at the same time
        let mut stream = stream::iter(futures).buffer_unordered(opt.parallelism);

        // for the result of each future, check to see if we've already seen that hash
        // and if not add it to the set of hashes to process
        while let Some(result) = stream.next().await {
            let new_hashes = result?;
            current_narinfo_hashes.extend(
                new_hashes
                    .into_iter()
                    .filter(|x| !processed_narinfo_hashes.contains(x)),
            );
            progress.inc(1);
        }
    }

    Ok(())
}
