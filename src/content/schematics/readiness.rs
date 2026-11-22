//! Additional checks for resolved legacy schematic references.

use super::super::{Error, Source, items::inspect_prototype, read_bounded};
use super::audit::{SchematicAudit, audit_all_schematics, audit_schematics};
use super::{ItemResolution, LegacyRecipeId};
use crate::vfs::Vfs;
use std::collections::BTreeMap;
use std::fmt;

const MAX_ILLUSTRATION_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IssueKind {
    UnsupportedItemType(u32),
    ItemDefinition,
    Illustration,
}

impl fmt::Display for IssueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedItemType(ty) => write!(f, "item type {ty} has no definition reader"),
            Self::ItemDefinition => f.write_str("item definition unavailable"),
            Self::Illustration => f.write_str("illustration unreadable or invalid size"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub kind: IssueKind,
    pub source: Source,
    pub detail: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Status {
    ReferencesIncomplete,
    /// An empty list means all implemented readiness checks passed.
    Checked(Vec<Issue>),
}

#[derive(Debug)]
pub struct ReadinessAudit {
    pub references: SchematicAudit,
    pub statuses: Vec<(LegacyRecipeId, Status)>,
}

/// Check the existing item definition readers and illustration byte reads.
/// An empty selection checks all discovered groups. Checks run only for
/// reference-complete groups and do not interpret illustration or script data.
pub fn audit_readiness(
    vfs: &mut Vfs,
    selected: &[LegacyRecipeId],
) -> Result<ReadinessAudit, Error> {
    let references = if selected.is_empty() {
        audit_all_schematics(vfs)?
    } else {
        audit_schematics(vfs, selected)?
    };
    let mut checked = BTreeMap::new();
    let statuses = references
        .references
        .iter()
        .map(|(id, result)| {
            let Ok(report) = result else {
                return (*id, Status::ReferencesIncomplete);
            };
            if report.definition.is_none() {
                return (*id, Status::ReferencesIncomplete);
            }
            let mut issues = Vec::new();
            for item in report.items.values() {
                let prototype = item.prototype.as_ref().expect("resolved item");
                let source = &prototype.source;
                if let Some(issue) = checked
                    .entry(source.virtual_path.as_str())
                    .or_insert_with(|| check_item(vfs, item, source))
                {
                    issues.push(issue.clone());
                }
            }
            let source = &report
                .illustration
                .as_ref()
                .expect("resolved artwork")
                .asset_source;
            if let Some(issue) = checked
                .entry(source.virtual_path.as_str())
                .or_insert_with(|| check_illustration(vfs, source))
            {
                issues.push(issue.clone());
            }
            (*id, Status::Checked(issues))
        })
        .collect();
    Ok(ReadinessAudit {
        references,
        statuses,
    })
}

fn check_item(vfs: &mut Vfs, item: &ItemResolution, source: &Source) -> Option<Issue> {
    if item.definition.is_some() {
        return None; // Reference resolution already constructed the Food definition.
    }
    let prototype = item.prototype.as_ref().expect("resolved item");
    let header = prototype.header;
    if !header.is_weapon_or_ammo() && !header.is_armour() && !header.is_generic() {
        return Some(Issue {
            kind: IssueKind::UnsupportedItemType(header.object_type),
            source: source.clone(),
            detail: "only the common description prefix was checked".into(),
        });
    }
    let result = inspect_prototype(vfs, &source.virtual_path)
        .map_err(|error| error.to_string())
        .and_then(|inspection| {
            if inspection.prototype.header != header
                || inspection.prototype.description != prototype.description
            {
                return Err(
                    "prototype identity, type or name key changed after reference resolution"
                        .into(),
                );
            }
            let name = item.name.as_ref().expect("resolved item name").text.clone();
            if let Some(fields) = inspection.weapon_ammo {
                inspection.weapon_ammo_definition(fields, name).map(drop)
            } else if let Some(fields) = inspection.armour {
                inspection.armour_definition(fields, name).map(drop)
            } else if let Some(fields) = inspection.generic {
                inspection.generic_definition(fields, name).map(drop)
            } else {
                return Err("item no longer has a supported body".into());
            }
            .map_err(|error| error.to_string())
        });
    result.err().map(|detail| Issue {
        kind: IssueKind::ItemDefinition,
        source: source.clone(),
        detail,
    })
}

fn check_illustration(vfs: &mut Vfs, source: &Source) -> Option<Issue> {
    let detail = match read_bounded(vfs, &source.virtual_path, MAX_ILLUSTRATION_BYTES) {
        Ok((_, bytes)) if !bytes.is_empty() => return None,
        Ok(_) => "illustration is empty".into(),
        Err(error) => error.to_string(),
    };
    Some(Issue {
        kind: IssueKind::Illustration,
        source: source.clone(),
        detail,
    })
}
