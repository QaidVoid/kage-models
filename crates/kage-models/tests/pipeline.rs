//! End-to-end tests over recorded upstream bodies.
//!
//! Every test runs against `tests/fixtures/`, so the suite needs no
//! network and is deterministic. The fixtures are hand-shaped to cover
//! the cases that matter: what gets pruned, what gets routed to which
//! provider, what is left out, and what survives from the curation.

use kage_models::build::{self, Curated};
use kage_models::catalog::{self, CuratedProvider};
use kage_provider::catalog::source::{self, SUPPORTED_PROVIDERS};
use serde_json::Value;

fn read(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The models.dev fixture with a `kenari` entry, which the Kenari pass
/// reads display metadata from.
fn models_dev() -> String {
    let mut dev: serde_json::Map<String, Value> =
        serde_json::from_str(&read("models_dev.json")).expect("models_dev parses");
    dev.insert(
        "kenari".to_owned(),
        serde_json::json!({ "name": "Kenari", "models": {} }),
    );
    serde_json::to_string(&dev).expect("render")
}

/// Assemble the upstream bodies from named fixtures.
fn inputs(models_fixture: &str, cli_fixture: &str) -> build::Inputs {
    build::Inputs {
        models_dev: models_dev(),
        kenari_models: read("kenari_models.json"),
        kenari_plans: read("kenari_plans.json"),
        command_code_models: read(models_fixture),
        command_code_cli: read(cli_fixture),
    }
}

/// The default pairing: both Command Code upstreams agree.
fn build_with(curation: &Curated) -> build::Built {
    build::build(
        &inputs("command_code_models.json", "command_code_cli.md"),
        curation,
    )
    .expect("build succeeds")
}

/// The bundled JSON a build produces, as kage's parser reads it.
fn bundled(built: &build::Built) -> Vec<source::SourceProvider> {
    let mut root = serde_json::Map::new();
    for provider in &built.providers {
        let json = catalog::provider_json(
            &provider.provider_id,
            &provider.generated,
            &provider.curated,
        )
        .expect("bundles");
        root.insert(provider.provider_id.clone(), json);
    }
    source::parse(&serde_json::to_string(&root).expect("render")).expect("kage parses it")
}

/// One provider's bundled JSON entry.
fn provider_json(built: &build::Built, provider_id: &str) -> Value {
    let provider = built
        .providers
        .iter()
        .find(|p| p.provider_id == provider_id)
        .unwrap_or_else(|| panic!("{provider_id} was not built"));
    catalog::provider_json(provider_id, &provider.generated, &provider.curated).expect("bundles")
}

/// One provider's generated entries.
fn generated_of<'a>(built: &'a build::Built, provider_id: &str) -> &'a catalog::GeneratedProvider {
    &built
        .providers
        .iter()
        .find(|p| p.provider_id == provider_id)
        .unwrap_or_else(|| panic!("{provider_id} was not built"))
        .generated
}

#[test]
fn a_new_command_code_model_needs_no_hand_edit() {
    // The failure this repo exists to remove: kage's own generator
    // refused to run until every new upstream model was copied into a
    // committed fixture by hand. Here the endpoint alone decides a model
    // exists, and the CLI catalog alone decides how it thinks.
    let built = build_with(&Curated::default());
    let row = provider_json(&built, "commandcode")["models"]["vendor/chat-thinker"].clone();
    assert_eq!(
        row["name"], "Chat Thinker",
        "a model the endpoint lists that no committed file has seen is still published"
    );
    assert_eq!(row["limit"]["context"], 1_000_000, "from the endpoint");
    assert_eq!(row["reasoning"], true, "from the CLI catalog");
    assert_eq!(
        row["reasoning_options"],
        serde_json::json!([
            { "type": "toggle" },
            { "type": "effort", "values": ["high", "max"] }
        ]),
        "`off` becomes a toggle, not an effort value"
    );
}

#[test]
fn a_claude_model_is_filed_under_the_messages_wire() {
    let built = build_with(&Curated::default());
    let json = provider_json(&built, "commandcode");
    assert!(
        json["models"].get("vendor/claude-1").is_none(),
        "a model served only on /messages does not belong under the chat provider"
    );
    let row = &provider_json(&built, "commandcode-claude")["models"]["vendor/claude-1"];
    assert_eq!(row["name"], "Claude 1");
    assert_eq!(row["reasoning"], true);
    assert_eq!(
        row["reasoning_options"],
        serde_json::json!([{ "type": "effort", "values": ["low", "high"] }]),
        "no `off` in the row, so no toggle"
    );
}

#[test]
fn a_model_published_without_efforts_does_not_think() {
    let built = build_with(&Curated::default());
    let row = &provider_json(&built, "commandcode")["models"]["vendor/chat-plain"];
    assert_eq!(
        row["reasoning"], false,
        "an em dash means no thinking setting"
    );
    assert!(
        row.get("reasoning_options").is_none(),
        "a non-thinking model carries no options to pick from"
    );
}

#[test]
fn an_effort_kage_cannot_represent_is_an_error() {
    // Publishing `turbo` would mean kage offers a thinking level its
    // wire format cannot send. Failing is right: it is a real gap in
    // kage, and guessing would hide it behind a published catalog.
    let err = build::build(
        &inputs("command_code_models.json", "command_code_cli_bad_effort.md"),
        &Curated::default(),
    )
    .expect_err("build fails");
    assert!(
        err.contains("unknown effort 'turbo'"),
        "expected an unknown-effort failure, got: {err}"
    );
}

#[test]
fn a_model_the_cli_catalog_has_not_published_yet_is_left_out_and_reported() {
    // The two upstreams publish on their own schedules, so one can lead.
    // Left out rather than published with a guessed shape: kage would
    // offer it as non-thinking and silently drop its thinking.
    let built = build::build(
        &inputs(
            "command_code_models_endpoint_leads.json",
            "command_code_cli.md",
        ),
        &Curated::default(),
    )
    .expect("build succeeds");
    assert_eq!(
        built.warnings.len(),
        1,
        "only the model that leads is reported: {built:?}"
    );
    assert!(
        built.warnings[0].contains("vendor/endpoint-leads"),
        "the model is named: {built:?}"
    );
    let json = provider_json(&built, "commandcode");
    assert!(json["models"].get("vendor/endpoint-leads").is_none());
    assert!(
        json["models"].get("vendor/chat-thinker").is_some(),
        "the models both upstreams agree on are unaffected"
    );
}

#[test]
fn a_model_the_endpoint_has_not_published_yet_is_left_out_and_reported() {
    // The reverse disagreement, same treatment.
    let built = build::build(
        &inputs(
            "command_code_models.json",
            "command_code_cli_endpoint_lags.md",
        ),
        &Curated::default(),
    )
    .expect("build succeeds");
    assert_eq!(built.warnings.len(), 1, "{built:?}");
    assert!(
        built.warnings[0].contains("vendor/catalog-leads"),
        "{built:?}"
    );
    let json = provider_json(&built, "commandcode");
    assert!(json["models"].get("vendor/catalog-leads").is_none());
}

#[test]
fn a_curated_field_reaches_the_bundled_json() {
    // The one field no upstream publishes, kept by hand in a small
    // curated file rather than mixed into the generated one.
    let mut curation = Curated::default();
    curation.insert(
        "commandcode".to_owned(),
        CuratedProvider {
            interleaved: [(
                "vendor/chat-thinker".to_owned(),
                "reasoning_content".to_owned(),
            )]
            .into_iter()
            .collect(),
        },
    );
    let built = build_with(&curation);
    let row = &provider_json(&built, "commandcode")["models"]["vendor/chat-thinker"];
    assert_eq!(row["interleaved"]["field"], "reasoning_content");
    // The generated file carries no observed field: a reviewer can tell
    // at a glance which values are derived and which are hand-kept.
    let generated = generated_of(&built, "commandcode");
    let entry = generated
        .models
        .iter()
        .find(|m| m.id == "vendor/chat-thinker")
        .unwrap();
    assert_eq!(entry.interleaved, None);
}

#[test]
fn an_upstream_interleaved_field_is_carried_through() {
    // models.dev publishes `interleaved` for some providers, so the
    // generated file holds it and the bundle must not drop it.
    let generated = catalog::GeneratedProvider {
        name: "DeepSeek".to_owned(),
        api: None,
        models: vec![catalog::ModelEntry {
            id: "deepseek-v4-pro".to_owned(),
            interleaved: Some("reasoning_content".to_owned()),
            ..Default::default()
        }],
    };
    let json = catalog::provider_json("deepseek", &generated, &CuratedProvider::default()).unwrap();
    assert_eq!(
        json["models"]["deepseek-v4-pro"]["interleaved"]["field"],
        "reasoning_content"
    );
}

#[test]
fn a_curated_field_overrides_the_upstream_value() {
    // The curated file exists for the rows the upstream gets wrong, so
    // its value wins over the published one.
    let generated = catalog::GeneratedProvider {
        name: "Command Code".to_owned(),
        api: None,
        models: vec![catalog::ModelEntry {
            id: "vendor/m".to_owned(),
            interleaved: Some("reasoning_content".to_owned()),
            ..Default::default()
        }],
    };
    let curated = CuratedProvider {
        interleaved: [("vendor/m".to_owned(), "reasoning_details".to_owned())]
            .into_iter()
            .collect(),
    };
    let json = catalog::provider_json("commandcode", &generated, &curated).unwrap();
    assert_eq!(
        json["models"]["vendor/m"]["interleaved"]["field"],
        "reasoning_details"
    );
}

#[test]
fn a_curated_field_for_a_dropped_model_is_dropped() {
    let mut curation = Curated::default();
    curation.insert(
        "commandcode".to_owned(),
        CuratedProvider {
            interleaved: [(
                "vendor/retired-upstream".to_owned(),
                "reasoning_content".to_owned(),
            )]
            .into_iter()
            .collect(),
        },
    );
    let built = build_with(&curation);
    let provider = built
        .providers
        .iter()
        .find(|p| p.provider_id == "commandcode")
        .unwrap();
    assert!(
        !provider
            .curated
            .interleaved
            .contains_key("vendor/retired-upstream"),
        "a curated field for a model the endpoint dropped must not linger: {:?}",
        provider.curated
    );
    assert!(
        provider.curated.is_empty(),
        "so the curated file would not be written at all"
    );
}

#[test]
fn kenari_lists_only_what_its_plans_cover() {
    let built = build_with(&Curated::default());
    let json = provider_json(&built, "kenari");
    let models = json["models"].as_object().expect("kenari models");
    assert_eq!(
        models.len(),
        1,
        "only the covered, tool-calling, chat-served model: {models:?}"
    );
    assert!(models.contains_key("kenari-covered"));
    for excluded in ["not-plan-covered", "no-tools", "chat-not-served"] {
        assert!(
            !models.contains_key(excluded),
            "{excluded} is served but is not a tool-calling chat model on a covering plan"
        );
    }
    let row = &models["kenari-covered"];
    assert_eq!(row["limit"]["output"], 32000, "from the model list");
    assert_eq!(
        row["modalities"]["input"],
        serde_json::json!(["text", "image"]),
        "`file` is dropped: kage cannot send it"
    );
    assert_eq!(
        row["reasoning_options"],
        serde_json::json!([
            { "type": "toggle" },
            { "type": "effort", "values": ["low", "high"] }
        ])
    );
}

#[test]
fn a_claude_model_at_kenari_lands_under_the_messages_provider() {
    let built = build_with(&Curated::default());
    assert!(
        provider_json(&built, "kenari")["models"]
            .get("claude-covered")
            .is_none()
    );
    assert!(
        provider_json(&built, "kenari-claude")["models"]
            .get("claude-covered")
            .is_some()
    );
    assert_eq!(
        provider_json(&built, "kenari-claude")["api"],
        "https://kenari.id"
    );
}

#[test]
fn the_bundle_carries_every_provider_kage_needs() {
    let providers = bundled(&build_with(&Curated::default()));
    for map in SUPPORTED_PROVIDERS {
        assert!(
            providers.iter().any(|p| p.id == map.kage_id),
            "kage reads no models for provider '{}'",
            map.kage_id
        );
    }
    // The curated providers are the reason this repo exists: kage's own
    // prune drops them, so they reach the runtime only via this feed.
    for id in [
        "commandcode",
        "commandcode-claude",
        "kenari",
        "kenari-claude",
    ] {
        let found = providers
            .iter()
            .find(|p| p.id == id)
            .unwrap_or_else(|| panic!("{id} missing"));
        assert!(!found.models.is_empty(), "{id} published no models");
    }
}

#[test]
fn providers_kage_does_not_support_are_dropped() {
    let built = build_with(&Curated::default());
    assert!(
        !built
            .providers
            .iter()
            .any(|p| p.provider_id == "not-a-kage-provider"),
        "models.dev carries ~210 providers kage has no implementation for"
    );
}

#[test]
fn a_model_without_tool_calling_is_dropped() {
    let built = build_with(&Curated::default());
    let json = provider_json(&built, "anthropic");
    assert!(json["models"].get("claude-fixture-no-tools").is_none());
}

#[test]
fn a_catalog_missing_a_provider_kage_needs_is_an_error() {
    // The guard that stops a broken upstream from publishing a catalog
    // that silently drops a provider kage depends on.
    let mut dev: serde_json::Map<String, Value> =
        serde_json::from_str(&models_dev()).expect("models_dev parses");
    dev.remove("anthropic");
    let err = build::build(
        &build::Inputs {
            models_dev: serde_json::to_string(&dev).expect("render"),
            kenari_models: read("kenari_models.json"),
            kenari_plans: read("kenari_plans.json"),
            command_code_models: read("command_code_models.json"),
            command_code_cli: read("command_code_cli.md"),
        },
        &Curated::default(),
    )
    .expect_err("build fails");
    assert!(
        err.contains("missing provider 'anthropic'"),
        "expected a missing-provider failure, got: {err}"
    );
}

#[test]
fn a_second_run_is_idempotent() {
    // Every run rewrites the generated files, so the build must be a
    // fixed point. Otherwise the scheduled workflow would commit a diff
    // on every run forever.
    let first = build_with(&Curated::default());
    assert!(first.warnings.is_empty(), "fixtures are consistent");
    let second = build_with(&Curated::default());
    assert_eq!(first.providers.len(), second.providers.len());
    for (a, b) in first.providers.iter().zip(&second.providers) {
        assert_eq!(a.provider_id, b.provider_id);
        assert_eq!(a.generated, b.generated, "{} is not stable", a.provider_id);
        assert_eq!(a.curated, b.curated, "{} is not stable", a.provider_id);
    }
}
