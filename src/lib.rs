use std::path::Path;

use futures::stream::StreamExt;
use tokio::fs;
use tokio::io::{self, AsyncBufReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::task;

use eyre::{bail, eyre, Result};
use nix_base32::to_nix_base32;
use path_clean::PathClean;
use sha2::{Digest, Sha256};

/// Extracts the narinfo hash part from a nix store filename.
/// ```
/// use nix_mirror::filename_to_narinfo_hash;
/// let hash = filename_to_narinfo_hash("0001w2k3pgl0pkrn827dxiibvc2sibnd-singleton-bool-0.1.5.tar.gz.drv").unwrap();
/// assert_eq!(hash, "0001w2k3pgl0pkrn827dxiibvc2sibnd");
/// ```
pub fn filename_to_narinfo_hash(filename: &str) -> Result<&str> {
    filename
        .split('-')
        .next()
        .ok_or_else(|| eyre!("failed to parse narinfo hash: {}", filename))
}

/// Extracts the narinfo hash path from a nix store path
/// ```
/// use nix_mirror::store_path_to_narinfo_hash;
/// let hash = store_path_to_narinfo_hash("/nix/store/0001w2k3pgl0pkrn827dxiibvc2sibnd-singleton-bool-0.1.5.tar.gz.drv").unwrap();
/// assert_eq!(hash, "0001w2k3pgl0pkrn827dxiibvc2sibnd");
/// ```
pub fn store_path_to_narinfo_hash(store_path: &str) -> Result<&str> {
    store_path
        .split('/')
        .nth(3)
        .ok_or_else(|| eyre!("failed to parse store_path: {}", store_path))
        .and_then(filename_to_narinfo_hash)
}

/// Download a single file to a temporary file, moving it to the destination file atomically
/// if the download succeeded.
///
/// `client` - a reqwest::Client, as given by `reqwest::Client::new()`.
/// `url` - the url we want to download
/// `destination` - where we want the resulting file to end up
/// `hash` - optionally a sha256-digest (in nix-base32) of the file that will be checked
pub async fn download_atomically(
    client: &reqwest::Client,
    url: String,
    destination: &Path,
    hash: Option<&str>,
) -> Result<fs::File> {
    let mut resp_stream = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .bytes_stream();
    let mut ctx = hash.map(|_| Sha256::new());

    let destination_dir = destination.parent().unwrap();
    let result: Result<_> = task::block_in_place(|| {
        let tempfile = tempfile::NamedTempFile::new_in(&destination_dir)?;
        let f: fs::File = tempfile.reopen()?.into();
        Ok((tempfile, f))
    });
    let (tempfile, mut async_file) = result?;

    while let Some(bytes) = resp_stream.next().await {
        let bytes = bytes?;
        if let Some(ctx) = ctx.as_mut() {
            ctx.update(&bytes);
        }
        async_file.write_all(&bytes).await?;
    }
    async_file.shutdown().await?;
    if let Some(ctx) = ctx {
        let hash = hash.unwrap();
        let computed = to_nix_base32(&ctx.finalize().as_ref());
        if computed != hash {
            bail!(
                "hash of file: {:?} failed, expected: {}, got: {}",
                destination,
                hash,
                computed
            );
        }
    }

    let f = task::block_in_place(|| tempfile.persist(&destination))?;
    let mut f = fs::File::from(f);
    f.seek(std::io::SeekFrom::Start(0)).await?;

    Ok(f)
}

/// Download a narinfo file (or open it if it already exists), parse it and download
/// the referenced nar-archive (if it doesn't already exist).
///
/// Returns a `Vec` of narinfo hashes for all the packages references, so that they can
/// be downloaded in turn.
pub async fn handle_narinfo(
    client: &reqwest::Client,
    cache_url: &String,
    mirror_dir: &Path,
    narinfo_hash: String,
) -> Result<Vec<String>> {
    let mut narinfo_filename = mirror_dir.join(&narinfo_hash);
    narinfo_filename.set_extension("narinfo");
    let narinfo_filename = narinfo_filename.clean();

    // check to see if the narinfo file exists and download it if it doesn't
    // (or if we fail to open it).
    let narinfo_file = if let Ok(f) = fs::File::open(&narinfo_filename).await {
        f
    } else {
        let url = format!("{}/{}.narinfo", cache_url, &narinfo_hash);
        download_atomically(client, url, &narinfo_filename, None).await?
    };

    let narinfo_file = io::BufReader::new(narinfo_file);
    let mut lines = narinfo_file.lines();

    // ugly parser, but it would be overkill to reach for a parsing library here
    let mut url = Err(eyre!("failed to find URL"));
    let mut references = Vec::new();
    let mut filehash = Err(eyre!("failed to find filehash"));
    while let Some(line) = lines.next_line().await? {
        let mut split = line.splitn(2, ": ");
        let key = split.next().ok_or_else(|| eyre!("failed to find key"))?;
        let val = split.next().ok_or_else(|| eyre!("failed to find val"))?;
        match key {
            "URL" => url = Ok(String::from(val)),
            "References" => {
                references = val
                    .split_whitespace()
                    .flat_map(|x| x.split("-").next())
                    .map(String::from)
                    .collect()
            }
            "FileHash" => {
                filehash = val
                    .split(':')
                    .nth(1)
                    .map(String::from)
                    .ok_or_else(|| eyre!("invalid filehash"))
            }
            _ => {}
        }
    }

    // we error out if we didn't find an url or a filehash
    let url = url?;
    let filehash = filehash?;

    // check to see if we need to download the nar archive, if so do it
    let filename = mirror_dir.join(&url).clean();
    if fs::File::open(&filename).await.is_err() {
        let url = format!("{}/{}", cache_url, &url);
        download_atomically(client, url, &filename, Some(&filehash)).await?;
    }

    Ok(references.into_iter().map(String::from).collect())
}
