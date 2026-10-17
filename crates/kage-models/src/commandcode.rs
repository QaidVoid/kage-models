//! Rebuilding the `commandcode` and `commandcode-claude` rows.
//!
//! Two upstreams between them carry everything derivable: the endpoint
//! publishes which models exist, their names, context windows and wire
//! placement; the CLI catalog publishes the reasoning effort levels.
//! Nothing here is hand-maintained. The two fields neither publishes, a
//! release date and an observed interleaved reasoning field, live in the
//! curated TOML instead.
//!
//! kage's own generator used to fail the run whenever the two disagreed
//! with a committed fixture, which meant every new upstream model cost a
//! hand edit. Here a new model is absorbed: the endpoint decides it
//! exists, the CLI catalog decides how it thinks, and the curated
//! fields simply stay absent until someone adds them.

use std::collections::BTreeMap;

use kage_core::Effort;
use serde_json::Value;

/// What one Command Code sync pass produced.
pub struct Synced {
    /// The overlay with both providers rebuilt.
    pub manual: serde_json::Map<String, Value>,
    /// Models left out, with why. Non-fatal: the endpoint and the CLI
    /// catalog publish on their own schedules, so a disagreement is
    /// normal and resolves on a later run.
    pub warnings: Vec<String>,
}

/// Rebuild the `commandcode` and `commandcode-claude` rows.
///
/// # Errors
///
/// A body does not parse, or a model the endpoint and the CLI catalog
/// both list is filed under the wrong provider for the wire format its
/// CLI section serves. That disagreement means kage would offer a model
/// on a format it is not served on, which no later run can fix.
pub fn sync(live: &str, cli_md: &str) -> Result<Synced, String> {
    let cli = parse_cli_catalog(cli_md)?;
    let live: Value = serde_json::from_str(live).map_err(|e| format!("parse live models: {e}"))?;
    let live_items = live
        .get("data")
        .and_then(Value::as_array)
        .ok_or("live model list has no `data` array")?;

    let mut rows: [BTreeMap<String, Value>; 2] = Default::default();
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let mut warnings: Vec<String> = Vec::new();
    for item in live_items {
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .ok_or("live model without an `id`")?;
        seen.insert(id);
        // No CLI row means no published reasoning shape. The model is
        // left out rather than published with a guessed one: kage would
        // offer it as non-thinking and silently drop its thinking, and
        // publishing it wrong is worse than publishing it late. The next
        // run picks it up once the npm package lists it.
        let Some(cli_row) = cli.get(id) else {
            warnings.push(format!(
                "{id}: listed by the model endpoint but not the CLI catalog, left out until it is"
            ));
            continue;
        };
        // The endpoint publishes nothing about reasoning, so the CLI
        // catalog decides. It also decides placement: the Anthropic
        // section is the Messages wire, everything else Chat Completions.
        // The two must agree, or a model would be offered on a format
        // it is not served on.
        let expected = if cli_row.anthropic {
            "commandcode-claude"
        } else {
            "commandcode"
        };
        let on_chat = item
            .get("supported_endpoints")
            .and_then(Value::as_array)
            .is_some_and(|endpoints| {
                endpoints
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|endpoint| endpoint == "/chat/completions")
            });
        let by_endpoint = if on_chat {
            "commandcode"
        } else {
            "commandcode-claude"
        };
        if expected != by_endpoint {
            return Err(format!(
                "'{id}': endpoint serves {by_endpoint}, CLI catalog says {expected}"
            ));
        }

        let slot = usize::from(expected == "commandcode-claude");
        rows[slot].insert(id.to_owned(), row(item, cli_row).into());
    }
    for id in cli.keys() {
        // The endpoint is the authority on what is served, so a model it
        // does not list is simply not offered. That is the reverse of
        // the case above and resolves the same way.
        if !seen.contains(id.as_str()) {
            warnings.push(format!(
                "{id}: listed by the CLI catalog but not the model endpoint, left out until it is"
            ));
        }
    }

    Ok(Synced {
        manual: providers(rows),
        warnings,
    })
}

/// Both Command Code providers, from their rebuilt rows. The provider
/// metadata is not upstream data: both endpoints serve the same two
/// bases and read the same credential.
fn providers(rows: [BTreeMap<String, Value>; 2]) -> serde_json::Map<String, Value> {
    let [chat, claude] = rows;
    let mut root = serde_json::Map::new();
    for (id, name, api, models) in [
        (
            "commandcode",
            "Command Code",
            "https://api.commandcode.ai/provider/v1",
            chat,
        ),
        (
            "commandcode-claude",
            "Command Code (Claude)",
            "https://api.commandcode.ai/provider",
            claude,
        ),
    ] {
        root.insert(
            id.to_owned(),
            serde_json::json!({
                "api": api,
                "doc": "https://commandcode.ai/docs/provider",
                "env": ["CMD_API_KEY"],
                "id": id,
                "models": models,
                "name": name,
            }),
        );
    }
    root
}

/// One published row, built from the endpoint's entry and the CLI
/// catalog's effort levels. Nothing here is hand-written: a field
/// neither upstream publishes is absent rather than guessed.
fn row(item: &Value, cli_row: &CliRow) -> serde_json::Map<String, Value> {
    let id = item.get("id").and_then(Value::as_str).unwrap_or_default();
    let mut row = serde_json::Map::new();
    row.insert("id".into(), id.into());
    let name = item.get("name").and_then(Value::as_str).unwrap_or(id);
    row.insert("name".into(), name.into());
    let mut limit = serde_json::Map::new();
    if let Some(context) = item.get("context_length").and_then(Value::as_u64) {
        limit.insert("context".into(), context.into());
    }
    if !limit.is_empty() {
        row.insert("limit".into(), limit.into());
    }
    row.insert(
        "modalities".into(),
        serde_json::json!({ "input": ["text"], "output": ["text"] }),
    );
    match &cli_row.efforts {
        None => {
            row.insert("reasoning".into(), Value::Bool(false));
        }
        Some(efforts) => {
            row.insert("reasoning".into(), Value::Bool(true));
            let mut options = Vec::new();
            if efforts.toggle {
                options.push(serde_json::json!({ "type": "toggle" }));
            }
            options.push(serde_json::json!({ "type": "effort", "values": efforts.values }));
            row.insert("reasoning_options".into(), options.into());
        }
    }
    row.insert("tool_call".into(), true.into());
    row
}

/// The effort settings one CLI catalog row publishes.
struct CliEfforts {
    /// Effort values the model accepts, with `off` removed.
    values: Vec<String>,
    /// The row listed `off`, so thinking can be switched off.
    toggle: bool,
}

/// One row of the CLI catalog's model tables.
struct CliRow {
    /// The Anthropic section lists the models served on the Messages
    /// wire; every other section is Chat Completions.
    anthropic: bool,
    /// The row's Efforts column. `None` when it reads an em dash: the
    /// CLI sends no thinking setting and the model decides its own depth.
    efforts: Option<CliEfforts>,
}

/// Parse the `command-code` CLI's generated models reference: markdown
/// tables under vendor sections, one row per model.
///
/// # Errors
///
/// A row lists an effort `kage_core::Effort` does not know.
fn parse_cli_catalog(md: &str) -> Result<BTreeMap<String, CliRow>, String> {
    let mut rows = BTreeMap::new();
    let mut anthropic = false;
    for line in md.lines() {
        if let Some(section) = line.strip_prefix("## ") {
            anthropic = section.trim() == "Anthropic";
            continue;
        }
        let Some(row) = line.strip_prefix("| `") else {
            continue;
        };
        let mut cells = row.split('|');
        let id = cells
            .next()
            .unwrap_or_default()
            .trim()
            .trim_end_matches('`');
        let _name = cells.next();
        let _context = cells.next();
        let efforts_cell = cells.next().map(str::trim).unwrap_or_default();
        let efforts = if efforts_cell == "\u{2014}" || efforts_cell.is_empty() {
            None
        } else {
            let mut toggle = false;
            let mut values = Vec::new();
            for token in efforts_cell.split(',').map(str::trim) {
                if token == "off" {
                    toggle = true;
                } else if Effort::parse(token).is_some() {
                    values.push(token.to_owned());
                } else {
                    return Err(format!(
                        "CLI catalog row '{id}' lists unknown effort '{token}'"
                    ));
                }
            }
            if values.is_empty() {
                return Err(format!("CLI catalog row '{id}' lists no efforts"));
            }
            Some(CliEfforts { values, toggle })
        };
        rows.insert(id.to_owned(), CliRow { anthropic, efforts });
    }
    Ok(rows)
}
