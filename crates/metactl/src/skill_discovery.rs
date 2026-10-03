//! Deterministic, project-policy-bound plain-instruction discovery.
//! Host ranking may reorder these results, never widen this eligibility set.
use super::*;
use serde::{Deserialize, Serialize};
use std::path::Component;

const MAX_RESOURCE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDescriptor {
    pub id: String,
    pub name: String,
    pub description: String,
    pub digest: String,
    pub aliases: Vec<String>,
    pub positive_intents: Vec<String>,
    pub negative_intents: Vec<String>,
    pub score: i64,
}

#[derive(Debug, Serialize)]
pub struct SkillCatalog {
    pub schema: String,
    pub catalog_digest: String,
    pub skills: Vec<SkillDescriptor>,
    pub excluded: usize,
}

#[derive(Debug, Serialize)]
pub struct LoadedSkill {
    pub id: String,
    pub digest: String,
    pub instructions: String,
    pub base_directory: String,
    pub resources: Vec<String>,
    pub activation: &'static str,
}

struct Entry {
    descriptor: SkillDescriptor,
    instructions: String,
    base: PathBuf,
    resources: Vec<String>,
    targets: Vec<String>,
}

fn skill_terms(text: &str) -> BTreeSet<String> {
    let mut terms = routing_terms(text);
    terms.retain(|term| {
        !matches!(
            term.as_str(),
            "a" | "an"
                | "and"
                | "are"
                | "as"
                | "at"
                | "be"
                | "before"
                | "after"
                | "by"
                | "do"
                | "for"
                | "from"
                | "how"
                | "i"
                | "in"
                | "is"
                | "it"
                | "its"
                | "me"
                | "my"
                | "of"
                | "on"
                | "or"
                | "please"
                | "that"
                | "the"
                | "their"
                | "these"
                | "this"
                | "those"
                | "to"
                | "use"
                | "using"
                | "was"
                | "we"
                | "with"
                | "you"
                | "your"
        )
    });
    terms
}

// Literal names/aliases remain useful inside prose. Identifier boundaries keep
// `some-skill` from becoming an explicit request for `some-skill-extended`.
fn skill_mention(query: &str, label: &str) -> Option<i64> {
    let label = label.to_ascii_lowercase();
    if label.trim().is_empty() {
        return None;
    }
    let identifier = |c: char| c.is_alphanumeric() || c == '-' || c == '_';
    let mut found = None;
    for (start, _) in query.match_indices(&label) {
        let end = start + label.len();
        if query[..start].chars().next_back().is_some_and(identifier)
            || query[end..].chars().next().is_some_and(identifier)
        {
            continue;
        }
        // Handle direct named exclusions, without interpreting general negation.
        let preceding = query[..start]
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        if [
            &["not"][..],
            &["not", "use"],
            &["don", "t", "use"],
            &["avoid"],
            &["without"],
            &["exclude"],
        ]
        .iter()
        .any(|suffix| preceding.ends_with(suffix))
        {
            return Some(-1);
        }
        // A bare task word (e.g. "review") is not an explicit skill request.
        // Preserve provider ranking unless a one-word label is marked as a name.
        if routing_terms(&label).len() == 1
            && !label.contains(['-', '_'])
            && !query[..start].ends_with(['$', '`'])
        {
            continue;
        }
        found = Some(10000 + routing_terms(&label).len().min(99) as i64);
    }
    found
}

fn skill_score(
    skill: &SkillDescriptor,
    original_query: &str,
    query: &str,
    terms: &BTreeSet<String>,
) -> i64 {
    if skill.name.eq_ignore_ascii_case(original_query.trim()) || skill.id == original_query.trim() {
        return 20000;
    }
    let mut explicit = 0;
    for label in std::iter::once(&skill.name).chain(skill.aliases.iter()) {
        if let Some(score) = skill_mention(query, label) {
            if score < 0 {
                return 0;
            }
            explicit = explicit.max(score);
        }
    }
    if explicit > 0 {
        return explicit;
    }
    let overlap = |field: &str| terms.intersection(&skill_terms(field)).count() as i64;
    // Bound repeated metadata by its strongest field instead of rewarding the
    // number of aliases/intents. Count meaningful distinct words within fields.
    let score = 12 * overlap(&skill.name)
        + 4 * overlap(&skill.description)
        + 10 * skill.aliases.iter().map(|s| overlap(s)).max().unwrap_or(0)
        + 6 * skill
            .positive_intents
            .iter()
            .map(|s| overlap(s))
            .max()
            .unwrap_or(0);
    let penalty = skill
        .negative_intents
        .iter()
        .map(|intent| {
            let negative = skill_terms(intent);
            if !negative.is_empty() && negative.is_subset(terms) {
                20 * negative.len() as i64
            } else {
                0
            }
        })
        .max()
        .unwrap_or(0);
    // The host reserves >=10000 for an explicit, provider-free selection.
    (score - penalty).min(9999)
}

fn safe_file(root: &Path, relative: &str) -> Result<PathBuf> {
    let relative = Path::new(relative);
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(anyhow!("unsafe declared resource path"));
    }
    let root = root.canonicalize()?;
    let path = root.join(relative).canonicalize()?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err(anyhow!("resource escapes library or is not a file"));
    }
    if fs::metadata(&path)?.len() > MAX_RESOURCE_BYTES {
        return Err(anyhow!("resource exceeds discovery size limit"));
    }
    Ok(path)
}

fn metadata(text: &str) -> Result<serde_yaml::Mapping> {
    let text = text
        .strip_prefix("---\n")
        .ok_or_else(|| anyhow!("missing frontmatter"))?;
    let (head, _) = text
        .split_once("\n---")
        .ok_or_else(|| anyhow!("unclosed frontmatter"))?;
    serde_yaml::from_str(head).context("invalid skill metadata")
}

fn string(map: &serde_yaml::Mapping, key: &str) -> Option<String> {
    map.get(serde_yaml::Value::String(key.into()))?
        .as_str()
        .map(str::to_owned)
}

fn bool_value(map: &serde_yaml::Mapping, key: &str) -> Option<bool> {
    map.get(serde_yaml::Value::String(key.into()))?.as_bool()
}

fn read_entry(pack: &DiscoveredPack, resource: &PackResource) -> Result<Entry> {
    let path = safe_file(&pack.library_root, &resource.path)?;
    // Deliberately bypass the mtime/length cache: unchanged timestamps do not
    // prove unchanged source. Load must never return synthetic fallback content.
    let instructions = fs::read_to_string(&path)?;
    let map = metadata(&instructions)?;
    for key in ["enabled", "disable-model-invocation"] {
        if map.contains_key(serde_yaml::Value::String(key.into()))
            && bool_value(&map, key).is_none()
        {
            return Err(anyhow!("invalid native invocation flag"));
        }
    }
    if bool_value(&map, "enabled") == Some(false)
        || bool_value(&map, "disable-model-invocation") == Some(true)
        || ["allowed-tools", "context", "agent", "hooks", "model"]
            .iter()
            .any(|key| map.contains_key(serde_yaml::Value::String((*key).into())))
    {
        return Err(anyhow!("native-only or disabled skill"));
    }
    let name = string(&map, "name")
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow!("missing name"))?;
    let description = string(&map, "description")
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow!("missing description"))?;
    let base = path
        .parent()
        .ok_or_else(|| anyhow!("missing base directory"))?
        .to_path_buf();
    let sidecar = base.join("agents/openai.yaml");
    if sidecar.exists() {
        // Native dependency/tool/implicit-invocation behavior is not emulated.
        return Err(anyhow!("native sidecar requires a native adapter"));
    }
    let package = Path::new(&resource.path)
        .parent()
        .ok_or_else(|| anyhow!("missing package"))?;
    let mut declared = pack
        .manifest
        .resources
        .iter()
        .filter(|r| Path::new(&r.path).starts_with(package))
        .collect::<Vec<_>>();
    declared.sort_by(|a, b| a.path.cmp(&b.path));
    let mut digest = Sha256::new();
    let mut resources = Vec::new();
    let mut card_bytes = None;
    for r in declared {
        let p = safe_file(&pack.library_root, &r.path)?;
        if !p.starts_with(&base) {
            return Err(anyhow!("resource escapes skill package"));
        }
        let bytes = if r.path == resource.path {
            instructions.as_bytes().to_vec()
        } else {
            fs::read(p)?
        };
        digest.update((r.path.len() as u64).to_le_bytes());
        digest.update(r.path.as_bytes());
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(&bytes);
        if Path::new(&r.path) == package.join("skill-card.json") {
            card_bytes = Some(bytes);
        }
        if r.path != resource.path {
            resources.push(
                Path::new(&r.path)
                    .strip_prefix(package)?
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    let (aliases, positive_intents, negative_intents, targets) = fresh_card(card_bytes.as_deref())?;
    let id = hex::encode(Sha256::digest(
        format!("{}\0{}", pack.manifest.pack_ref().key(), resource.path).as_bytes(),
    ));
    Ok(Entry {
        descriptor: SkillDescriptor {
            id,
            name,
            description,
            digest: hex::encode(digest.finalize()),
            aliases,
            positive_intents,
            negative_intents,
            score: 0,
        },
        instructions,
        base,
        resources,
        targets,
    })
}

type CardFields = (Vec<String>, Vec<String>, Vec<String>, Vec<String>);
fn fresh_card(bytes: Option<&[u8]>) -> Result<CardFields> {
    let Some(bytes) = bytes else {
        return Ok((vec![], vec![], vec![], vec![]));
    };
    let card: serde_json::Value = serde_json::from_slice(bytes)?;
    crate::skill_card::validate_skill_card(&card)?;
    // Dependency composition is not implemented by this plain-instruction adapter.
    if card
        .get("reviewed_relations")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|relations| {
            relations
                .iter()
                .any(|r| r["type"].as_str() == Some("requires"))
        })
    {
        return Err(anyhow!("native prerequisites unsupported"));
    }
    let strings = |v: &serde_json::Value| {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    Ok((
        strings(&card["aliases"]),
        strings(&card["intents"]["positive"]),
        strings(&card["intents"]["negative"]),
        strings(&card["host_compatibility"]["targets"]),
    ))
}

impl LibraryRegistry {
    fn discovery_entries(
        &self,
        config: &Config,
        overlay: Option<&InvocationOverlay>,
    ) -> Result<(Vec<Entry>, usize)> {
        let role = self.find_role(&config.role)?;
        let policy = self.find_policy(&config.policy)?;
        let target = self.selected_target(config, overlay)?;
        let mut entries = Vec::new();
        let mut excluded = 0;
        for pack in self.packs.values() {
            // Candidates and approval-required packs are never implicitly loaded,
            // even if exploratory search would show them as advisory results.
            let denied = pack.promotion_status != PromotionStatus::Promoted
                || pack
                    .provenance
                    .as_ref()
                    .and_then(|p| p.review.as_ref())
                    .and_then(|r| r.promotion_status.as_ref())
                    .is_some_and(|s| *s != PromotionStatus::Promoted)
                || pack.manifest.requires_confirmation
                || pack.manifest.activation_class != ActivationClass::Instruction
                || pack.manifest.side_effect_class != crate::types::SideEffectClass::None
                || pack.manifest.resources.iter().any(|r| {
                    matches!(
                        r.kind,
                        ResourceKind::Hook
                            | ResourceKind::HookWiring
                            | ResourceKind::Plugin
                            | ResourceKind::Subagent
                    )
                })
                || policy.rules.iter().any(|r| {
                    r.subject == PolicySubject::Pack
                        && r.operator == PolicyOperator::RequireApproval
                        && selectors_match(r.selectors.as_ref(), &pack.manifest)
                })
                || self
                    .suppression_reason(pack, role, policy, &target, &DiscoveryMode::CuratedOnly)
                    .is_some();
            for resource in &pack.manifest.resources {
                if !resource.path.ends_with("/SKILL.md") {
                    continue;
                }
                let candidate = (|| -> Result<Entry> {
                    if denied || resource.kind != ResourceKind::Instruction {
                        return Err(anyhow!("ineligible pack"));
                    }
                    if let Some(selection) = config
                        .defaults
                        .as_ref()
                        .and_then(|d| d.auto_surface_selection.as_ref())
                    {
                        // Conservative until per-surface native semantics are supported:
                        // any blocked surface excludes this pack, never reactivates it.
                        if selection
                            .blocked_surface_ids
                            .iter()
                            .any(|s| s.starts_with(&format!("{}:", pack.manifest.id)))
                        {
                            return Err(anyhow!("blocked surface"));
                        }
                    }
                    let e = read_entry(pack, resource)?;
                    if !e.targets.is_empty() && !e.targets.contains(&target.id) {
                        return Err(anyhow!("incompatible card target"));
                    }
                    Ok(e)
                })();
                match candidate {
                    Ok(e) => entries.push(e),
                    Err(_) => excluded += 1,
                }
            }
        }
        entries.sort_by(|a, b| a.descriptor.id.cmp(&b.descriptor.id));
        Ok((entries, excluded))
    }

    pub fn skill_catalog(
        &self,
        config: &Config,
        overlay: Option<&InvocationOverlay>,
    ) -> Result<SkillCatalog> {
        let fresh = Self::load_from_roots(&self.roots)?;
        if effective_discovery_mode(config, fresh.find_policy(&config.policy)?)
            == DiscoveryMode::None
        {
            return Err(anyhow!("discovery disabled by configuration"));
        }
        let (entries, excluded) = fresh.discovery_entries(config, overlay)?;
        let skills = entries
            .into_iter()
            .map(|e| e.descriptor)
            .collect::<Vec<_>>();
        let catalog_digest = hex::encode(Sha256::digest(serde_json::to_vec(&(
            config, overlay, &skills,
        ))?));
        Ok(SkillCatalog {
            schema: "metactl.skill_discovery.v1".into(),
            catalog_digest,
            skills,
            excluded,
        })
    }

    pub fn discover_skills(
        &self,
        config: &Config,
        overlay: Option<&InvocationOverlay>,
        query: &str,
        limit: usize,
        excluded: &BTreeSet<String>,
    ) -> Result<SkillCatalog> {
        if query.len() > 8192 || !(1..=20).contains(&limit) {
            return Err(anyhow!("query or limit outside discovery bounds"));
        }
        let mut catalog = self.skill_catalog(config, overlay)?;
        catalog
            .skills
            .retain(|s| !excluded.contains(&s.id) && !excluded.contains(&s.name));
        let normalized_query = query.to_ascii_lowercase();
        let terms = skill_terms(&normalized_query);
        catalog.skills.iter_mut().for_each(|s| {
            s.score = skill_score(s, query, &normalized_query, &terms);
        });
        catalog.skills.retain(|s| s.score > 0);
        catalog
            .skills
            .sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        catalog.skills.truncate(limit);
        Ok(catalog)
    }

    pub fn load_discovered_skill(
        &self,
        config: &Config,
        overlay: Option<&InvocationOverlay>,
        id: &str,
        expected_digest: &str,
    ) -> Result<LoadedSkill> {
        let fresh = Self::load_from_roots(&self.roots)?;
        if effective_discovery_mode(config, fresh.find_policy(&config.policy)?)
            == DiscoveryMode::None
        {
            return Err(anyhow!("discovery disabled by configuration"));
        }
        let (entries, _) = fresh.discovery_entries(config, overlay)?;
        let e = entries
            .into_iter()
            .find(|e| e.descriptor.id == id)
            .ok_or_else(|| anyhow!("unknown or ineligible skill"))?;
        if e.descriptor.digest != expected_digest {
            return Err(anyhow!("stale skill digest; discover again"));
        }
        Ok(LoadedSkill {
            id: e.descriptor.id,
            digest: e.descriptor.digest,
            instructions: e.instructions,
            base_directory: e.base.to_string_lossy().into_owned(),
            resources: e.resources,
            activation: "plain-instructions-only; host permissions unchanged",
        })
    }
}
