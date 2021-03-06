use anyhow::{anyhow, Context, Result};

use hyper::{net::HttpsConnector, Client, Url};
use hyper_native_tls::NativeTlsClient;
use semver_rs::{Range, Version};
use serde_json::Value;
use std::{
    collections::{HashSet},
    io::Read,
    path::{Path, PathBuf},
};
use tar::Archive;

mod pack;
use crate::pack::{gunzip, unpack_archive};

mod cache;
pub use crate::cache::{cache, get_cache_dir, PATH_SEGMENT_ENCODE_SET};

pub mod deps;
pub use deps::{calculate_depends, path_to_root_dependency, path_to_dependencies, Dependency};

use percent_encoding::utf8_percent_encode;
use static_init::{dynamic};

#[dynamic]
static CLIENT_CONNECTOR: Client = Client::with_connector(HttpsConnector::new(NativeTlsClient::new().unwrap()));

static REGISTRY: &'static str = "https://registry.npmjs.org";
// static REGISTRY: &'static str = "http://127.0.0.1:5080";

pub fn install_dep(path: &Path, dep: &Dependency) -> Result<()> {
    let required_version = Range::new(&dep.version)
        .parse()
        .with_context(|| format!("Version {} of {} didn't parse", dep.version, dep.name))?;

    if dep.version.starts_with("git://") {
        use git2::Repository;
        let mut path = path.clone().to_path_buf();
        path.push(dep.name.clone());

        if let Some(x) = dep.version.rfind('#') {
            let (repo, hash) = dep.version.split_at(x);
            let repo_cloned = Repository::clone(repo, &path)?;
            let mut hash = hash.clone().to_string();
            hash.remove(0);
            println!("hash: {}", hash);
            let obj = repo_cloned.revparse_single(&hash)?;
            repo_cloned.checkout_tree(&obj, None)?;
        } else {
            Repository::clone(&dep.version, &path)?;
        }
        return Ok(())
    }

    let metadata = fetch_package_root_metadata(&dep)?;

    let versions = &metadata["versions"]
        .as_object()
        .ok_or(anyhow!("Versions was not a JSON object"))?;

    let mut next_paths: HashSet<PathBuf> = HashSet::new();
    for version in versions.iter().rev() {
        if required_version.test(
            &Version::new(version.0.as_str())
                .parse()
                .with_context(|| format!("{} didn't parse", version.0))?,
        ) {
            let dist = &version.1["dist"];

            let tarball_url = Url::parse(
                &dist["tarball"]
                    .as_str()
                    .ok_or(anyhow!("tarball URL didn't convert to string"))?,
            )
            .context("Couldn't parse URL")?;

            let tarball = gunzip(cache(&dep.name, &version.0, &tarball_url)?, &tarball_url)?;
            let mut archive = Archive::new(tarball.as_slice());

            let mut path = path.to_path_buf();
            path.push(&dep.name);

            unpack_archive(&mut archive, &path, &tarball_url)?;

            next_paths.insert(path);

            break;
        }
    }

    Ok(())
}

/// Metadata for a specific version of a package
pub fn fetch_package_version_metadata(dep: &Dependency, version: &String) -> Result<serde_json::Value> {
    let ssl = NativeTlsClient::new().context("Unable to create a NativeTlsClient")?;
    let connector = HttpsConnector::new(ssl);
    let client = Client::with_connector(connector);

    let url = format!("{}/{}/{}", REGISTRY,
        utf8_percent_encode(&dep.name, PATH_SEGMENT_ENCODE_SET), utf8_percent_encode(&version, PATH_SEGMENT_ENCODE_SET));

    let mut body = String::new();

    client
        .get(&url)
        .send()
        .with_context(|| format!("Couldn't GET URL: {}", url))?
        .read_to_string(&mut body)
        .with_context(|| format!("Couldn't ready body of: {}", url))?;

    let metadata: Value = serde_json::from_str(&body)
        .with_context(|| format!("Couldn't JSON parse metadata from {}", url))?;

    Ok(metadata)
}

/// Metadata for all versions
pub fn fetch_package_root_metadata(dep: &Dependency) -> Result<serde_json::Value> {
    let url = format!("{}/{}", REGISTRY, utf8_percent_encode(&dep.name, PATH_SEGMENT_ENCODE_SET));

    let mut body = String::new();

    CLIENT_CONNECTOR
        .get(&url)
        .send()
        .with_context(|| format!("Couldn't GET URL: {}", url))?
        .read_to_string(&mut body)
        .with_context(|| format!("Couldn't ready body of: {}", url))?;

    let body: Value = serde_json::from_str(&body)
        .with_context(|| format!("Couldn't JSON parse metadata from {}", url))?;

    Ok(body)
}

pub fn fetch_matching_version_metadata<'a>(dep: &'a Dependency, root_metadata: &'a serde_json::Value) -> Result<(&'a String, &'a Value)> {
    let required_version = Range::new(&dep.version)
        .parse()
        .with_context(|| format!("Version {} of {} didn't parse", dep.version, dep.name))?;

    let versions = &root_metadata["versions"]
        .as_object()
        .ok_or(anyhow!("Versions was not a JSON object"))?;

    for version in versions.iter().rev() {
        if required_version.test(
            &Version::new(version.0.as_str())
                .parse()
                .with_context(|| format!("{} didn't parse", version.0))?,
        ) {
            // let dist = &version.1["dist"];

            // let tarball_url = Url::parse(
            //     &dist["tarball"]
            //         .as_str()
            //         .ok_or(anyhow!("tarball URL didn't convert to string"))?,
            // )
            // .context("Couldn't parse URL")?;

            return Ok(version);
        }
    }

    Err(anyhow!("ho matching version"))
}