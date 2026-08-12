use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillCardDecision {
    pub decision: String,
    pub canonical_hash: String,
    #[serde(default)]
    pub degradations: Vec<String>,
}

pub fn validate_skill_card(card: &Value) -> Result<SkillCardDecision> {
    let object = card
        .as_object()
        .ok_or_else(|| anyhow!("skill card must be an object"))?;
    let schema_version = required_string(object, "schema_version")?;
    required_string(object, "name")?;
    match schema_version {
        "0.1" => Ok(decision(
            "accept",
            &serde_json::json!({
                "schema_version": "0.1",
                "aliases": [],
                "intents": {"positive": [], "negative": []},
                "facets": {},
                "reviewed_relations": [],
                "host_compatibility": {},
                "provenance": {}
            }),
            Vec::new(),
        )?),
        "2alpha1" => validate_v2(object),
        _ => Err(anyhow!("unknown required skill-card schema version")),
    }
}

fn validate_v2(object: &serde_json::Map<String, Value>) -> Result<SkillCardDecision> {
    for key in ["version", "summary"] {
        required_string(object, key)?;
    }
    unique_strings(object.get("aliases"), false, true)?;
    let intents = required_object(object, "intents")?;
    unique_strings(intents.get("positive"), true, false)?;
    unique_strings(intents.get("negative"), false, false)?;
    let facets = required_object(object, "facets")?;
    for (key, value) in facets {
        if key.is_empty() {
            return Err(anyhow!("facet names must be non-empty"));
        }
        unique_strings(Some(value), true, false)?;
    }
    validate_relations(object.get("reviewed_relations"))?;
    let compatibility = required_object(object, "host_compatibility")?;
    unique_strings(compatibility.get("targets"), true, false)?;
    if let Some(value) = compatibility.get("minimum_metactl") {
        non_empty_string(value, "host_compatibility.minimum_metactl")?;
    }
    if let Some(value) = compatibility.get("notes") {
        unique_strings(Some(value), false, false)?;
    }
    let provenance = required_object(object, "provenance")?;
    let source_kind = required_string(provenance, "source_kind")?;
    if !["first_party", "vendored", "imported", "local"].contains(&source_kind) {
        return Err(anyhow!("invalid provenance.source_kind"));
    }
    required_string(provenance, "reviewed_by")?;
    required_string(provenance, "reviewed_at")?;
    if object.contains_key("requires_features") {
        return Err(anyhow!(
            "unknown required skill-card semantics: requires_features"
        ));
    }
    let known = BTreeSet::from([
        "schema_version",
        "name",
        "version",
        "summary",
        "aliases",
        "intents",
        "facets",
        "reviewed_relations",
        "host_compatibility",
        "provenance",
    ]);
    let degradations = object
        .keys()
        .filter(|key| !known.contains(key.as_str()))
        .map(|key| format!("unknown_optional:{key}"))
        .collect::<Vec<_>>();
    let normalized = serde_json::json!({
        "schema_version": "2alpha1",
        "aliases": object["aliases"].clone(),
        "intents": object["intents"].clone(),
        "facets": object["facets"].clone(),
        "reviewed_relations": object["reviewed_relations"].clone(),
        "host_compatibility": object["host_compatibility"].clone(),
        "provenance": object["provenance"].clone()
    });
    Ok(decision(
        if degradations.is_empty() {
            "accept"
        } else {
            "degrade"
        },
        &normalized,
        degradations,
    )?)
}

fn validate_relations(value: Option<&Value>) -> Result<()> {
    let items = value
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("reviewed_relations must be an array"))?;
    for item in items {
        let relation = item
            .as_object()
            .ok_or_else(|| anyhow!("reviewed relation must be an object"))?;
        let relation_type = required_string(relation, "type")?;
        if ![
            "alias_of",
            "complements",
            "requires",
            "see_also",
            "supersedes",
        ]
        .contains(&relation_type)
        {
            return Err(anyhow!("invalid reviewed relation type"));
        }
        for key in ["target", "reviewed_by", "reviewed_at"] {
            required_string(relation, key)?;
        }
        if relation.len() != 4 {
            return Err(anyhow!("unknown reviewed relation field"));
        }
    }
    Ok(())
}

fn required_object<'a>(
    object: &'a serde_json::Map<String, Value>,
    key: &str,
) -> Result<&'a serde_json::Map<String, Value>> {
    object
        .get(key)
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("{key} must be an object"))
}

fn required_string<'a>(object: &'a serde_json::Map<String, Value>, key: &str) -> Result<&'a str> {
    let value = object
        .get(key)
        .ok_or_else(|| anyhow!("missing required field {key}"))?;
    non_empty_string(value, key)
}

fn non_empty_string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .as_str()
        .filter(|value| !value.is_empty() && value.len() <= 4096)
        .ok_or_else(|| anyhow!("{key} must be a non-empty bounded string"))
}

fn unique_strings(value: Option<&Value>, non_empty: bool, casefold: bool) -> Result<()> {
    let items = value
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("expected string array"))?;
    if non_empty && items.is_empty() {
        return Err(anyhow!("expected non-empty string array"));
    }
    let mut seen = BTreeSet::new();
    for item in items {
        let item = non_empty_string(item, "array item")?;
        let key = if casefold {
            item.to_lowercase()
        } else {
            item.to_string()
        };
        if !seen.insert(key) {
            return Err(anyhow!("duplicate string array item"));
        }
    }
    Ok(())
}

fn decision(decision: &str, card: &Value, degradations: Vec<String>) -> Result<SkillCardDecision> {
    Ok(SkillCardDecision {
        decision: decision.to_string(),
        canonical_hash: canonical_hash(card)?,
        degradations,
    })
}

fn canonical_hash(card: &Value) -> Result<String> {
    fn canonicalize(value: &Value) -> Value {
        match value {
            Value::Object(object) => {
                let ordered = object
                    .iter()
                    .map(|(key, value)| (key.clone(), canonicalize(value)))
                    .collect::<BTreeMap<_, _>>();
                serde_json::to_value(ordered).expect("canonical map serializes")
            }
            Value::Array(items) => Value::Array(items.iter().map(canonicalize).collect()),
            _ => value.clone(),
        }
    }
    let bytes = serde_json::to_vec(&canonicalize(card))?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conformance_decisions_match_shared_cases() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../fixtures/skill-card/conformance-cases.json"
        ))
        .expect("fixture JSON");
        for case in fixture["cases"].as_array().expect("cases") {
            let expected = case["expected"].as_str().expect("expected");
            let result = validate_skill_card(&case["card"]);
            match expected {
                "reject" => assert!(result.is_err(), "{} should reject", case["id"]),
                expected => {
                    let result = result.expect("valid card");
                    assert_eq!(result.decision, expected, "{}", case["id"]);
                    assert_eq!(
                        result.canonical_hash,
                        case["expected_canonical_hash"]
                            .as_str()
                            .expect("expected hash"),
                        "{}",
                        case["id"]
                    );
                }
            }
        }
    }
}
