//! Assembling the catalog files.
//!
//! [`build`] takes the upstream bodies plus the committed curation and
//! returns, per provider, the generated entries and the trimmed
//! curation to write to disk. Nothing here touches the filesystem;
//! [`crate::layout`] does that.

use kage_provider::catalog::source::{self, SUPPORTED_PROVIDERS};
use serde_json::Value;

use crate::catalog::{self, CuratedProvider, GeneratedProvider};

/// The upstream bodies one generation run reads. `file://` URLs are
/// accepted for every field, which is how the tests run offline.
#[derive(Debug, Clone)]
pub struct Inputs {
    /// models.dev's `api.json`.
    pub models_dev: String,
    /// Kenari's `/v1/models`.
    pub kenari_models: String,
    /// Kenari's `/api/plans`.
    pub kenari_plans: String,
    /// Command Code's `/provider/v1/models`.
    pub command_code_models: String,
    /// The `command-code` CLI's generated `models.md`.
    pub command_code_cli: String,
}

/// One provider's files, as a run produced them.
#[derive(Debug, Clone)]
pub struct ProviderBuild {
    /// Catalog key, which is the provider id for every provider kage
    /// supports.
    pub provider_id: String,
    /// The upstream-published entries.
    pub generated: GeneratedProvider,
    /// The curation, trimmed to models the provider still lists.
    pub curated: CuratedProvider,
}

/// Everything one run produced.
#[derive(Debug, Clone)]
pub struct Built {
    /// One entry per catalog key, in `SUPPORTED_PROVIDERS` order.
    pub providers: Vec<ProviderBuild>,
    /// How many models the catalog carries.
    pub models: usize,
    /// Models the upstreams left out, and why.
    pub warnings: Vec<String>,
}

/// The committed curation for every provider, keyed by catalog key.
#[derive(Debug, Clone, Default)]
pub struct Curated {
    files: std::collections::BTreeMap<String, CuratedProvider>,
}

impl Curated {
    /// The curation for `provider_id`, empty when it has none.
    #[must_use]
    pub fn get(&self, provider_id: &str) -> CuratedProvider {
        self.files.get(provider_id).cloned().unwrap_or_default()
    }

    /// Insert a provider's curation.
    pub fn insert(&mut self, provider_id: String, curated: CuratedProvider) {
        self.files.insert(provider_id, curated);
    }

    /// Every curated provider, by id.
    pub fn providers(&self) -> impl Iterator<Item = (&String, &CuratedProvider)> {
        self.files.iter()
    }
}

/// Build every provider's files from the upstreams and the committed
/// curation, keyed by catalog key.
///
/// The generated files carry only what an upstream published. The two
/// curated fields for Command Code (a release date and an observed
/// interleaved reasoning field) come from the curation the caller read
/// from disk; everything else is regenerated.
///
/// # Errors
///
/// An upstream body does not parse, the Command Code sources disagree
/// about a model's wire format, or the merged result is missing a
/// provider `SUPPORTED_PROVIDERS` requires.
pub fn build(inputs: &Inputs, curation: &Curated) -> Result<Built, String> {
    // The two manual provider sets are generated from their own
    // endpoints, then merged over models.dev so their bodies replace
    // whatever models.dev lists under the same key.
    let kenari = crate::kenari::sync(
        &inputs.kenari_models,
        &inputs.kenari_plans,
        &inputs.models_dev,
    )?;
    let command_code =
        crate::commandcode::sync(&inputs.command_code_models, &inputs.command_code_cli)?;
    let merged = merge(&inputs.models_dev, &kenari, &command_code.manual)?;

    // Parse through kage's own reader so the catalog is exactly what
    // kage will see, then validate it covers every provider.
    let providers = source::parse(&merged)?;
    for map in SUPPORTED_PROVIDERS {
        if !providers.iter().any(|p| p.id == map.kage_id) {
            return Err(format!("catalog is missing provider '{}'", map.api_id));
        }
    }

    // One build per catalog key. `openai` feeds two kage ids, so the
    // key appears once; the first source provider for a key wins.
    let mut seen: Vec<&str> = Vec::new();
    let mut out: Vec<ProviderBuild> = Vec::new();
    let mut models = 0;
    for map in SUPPORTED_PROVIDERS {
        if seen.contains(&map.api_id) {
            continue;
        }
        seen.push(map.api_id);
        let Some(provider) = providers.iter().find(|p| p.id == map.kage_id) else {
            continue;
        };
        let live: Vec<String> = provider.models.iter().map(|m| m.id.clone()).collect();
        out.push(ProviderBuild {
            provider_id: map.api_id.to_owned(),
            generated: catalog::generated(provider),
            curated: curation.get(map.api_id).without_retired(&live),
        });
        models += provider.models.len();
    }
    Ok(Built {
        providers: out,
        models,
        warnings: command_code.warnings,
    })
}

/// `upstream` with the manual providers inserted over it: each manual
/// provider replaces the same-named upstream provider wholesale,
/// everything else passes through untouched.
///
/// # Errors
///
/// The upstream body does not parse as a JSON object.
fn merge(
    upstream: &str,
    kenari: &serde_json::Map<String, Value>,
    command_code: &serde_json::Map<String, Value>,
) -> Result<String, String> {
    let mut root: serde_json::Map<String, Value> =
        serde_json::from_str(upstream).map_err(|e| format!("parse catalog: {e}"))?;
    for (id, provider) in kenari.iter().chain(command_code) {
        root.insert(id.clone(), provider.clone());
    }
    serde_json::to_string(&root).map_err(|e| e.to_string())
}
