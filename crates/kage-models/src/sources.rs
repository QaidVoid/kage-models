//! Upstream model catalogs this repo curates.
//!
//! Five sources feed [`crate::generate`]. Three of them are the model
//! lists that decide what exists; two carry metadata those lists omit.

/// models.dev: the provider catalog kage's own schema is built on.
/// Only the providers `kage_provider::catalog::source::SUPPORTED_PROVIDERS`
/// names survive pruning, so the other ~210 are discarded on read.
pub const MODELS_DEV_URL: &str = "https://models.dev/api.json";

/// Kenari's public model list: context windows and served endpoints for
/// every model, no key needed.
pub const KENARI_MODELS_URL: &str = "https://kenari.id/v1/models";

/// Kenari's subscription plans. The union of their `scope_models` is the
/// model set the catalog lists for Kenari.
pub const KENARI_PLANS_URL: &str = "https://kenari.id/api/plans";

/// Command Code's public model list. The authority for which models
/// exist, their names, context windows, and which wire format each is
/// served on.
pub const COMMAND_CODE_MODELS_URL: &str = "https://api.commandcode.ai/provider/v1/models";

/// The official `command-code` CLI's generated model reference, served
/// from the npm package. The authority for reasoning effort levels, which
/// the endpoint above publishes nothing about.
pub const COMMAND_CODE_CLI_CATALOG_URL: &str = "https://cdn.jsdelivr.net/npm/command-code/dist/bundled/command-code-knowledge/reference/models.md";

/// Where models.dev serves one SVG per provider, as `<id>.svg`. The
/// source of the logos snapshotted under `logos/`; kage reads them from
/// kage-models instead, so this is only ever read at generation time.
pub const LOGOS_BASE_URL: &str = "https://models.dev/logos";
