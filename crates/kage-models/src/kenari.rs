//! Rebuilding the `kenari` and `kenari-claude` rows.
//!
//! Kenari's own two endpoints are the authority for what exists and what
//! it serves; models.dev supplies the display metadata those endpoints
//! omit. Nothing here is hand-maintained.

use kage_core::Modality;
use serde_json::Value;

/// The `kenari` and `kenari-claude` providers, rebuilt from Kenari's
/// live model list and plans.
///
/// A row is every plan-covered tool-calling chat model the list serves,
/// with Claude models under `kenari-claude`. Context, output, inputs and
/// thinking come from the model list. The list names models by id, so
/// names, release dates and the interleaved reasoning field come from the
/// models.dev `kenari` entry when it carries the model. No cost is
/// written: plan usage is not billed per token.
///
/// # Errors
///
/// The model list or plans body does not parse, or the plans body is not
/// an array.
pub fn sync(
    live: &str,
    plans: &str,
    models_dev: &str,
) -> Result<serde_json::Map<String, Value>, String> {
    let mut root = serde_json::Map::new();
    let live: Value = serde_json::from_str(live).map_err(|e| format!("parse live models: {e}"))?;
    let plans: Value = serde_json::from_str(plans).map_err(|e| format!("parse plans: {e}"))?;
    let models_dev: Value =
        serde_json::from_str(models_dev).map_err(|e| format!("parse catalog: {e}"))?;
    let known = &models_dev["kenari"]["models"];

    let covered: std::collections::BTreeSet<&str> = plans
        .as_array()
        .ok_or("plans body is not an array")?
        .iter()
        .flat_map(|plan| strs(plan.get("scope_models")))
        .collect();

    let mut rows: [std::collections::BTreeMap<String, Value>; 2] = Default::default();
    for item in live
        .get("data")
        .and_then(Value::as_array)
        .ok_or("live model list has no `data` array")?
    {
        let Some(id) = item.get("id").and_then(Value::as_str) else {
            continue;
        };
        // Kenari serves models it does not describe: skip anything the
        // list cannot give a context window for.
        let Some(context) = item.get("context_length").and_then(Value::as_u64) else {
            continue;
        };
        let on_chat = strs(item.get("endpoints")).contains(&"chat");
        let tools = item.get("tool_call").and_then(Value::as_bool) == Some(true);
        if !covered.contains(id) || !on_chat || !tools {
            continue;
        }
        let known = &known[id];
        let mut row = serde_json::Map::new();
        row.insert("id".into(), id.into());
        if let Some(interleaved) = known.get("interleaved").filter(|v| v.is_object()) {
            row.insert("interleaved".into(), interleaved.clone());
        }
        let mut limit = serde_json::json!({ "context": context });
        if let Some(output) = item.get("max_output_tokens").and_then(Value::as_u64) {
            limit["output"] = output.into();
        }
        row.insert("limit".into(), limit);
        // Keep only modalities kage knows, so the catalog cannot offer
        // an input the wire format cannot carry.
        let modalities = item.get("modalities");
        let mut inputs = strs(modalities.and_then(|m| m.get("input")));
        inputs.retain(|input| Modality::parse(input).is_some());
        let mut outputs = strs(modalities.and_then(|m| m.get("output")));
        outputs.retain(|output| Modality::parse(output).is_some());
        if outputs.is_empty() {
            outputs.push("text");
        }
        row.insert(
            "modalities".into(),
            serde_json::json!({ "input": inputs, "output": outputs }),
        );
        let name = known.get("name").and_then(Value::as_str).unwrap_or(id);
        row.insert("name".into(), name.into());
        let thinks = item.get("reasoning").and_then(Value::as_bool) == Some(true);
        row.insert("reasoning".into(), thinks.into());
        let mut options = Vec::new();
        if item.get("reasoning_toggle").and_then(Value::as_bool) == Some(true) {
            options.push(serde_json::json!({ "type": "toggle" }));
        }
        let efforts = strs(item.get("reasoning_options"));
        if !efforts.is_empty() {
            options.push(serde_json::json!({ "type": "effort", "values": efforts }));
        }
        if thinks && !options.is_empty() {
            row.insert("reasoning_options".into(), options.into());
        }
        if let Some(date) = known.get("release_date").filter(|v| v.is_string()) {
            row.insert("release_date".into(), date.clone());
        }
        row.insert("tool_call".into(), true.into());
        let slot = usize::from(id.starts_with("claude-"));
        rows[slot].insert(id.to_owned(), row.into());
    }

    let [chat, claude] = rows;
    for (id, name, api, models) in [
        ("kenari", "Kenari", "https://kenari.id/v1", chat),
        (
            "kenari-claude",
            "Kenari (Claude)",
            "https://kenari.id",
            claude,
        ),
    ] {
        root.insert(
            id.to_owned(),
            serde_json::json!({
                "api": api,
                "doc": "https://kenari.id/en/docs",
                "env": ["KENARI_API_KEY"],
                "id": id,
                "models": models,
                "name": name,
            }),
        );
    }
    Ok(root)
}

/// The strings in `value`, or none when it is not an array.
fn strs(value: Option<&Value>) -> Vec<&str> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}
