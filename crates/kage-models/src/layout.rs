//! Writing and reading the catalog directory.
//!
//! The layout is `catalog/generated/<provider>.toml` for the rewritten
//! upstream data and `catalog/curated/<provider>.toml` for the
//! hand-maintained fields. A provider with more than `SHARD_THRESHOLD`
//! models is split across numbered shards, because one 318-model file
//! is a diff nobody reads.
//!
//! Provider logos sit beside them in `logos/<id>.svg`, one file per
//! catalog key, and are copied into the deploy directory unchanged.
//!
//! Reads are always by provider: shards are concatenated in name order,
//! so the numbering is invisible to a caller.

use std::fs;
use std::path::{Path, PathBuf};

use kage_provider::catalog::source::SourceProvider;

use crate::catalog::{self, CuratedProvider, GeneratedProvider, SHARD_THRESHOLD};

/// The catalog directory, resolved against the repository root.
#[must_use]
pub fn catalog_dir() -> PathBuf {
    crate::repo_root().join("catalog")
}

/// `catalog/curated/<provider>.toml`.
#[must_use]
pub fn curated_path(provider_id: &str) -> PathBuf {
    catalog_dir()
        .join("curated")
        .join(provider_id)
        .with_extension("toml")
}

/// The generated paths for `provider_id`, given how many shards it needs.
///
/// One shard is `<provider>.toml`. Several are `<provider>/<NNN>.toml`,
/// numbered so a lexical sort is the right order.
#[must_use]
pub fn generated_paths(provider_id: &str, shards: usize) -> Vec<PathBuf> {
    if shards <= 1 {
        return vec![
            catalog_dir()
                .join("generated")
                .join(provider_id)
                .with_extension("toml"),
        ];
    }
    (0..shards)
        .map(|index| {
            catalog_dir()
                .join("generated")
                .join(provider_id)
                .join(format!("{index:03}.toml"))
        })
        .collect()
}

/// `generated` cut into near-equal shards, each carrying the provider's
/// name and api so a shard read on its own is complete.
#[must_use]
pub fn shards(generated: &GeneratedProvider) -> Vec<GeneratedProvider> {
    let total = generated.models.len();
    let wanted = total.div_ceil(SHARD_THRESHOLD).max(1);
    if wanted <= 1 {
        return vec![generated.clone()];
    }
    let per = total.div_ceil(wanted);
    generated
        .models
        .chunks(per)
        .map(|models| GeneratedProvider {
            name: generated.name.clone(),
            api: generated.api.clone(),
            models: models.to_vec(),
        })
        .collect()
}

/// The deploy directory the workflow publishes. Gitignored: it holds
/// generated output, not source.
#[must_use]
pub fn public_dir() -> PathBuf {
    crate::repo_root().join("public")
}

/// The committed provider logos, by catalog id.
#[must_use]
pub fn logo_dir() -> PathBuf {
    crate::repo_root().join("logos")
}

/// One committed logo, `logos/<id>.svg`.
#[must_use]
pub fn logo_path(provider_id: &str) -> PathBuf {
    logo_dir().join(provider_id).with_extension("svg")
}

/// Where the deploy directory's logos are published.
#[must_use]
pub fn public_logo_path(provider_id: &str) -> PathBuf {
    public_dir()
        .join("logos")
        .join(provider_id)
        .with_extension("svg")
}

/// Delete every catalog TOML the run did not write.
///
/// This is what keeps the tree honest when a provider shrinks below the
/// shard threshold or loses its curation: without it the old shard, or
/// an emptied curated file, would survive and be concatenated back in on
/// the next read. Logos are pruned the same way, so a provider that
/// loses its logo upstream does not keep serving the old file.
///
/// # Errors
///
/// A stale file cannot be removed.
pub fn prune_stale(keep: &[PathBuf]) -> Result<(), String> {
    for path in catalog_files() {
        if keep.iter().any(|k| k == &path) {
            continue;
        }
        fs::remove_file(&path).map_err(|e| format!("remove {}: {e}", path.display()))?;
        // A shard directory left empty goes with its file.
        if let Some(dir) = path.parent()
            && dir.file_name().is_some_and(|n| n != "generated")
            && dir.file_name().is_some_and(|n| n != "curated")
            && fs::read_dir(dir).is_ok_and(|mut d| d.next().is_none())
        {
            let _ = fs::remove_dir(dir);
        }
    }
    prune_stale_logos(keep)
}

/// Every committed logo on disk.
fn logo_files() -> Vec<PathBuf> {
    fs::read_dir(logo_dir())
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "svg"))
                .collect()
        })
        .unwrap_or_default()
}

/// Delete every logo the run did not write.
fn prune_stale_logos(keep: &[PathBuf]) -> Result<(), String> {
    for path in logo_files() {
        if keep.iter().any(|k| k == &path) {
            continue;
        }
        fs::remove_file(&path).map_err(|e| format!("remove {}: {e}", path.display()))?;
    }
    Ok(())
}

/// The reviewed, committed routing config copied into [`public_dir`].
#[must_use]
pub fn redirects_path() -> PathBuf {
    crate::repo_root().join("static/_redirects")
}

/// Every catalog TOML currently on disk.
///
/// Used by [`prune_stale`] to tell a file this run produced from one it
/// did not.
fn catalog_files() -> Vec<PathBuf> {
    let generated = catalog_dir().join("generated");
    let curated = catalog_dir().join("curated");
    let mut found: Vec<PathBuf> = Vec::new();
    for dir in [generated, curated] {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "toml") {
                found.push(path);
                continue;
            }
            // A sharded provider's own directory holds `NNN.toml`.
            let Ok(shards) = fs::read_dir(&path) else {
                continue;
            };
            found.extend(
                shards
                    .filter_map(Result::ok)
                    .map(|s| s.path())
                    .filter(|p| p.extension().is_some_and(|e| e == "toml")),
            );
        }
    }
    found
}

/// Every provider kage supports, deduplicated by catalog key.
///
/// `openai` feeds two kage ids; the catalog key appears once.
#[must_use]
pub fn catalog_keys() -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for (api_id, _) in catalog::provider_ids() {
        if !keys.iter().any(|k| k == api_id) {
            keys.push(api_id.to_owned());
        }
    }
    keys
}

/// A provider's generated entries, from disk, across its shards.
fn read_generated(provider_id: &str) -> Result<GeneratedProvider, String> {
    let mut models = Vec::new();
    let mut name = String::new();
    let mut api = None;
    for path in existing_generated(provider_id) {
        let body =
            fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let shard: GeneratedProvider =
            toml::from_str(&body).map_err(|e| format!("parse {}: {e}", path.display()))?;
        name = shard.name;
        api = shard.api;
        models.extend(shard.models);
    }
    Ok(GeneratedProvider { name, api, models })
}

/// The generated files that exist for `provider_id`, in read order.
fn existing_generated(provider_id: &str) -> Vec<PathBuf> {
    let single = catalog_dir()
        .join("generated")
        .join(provider_id)
        .with_extension("toml");
    if single.exists() {
        return vec![single];
    }
    let mut shards: Vec<PathBuf> = fs::read_dir(catalog_dir().join("generated").join(provider_id))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "toml"))
                .collect()
        })
        .unwrap_or_default();
    shards.sort();
    shards
}

/// A provider's curated entries, from disk, empty when it has none.
fn read_curated(provider_id: &str) -> Result<CuratedProvider, String> {
    let path = curated_path(provider_id);
    if !path.exists() {
        return Ok(CuratedProvider::default());
    }
    let body = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    toml::from_str(&body).map_err(|e| format!("parse {}: {e}", path.display()))
}

/// Read a provider's files back as kage's parser sees them.
///
/// # Errors
///
/// A file does not parse, or a curated interleaved field names one kage
/// does not know.
pub fn read(provider_id: &str) -> Result<SourceProvider, String> {
    catalog::to_source(
        provider_id,
        &read_generated(provider_id)?,
        &read_curated(provider_id)?,
    )
}

/// The models.dev-shaped JSON the runtime fetches, read back from the
/// committed TOML rather than from the run in memory.
///
/// Reading the files is deliberate: the published document is then
/// exactly what the repo says, so a hand correction to a TOML file
/// reaches kage even if a generator bug would have reverted it.
///
/// # Errors
///
/// A provider's files do not parse.
pub fn bundle() -> Result<String, String> {
    let mut root = serde_json::Map::new();
    for key in catalog_keys() {
        let generated = read_generated(&key)?;
        let curated = read_curated(&key)?;
        root.insert(
            key.clone(),
            catalog::provider_json(&key, &generated, &curated)?,
        );
    }
    serde_json::to_string(&root).map_err(|e| e.to_string())
}

/// Whether `path` is inside the catalog directory.
#[must_use]
pub fn is_catalog_file(path: &Path) -> bool {
    path.starts_with(catalog_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::ModelEntry;

    fn provider_with(count: usize) -> GeneratedProvider {
        GeneratedProvider {
            name: "P".to_owned(),
            api: Some("https://example.test/v1".to_owned()),
            models: (0..count)
                .map(|i| ModelEntry {
                    id: format!("m{i:04}"),
                    ..Default::default()
                })
                .collect(),
        }
    }

    #[test]
    fn catalog_keys_are_deduplicated() {
        let keys = catalog_keys();
        let mut seen: Vec<&String> = Vec::new();
        for key in &keys {
            assert!(!seen.contains(&key), "{key} is listed twice: {keys:?}");
            seen.push(key);
        }
        assert_eq!(
            keys.iter().filter(|k| *k == "openai").count(),
            1,
            "`openai` feeds two kage ids but is one key: {keys:?}"
        );
    }

    #[test]
    fn a_provider_at_the_threshold_stays_one_file() {
        assert_eq!(shards(&provider_with(SHARD_THRESHOLD)).len(), 1);
        assert_eq!(shards(&provider_with(SHARD_THRESHOLD + 1)).len(), 2);
        assert_eq!(shards(&provider_with(SHARD_THRESHOLD * 3)).len(), 3);
        assert_eq!(
            shards(&provider_with(0)).len(),
            1,
            "an empty provider is one file"
        );
    }

    #[test]
    fn shards_partition_the_models_and_carry_the_metadata() {
        let generated = provider_with(95);
        let parts = shards(&generated);
        assert_eq!(parts.len(), 3);
        for part in &parts {
            assert_eq!(part.name, "P");
            assert_eq!(part.api.as_deref(), Some("https://example.test/v1"));
        }
        assert_eq!(parts[0].models.len(), 32);
        assert_eq!(parts[1].models.len(), 32);
        assert_eq!(parts[2].models.len(), 31);
        let mut ids: Vec<&str> = parts
            .iter()
            .flat_map(|p| p.models.iter().map(|m| m.id.as_str()))
            .collect();
        ids.sort_unstable();
        let mut expected: Vec<&str> = generated.models.iter().map(|m| m.id.as_str()).collect();
        expected.sort_unstable();
        assert_eq!(ids, expected, "no model is lost or duplicated");
    }

    #[test]
    fn generated_paths_name_shards_so_a_lexical_sort_reads_in_order() {
        let single = generated_paths("groq", 1);
        assert_eq!(single.len(), 1);
        assert!(single[0].ends_with("generated/groq.toml"), "{single:?}");

        let many = generated_paths("openrouter", 12);
        assert_eq!(many.len(), 12);
        assert!(many[0].ends_with("openrouter/000.toml"), "{many:?}");
        assert!(many[11].ends_with("openrouter/011.toml"), "{many:?}");
    }
}
