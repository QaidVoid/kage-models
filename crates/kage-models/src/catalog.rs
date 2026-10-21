//! The on-disk catalog shape: one TOML per provider, sharded past a
//! threshold, plus a small curated file per provider.
//!
//! Two file kinds live under `catalog/`:
//!
//! * `generated/<provider>.toml` is rewritten by every run and holds
//!   only what an upstream published. Nobody edits it by hand, so a
//!   scheduled run cannot collide with a reviewer's change.
//! * `curated/<provider>.toml` is hand-maintained and holds the one
//!   field that is observed rather than published: which field a model
//!   reads its reasoning back from.
//!
//! Both feed [`provider_json`], which writes the models.dev-shaped
//! document kage's runtime fetches. The JSON is a packaging step, not a
//! translation: kage's existing parser reads it unchanged, so the TOML
//! split costs the runtime nothing.

use std::collections::BTreeMap;

use kage_core::{Effort, Efforts, Modalities, Modality, Reasoning, ReasoningField};
use kage_provider::catalog::ModelCost;
use kage_provider::catalog::source::{self, SourceModel, SourceProvider};
use serde::{Deserialize, Serialize};

/// A provider with more models than this is split across numbered
/// shards. A 318-model file produces a diff nobody reads.
pub const SHARD_THRESHOLD: usize = 40;

/// One provider's upstream-published entries, as written to
/// `catalog/generated/<provider>.toml`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GeneratedProvider {
    /// Display name.
    pub name: String,
    /// Documented API endpoint, when published.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    /// The provider's models, sorted by id.
    pub models: Vec<ModelEntry>,
}

/// One generated model. A field no upstream answers for is absent,
/// which is what tells a reviewer it was never published.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelEntry {
    /// Provider-scoped model id.
    pub id: String,
    /// Display name. Absent when it is the id, which kage reads the
    /// same way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Context window in tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    /// Largest prompt in tokens, when smaller than the context window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_limit: Option<u64>,
    /// Maximum output tokens per turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<u64>,
    /// Inputs the model accepts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input: Vec<Modality>,
    /// Media the model produces. Absent when the upstream publishes
    /// none, which kage reads as text only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub produces: Vec<Modality>,
    /// Whether the model thinks at all. `false` is written only when
    /// the upstream says so; a model that does not publish the field
    /// leaves it unset and is read as not thinking.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub thinks: bool,
    /// Effort values the model accepts. Empty for a model that does not
    /// take a level, and for one that thinks at a fixed depth.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub efforts: Vec<Effort>,
    /// Written only as `true`, meaning the model can switch thinking
    /// off. A model that cannot leaves it out.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub toggle: bool,
    /// The token budget a model takes instead of an effort level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<Budget>,
    /// Release date, when the upstream published one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
    /// The field this model reads its reasoning back from, when the
    /// upstream published one. models.dev carries this for some
    /// providers; a `catalog/curated/` entry overrides it, because the
    /// rows models.dev leaves `null` demonstrably still use a field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interleaved: Option<String>,
    /// Per-million-token prices in USD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Cost>,
}

/// A token-budget thinking range, for a model that takes no effort
/// level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Budget {
    /// Smallest budget in tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<u32>,
    /// Largest budget in tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
}

/// Per-million-token prices in USD.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    /// Price per million input tokens.
    pub input: f64,
    /// Price per million output tokens.
    pub output: f64,
    /// Price per million cache-read tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,
    /// Price per million cache-write tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
}

/// One provider's hand-maintained fields, as written to
/// `catalog/curated/<provider>.toml`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CuratedProvider {
    /// The field each model reads its reasoning back from during a tool
    /// loop, by model id.
    ///
    /// models.dev publishes `interleaved` for some providers and carries
    /// `null` for rows that demonstrably use `reasoning_content`, so
    /// this is observed rather than derived: a value here overrides what
    /// the upstream published for the same model.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub interleaved: BTreeMap<String, String>,
}

impl CuratedProvider {
    /// Whether anything is curated, so an empty file is never written.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.interleaved.is_empty()
    }

    /// The reasoning field curated for `model_id`, if any.
    pub fn field(&self, model_id: &str) -> Option<&str> {
        self.interleaved.get(model_id).map(String::as_str)
    }

    /// Curated model ids the provider no longer lists.
    ///
    /// A curated field for a dropped model is dead weight that would
    /// come back if the upstream ever re-adds the id, so it is reported
    /// and dropped rather than carried silently.
    #[must_use]
    pub fn retired(&self, live_ids: &[String]) -> Vec<String> {
        let mut stale: Vec<String> = self
            .interleaved
            .keys()
            .filter(|id| !live_ids.contains(id))
            .cloned()
            .collect();
        stale.sort();
        stale.dedup();
        stale
    }

    /// `self` without entries for models the provider no longer lists.
    #[must_use]
    pub fn without_retired(&self, live_ids: &[String]) -> Self {
        Self {
            interleaved: retain(&self.interleaved, live_ids),
        }
    }
}

fn retain(map: &BTreeMap<String, String>, live_ids: &[String]) -> BTreeMap<String, String> {
    map.iter()
        .filter(|(id, _)| live_ids.contains(id))
        .map(|(id, v)| (id.clone(), v.clone()))
        .collect()
}

/// `provider` as its generated TOML entries.
#[must_use]
pub fn generated(provider: &SourceProvider) -> GeneratedProvider {
    GeneratedProvider {
        name: provider.name.clone(),
        api: provider.api.clone(),
        models: provider.models.iter().map(model_entry).collect(),
    }
}

fn model_entry(model: &SourceModel) -> ModelEntry {
    let (thinks, efforts, toggle, budget) = match &model.reasoning {
        // `Unknown` only arises from a hand-written catalog entry; a
        // model the upstreams list either thinks or does not.
        Reasoning::None | Reasoning::Unknown => (false, Vec::new(), false, None),
        Reasoning::Fixed => (true, Vec::new(), false, None),
        Reasoning::Toggle => (true, Vec::new(), true, None),
        Reasoning::Effort { efforts, toggle } => (true, efforts.iter().collect(), *toggle, None),
        Reasoning::Budget { min, max, toggle } => (
            true,
            Vec::new(),
            *toggle,
            Some(Budget {
                min: Some(*min),
                max: *max,
            }),
        ),
    };
    ModelEntry {
        id: model.id.clone(),
        name: (model.name != model.id).then(|| model.name.clone()),
        context: model.context,
        input_limit: model.input_limit,
        output: model.output,
        input: model.input.iter().collect(),
        produces: model.produces.iter().collect(),
        thinks,
        efforts,
        toggle,
        budget,
        release_date: model.release_date.clone(),
        interleaved: model.interleaved.map(|f| f.as_str().to_owned()),
        cost: model.cost.map(|c| Cost {
            input: c.input,
            output: c.output,
            cache_read: c.cache_read,
            cache_write: c.cache_write,
        }),
    }
}

/// The models.dev-shaped JSON one provider's two files stand for.
///
/// # Errors
///
/// Never: the JSON is built from values that already round-trip.
/// Kept as a `Result` so the signature can hold a failure a future
/// field introduces.
pub fn provider_json(
    provider_id: &str,
    generated: &GeneratedProvider,
    curated: &CuratedProvider,
) -> Result<serde_json::Value, String> {
    let mut models = serde_json::Map::new();
    for entry in &generated.models {
        models.insert(entry.id.clone(), model_json(entry, curated));
    }
    let mut provider = serde_json::Map::new();
    provider.insert("id".into(), provider_id.into());
    provider.insert("name".into(), generated.name.clone().into());
    if let Some(api) = &generated.api {
        provider.insert("api".into(), api.clone().into());
    }
    provider.insert("models".into(), models.into());
    Ok(provider.into())
}

fn model_json(entry: &ModelEntry, curated: &CuratedProvider) -> serde_json::Value {
    let mut row = serde_json::Map::new();
    row.insert("id".into(), entry.id.clone().into());
    row.insert(
        "name".into(),
        entry
            .name
            .clone()
            .unwrap_or_else(|| entry.id.clone())
            .into(),
    );
    row.insert("tool_call".into(), true.into());

    let mut limit = serde_json::Map::new();
    if let Some(context) = entry.context {
        limit.insert("context".into(), context.into());
    }
    if let Some(input) = entry.input_limit {
        limit.insert("input".into(), input.into());
    }
    if let Some(output) = entry.output {
        limit.insert("output".into(), output.into());
    }
    if !limit.is_empty() {
        row.insert("limit".into(), limit.into());
    }

    row.insert("reasoning".into(), entry.thinks.into());
    if let Some(options) = options_json(entry) {
        row.insert("reasoning_options".into(), options);
    }

    if !entry.input.is_empty() || !entry.produces.is_empty() {
        let names =
            |kinds: &[Modality]| -> Vec<&str> { kinds.iter().map(|k| k.as_str()).collect() };
        let output = if entry.produces.is_empty() {
            vec!["text"]
        } else {
            names(&entry.produces)
        };
        row.insert(
            "modalities".into(),
            serde_json::json!({ "input": names(&entry.input), "output": output }),
        );
    }

    // Release date is upstream-published, never curated: models.dev
    // carries one for every provider kage supports, so a hand-kept copy
    // could only drift from it.
    if let Some(date) = entry.release_date.as_deref() {
        row.insert("release_date".into(), date.into());
    }
    // The curated field wins: it exists precisely for the rows the
    // upstream leaves blank but that demonstrably use a field.
    let interleaved = curated.field(&entry.id).or(entry.interleaved.as_deref());
    if let Some(field) = interleaved {
        row.insert("interleaved".into(), serde_json::json!({ "field": field }));
    }
    if let Some(cost) = entry.cost {
        row.insert("cost".into(), cost_json(cost));
    }
    row.into()
}

/// A model's `reasoning_options`, or `None` for a model that thinks at a
/// fixed depth, which kage's parser reads back as `Fixed`.
fn options_json(entry: &ModelEntry) -> Option<serde_json::Value> {
    let mut options = Vec::new();
    if entry.toggle {
        options.push(serde_json::json!({ "type": "toggle" }));
    }
    if let Some(budget) = &entry.budget {
        let mut bounds = serde_json::Map::new();
        bounds.insert("min".into(), budget.min.unwrap_or(0).into());
        if let Some(max) = budget.max {
            bounds.insert("max".into(), max.into());
        }
        options.push(serde_json::json!({ "type": "budget_tokens", "bounds": bounds }));
    } else if !entry.efforts.is_empty() {
        let values: Vec<&str> = entry.efforts.iter().map(|e| e.as_str()).collect();
        options.push(serde_json::json!({ "type": "effort", "values": values }));
    }
    (!options.is_empty()).then(|| options.into())
}

fn cost_json(cost: Cost) -> serde_json::Value {
    let mut c = serde_json::Map::new();
    c.insert("input".into(), cost.input.into());
    c.insert("output".into(), cost.output.into());
    if let Some(read) = cost.cache_read {
        c.insert("cache_read".into(), read.into());
    }
    if let Some(write) = cost.cache_write {
        c.insert("cache_write".into(), write.into());
    }
    c.into()
}

/// A generated provider file as kage's own parser reads it, with the
/// curated fields folded in.
///
/// # Errors
///
/// A curated interleaved field names one kage does not know.
pub fn to_source(
    provider_id: &str,
    generated: &GeneratedProvider,
    curated: &CuratedProvider,
) -> Result<SourceProvider, String> {
    let mut models = Vec::with_capacity(generated.models.len());
    for entry in &generated.models {
        models.push(to_model(entry, curated)?);
    }
    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(SourceProvider {
        id: provider_id.to_owned(),
        name: generated.name.clone(),
        api: generated.api.clone(),
        models,
    })
}

fn to_model(entry: &ModelEntry, curated: &CuratedProvider) -> Result<SourceModel, String> {
    // A field from either source has to be one kage can name, whether it
    // came from the upstream or from the curated file.
    let interleaved = curated
        .field(&entry.id)
        .or(entry.interleaved.as_deref())
        .map(|field| {
            ReasoningField::parse(field)
                .ok_or_else(|| format!("model '{}': unknown interleaved field '{field}'", entry.id))
        })
        .transpose()?;
    Ok(SourceModel {
        id: entry.id.clone(),
        name: entry.name.clone().unwrap_or_else(|| entry.id.clone()),
        context: entry.context,
        input_limit: entry.input_limit,
        output: entry.output,
        reasoning: reasoning_from(entry),
        input: Modalities::of(&entry.input),
        produces: Modalities::of(&entry.produces),
        interleaved,
        release_date: entry.release_date.clone(),
        cost: entry.cost.map(|c| ModelCost {
            input: c.input,
            output: c.output,
            cache_read: c.cache_read,
            cache_write: c.cache_write,
        }),
    })
}

/// The thinking a generated entry declares. A budget wins over
/// efforts, and efforts over a bare toggle, because that is the order
/// kage's own parser reads them in.
fn reasoning_from(entry: &ModelEntry) -> Reasoning {
    if let Some(budget) = entry.budget {
        return Reasoning::Budget {
            min: budget.min.unwrap_or(0),
            max: budget.max,
            toggle: entry.toggle,
        };
    }
    if !entry.efforts.is_empty() {
        return Reasoning::Effort {
            efforts: Efforts::of(&entry.efforts),
            toggle: entry.toggle,
        };
    }
    match (entry.thinks, entry.toggle) {
        (false, _) => Reasoning::None,
        (true, true) => Reasoning::Toggle,
        // Thinks with no level and no switch: one unstated depth.
        (true, false) => Reasoning::Fixed,
    }
}

/// Every provider kage supports, as `(catalog key, kage id)`.
///
/// `openai` is read by two kage ids, so this lists a key twice and the
/// two vectors differ in length.
#[must_use]
pub fn provider_ids() -> Vec<(&'static str, &'static str)> {
    source::SUPPORTED_PROVIDERS
        .iter()
        .map(|m| (m.api_id, m.kage_id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str) -> SourceModel {
        SourceModel {
            id: id.to_owned(),
            name: id.to_owned(),
            context: Some(200_000),
            input_limit: None,
            output: Some(64_000),
            reasoning: Reasoning::None,
            input: Modalities::of(&[Modality::Text]),
            produces: Modalities::of(&[]),
            interleaved: None,
            release_date: None,
            cost: None,
        }
    }

    fn provider(id: &str, models: Vec<SourceModel>) -> SourceProvider {
        SourceProvider {
            id: id.to_owned(),
            name: id.to_owned(),
            api: Some("https://example.test/v1".to_owned()),
            models,
        }
    }

    /// The whole point of the TOML split: a generated file survives
    /// being written and re-read with its meaning intact.
    fn round_trip(source: &SourceProvider) -> SourceProvider {
        let body = toml::to_string_pretty(&generated(source)).expect("renders");
        let read: GeneratedProvider = toml::from_str(&body).expect("re-reads");
        to_source(&source.id, &read, &CuratedProvider::default()).expect("converts")
    }

    #[test]
    fn a_non_thinking_model_survives_the_round_trip() {
        let source = provider("groq", vec![model("m")]);
        let back = round_trip(&source);
        assert_eq!(back.models[0].reasoning, Reasoning::None);
        assert_eq!(back.models[0].context, Some(200_000));
        assert_eq!(back.models[0].input, Modalities::of(&[Modality::Text]));
        assert_eq!(back.models[0].output, Some(64_000));
    }

    #[test]
    fn every_reasoning_shape_survives_the_round_trip() {
        for reasoning in [
            Reasoning::Fixed,
            Reasoning::Toggle,
            Reasoning::Effort {
                efforts: Efforts::of(&[Effort::Low, Effort::XHigh]),
                toggle: true,
            },
            Reasoning::Effort {
                efforts: Efforts::of(&[Effort::Max]),
                toggle: false,
            },
            Reasoning::Budget {
                min: 1024,
                max: Some(32768),
                toggle: true,
            },
            Reasoning::Budget {
                min: 0,
                max: None,
                toggle: false,
            },
        ] {
            let source = provider(
                "anthropic",
                vec![SourceModel {
                    reasoning,
                    ..model("m")
                }],
            );
            let back = round_trip(&source);
            assert_eq!(back.models[0].reasoning, reasoning, "{reasoning:?}");
        }
    }

    #[test]
    fn costs_survive_the_round_trip() {
        let source = provider(
            "anthropic",
            vec![SourceModel {
                cost: Some(ModelCost {
                    input: 3.0,
                    output: 15.0,
                    cache_read: Some(0.3),
                    cache_write: Some(3.75),
                }),
                ..model("m")
            }],
        );
        let back = round_trip(&source);
        assert_eq!(back.models[0].cost, source.models[0].cost);
    }

    #[test]
    fn modalities_and_limits_survive_the_round_trip() {
        let source = provider(
            "zai",
            vec![SourceModel {
                input: Modalities::of(&[Modality::Text, Modality::Image, Modality::Pdf]),
                produces: Modalities::of(&[Modality::Text, Modality::Image]),
                input_limit: Some(272_000),
                ..model("m")
            }],
        );
        let back = round_trip(&source);
        assert_eq!(back.models[0].input, source.models[0].input);
        assert_eq!(back.models[0].produces, source.models[0].produces);
        assert_eq!(back.models[0].input_limit, Some(272_000));
    }

    #[test]
    fn a_curated_field_reaches_a_model_the_upstream_leaves_blank() {
        let source = provider("commandcode", vec![model("vendor/m")]);
        let curated = CuratedProvider {
            interleaved: [("vendor/m".to_owned(), "reasoning_content".to_owned())]
                .into_iter()
                .collect(),
        };
        let back = to_source("commandcode", &generated(&source), &curated).unwrap();
        assert_eq!(
            back.models[0].interleaved,
            Some(ReasoningField::ReasoningContent)
        );
        // A curated field is the only thing this file holds, so a model
        // with none stays blank rather than inventing a default.
        assert_eq!(back.models[0].release_date, None);
    }

    #[test]
    fn an_upstream_date_reaches_the_bundle_untouched() {
        // The release date is upstream-published for every provider kage
        // supports, so it is never curated. kage uses it to pick a
        // provider's default model, so losing it would change which
        // model a fresh install starts on.
        let source = provider(
            "zai",
            vec![SourceModel {
                release_date: Some("2026-03-17".to_owned()),
                ..model("m")
            }],
        );
        let back = round_trip(&source);
        assert_eq!(back.models[0].release_date.as_deref(), Some("2026-03-17"));
        let json = provider_json("zai", &generated(&source), &CuratedProvider::default()).unwrap();
        assert_eq!(json["models"]["m"]["release_date"], "2026-03-17");
    }

    #[test]
    fn an_unknown_curated_field_is_an_error() {
        let source = provider("commandcode", vec![model("m")]);
        let curated = CuratedProvider {
            interleaved: [("m".to_owned(), "thought_bubbles".to_owned())]
                .into_iter()
                .collect(),
        };
        let err = to_source("commandcode", &generated(&source), &curated).unwrap_err();
        assert!(err.contains("thought_bubbles"), "{err}");
    }

    #[test]
    fn curated_entries_for_dropped_models_are_reported_and_dropped() {
        let curated = CuratedProvider {
            interleaved: [
                ("kept".to_owned(), "reasoning_content".to_owned()),
                ("also-gone".to_owned(), "reasoning_details".to_owned()),
            ]
            .into_iter()
            .collect(),
        };
        let live = vec!["kept".to_owned()];
        assert_eq!(curated.retired(&live), ["also-gone"]);
        let trimmed = curated.without_retired(&live);
        assert_eq!(trimmed.interleaved.len(), 1);
        assert!(trimmed.interleaved.contains_key("kept"));
    }

    #[test]
    fn an_empty_curation_is_reported_as_empty() {
        assert!(CuratedProvider::default().is_empty());
        assert!(
            !CuratedProvider {
                interleaved: [("m".to_owned(), "reasoning_content".to_owned())]
                    .into_iter()
                    .collect(),
            }
            .is_empty()
        );
    }

    #[test]
    fn a_model_name_equal_to_its_id_is_omitted_from_the_file() {
        let source = provider("groq", vec![model("m")]);
        let body = toml::to_string_pretty(&generated(&source)).unwrap();
        // The provider name is always written; a model name equal to
        // the id is not, since kage reads them the same way.
        assert!(body.contains("name = \"groq\""), "{body}");
        assert_eq!(body.matches("name =").count(), 1, "{body}");
        assert_eq!(body.matches("id = \"m\"").count(), 1, "{body}");
        let read: GeneratedProvider = toml::from_str(&body).unwrap();
        assert_eq!(read.models[0].name, None);
        // kage still gets the id as the display name.
        let back = to_source("groq", &read, &CuratedProvider::default()).unwrap();
        assert_eq!(back.models[0].name, "m");
    }

    #[test]
    fn a_thinking_model_without_a_level_keeps_the_fixed_shape() {
        let source = provider(
            "groq",
            vec![SourceModel {
                reasoning: Reasoning::Fixed,
                ..model("m")
            }],
        );
        let body = toml::to_string_pretty(&generated(&source)).unwrap();
        assert!(body.contains("thinks = true"), "{body}");
        assert!(
            !body.contains("efforts") && !body.contains("toggle"),
            "a fixed thinker declares no level: {body}"
        );
    }

    #[test]
    fn the_bundled_json_is_readable_by_kages_own_parser() {
        let source = provider(
            "anthropic",
            vec![SourceModel {
                reasoning: Reasoning::Effort {
                    efforts: Efforts::of(&[Effort::High]),
                    toggle: true,
                },
                release_date: Some("2026-01-02".to_owned()),
                ..model("claude-x")
            }],
        );
        let curated = CuratedProvider {
            interleaved: [("claude-x".to_owned(), "reasoning_content".to_owned())]
                .into_iter()
                .collect(),
        };
        let json = provider_json("anthropic", &generated(&source), &curated).unwrap();
        let mut root = serde_json::Map::new();
        root.insert("anthropic".into(), json);
        let parsed = source::parse(&serde_json::to_string(&root).unwrap()).unwrap();
        assert_eq!(parsed.len(), 1);
        let model = &parsed[0].models[0];
        assert_eq!(model.id, "claude-x");
        assert_eq!(model.reasoning, source.models[0].reasoning);
        assert_eq!(model.interleaved, Some(ReasoningField::ReasoningContent));
        assert_eq!(model.release_date.as_deref(), Some("2026-01-02"));
    }

    #[test]
    fn every_provider_kage_needs_is_listed() {
        let ids: Vec<&str> = provider_ids().iter().map(|(api, _)| *api).collect();
        for needed in ["anthropic", "commandcode", "kenari", "kenari-claude"] {
            assert!(ids.contains(&needed), "{needed} missing from {ids:?}");
        }
    }
}
