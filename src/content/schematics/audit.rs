//! Structural coverage of learned and found schematic records.

use super::{
    GroupError, LegacyGroup, LegacyRecipeId, MAX_VARIANTS, RULES, SchematicReport, item_references,
    prototype_candidates, resolve_schematic,
};
use crate::content::{Error, Source, load_mes};
use crate::mes::{EntryId, Mes};
use crate::vfs::Vfs;
use std::collections::BTreeSet;
use std::num::NonZeroU32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupShape {
    /// The length of each of the two required input lists as stored in MES.
    pub ingredient_ids: [usize; 2],
    pub repeated_ingredients: [bool; 2],
    pub product_ids: usize,
    pub output_quantity: NonZeroU32,
    /// Each second-slot position has a product and the first slot is either
    /// a singleton or has the same width.
    pub lists_align: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum GroupStatus {
    Supported(LegacyGroup),
    Unsupported(GroupError),
    MalformedOrIncomplete(GroupError),
}

#[derive(Debug, PartialEq, Eq)]
pub struct GroupAudit {
    pub id: LegacyRecipeId,
    /// Present for complete, well-formed groups, including unsupported mappings.
    pub shape: Option<GroupShape>,
    pub status: GroupStatus,
}

#[derive(Debug)]
pub struct SchematicAudit {
    pub rules_source: Source,
    pub groups: Vec<GroupAudit>,
    pub duplicate_keys: Vec<EntryId>,
    /// Explicitly selected groups in ascending ID order. A report can still
    /// contain missing or invalid references and lack a native definition.
    pub references: Vec<(LegacyRecipeId, Result<SchematicReport, GroupError>)>,
}

/// Discover groups with any of their seven expected cells in the learned
/// and found ranges. Ignore reserved cells and out-of-range records.
pub fn audit_groups(rules: &Mes) -> Vec<GroupAudit> {
    (2000..6000)
        .step_by(10)
        .filter(|base| (0..=6).any(|offset| rules.get(base + offset).is_some()))
        .map(|base| audit_group(LegacyRecipeId(base), rules))
        .collect()
}

fn audit_group(id: LegacyRecipeId, rules: &Mes) -> GroupAudit {
    match read_shape(id, rules) {
        Ok(shape) => GroupAudit {
            id,
            shape: Some(shape),
            status: match LegacyGroup::decode(id, rules) {
                Ok(group) => GroupStatus::Supported(group),
                Err(error) => GroupStatus::Unsupported(error),
            },
        },
        Err(error) => GroupAudit {
            id,
            shape: None,
            status: GroupStatus::MalformedOrIncomplete(error),
        },
    }
}

fn read_shape(id: LegacyRecipeId, rules: &Mes) -> Result<GroupShape, GroupError> {
    let mut cells = [""; 7];
    for (offset, cell) in (0..).zip(&mut cells) {
        let key = id.0 + offset;
        *cell = rules.get(key).ok_or(GroupError::Missing(key))?;
    }
    for (offset, cell) in (0..).zip(&cells[..3]) {
        if !cell
            .trim_ascii()
            .parse::<i32>()
            .is_ok_and(|value| value >= 0)
        {
            return Err(GroupError::InvalidNumber(id.0 + offset));
        }
    }
    let list = |offset: i32| {
        let key = id.0 + offset;
        let mut count = 0;
        let mut unique = BTreeSet::new();
        for token in cells[offset as usize].split_ascii_whitespace() {
            let value = token
                .parse::<i32>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or(GroupError::InvalidNumber(key))?;
            count += 1;
            if count > MAX_VARIANTS {
                return Err(GroupError::UnsupportedMapping {
                    key,
                    reason: "more than three items",
                });
            }
            unique.insert(value);
        }
        if count == 0 {
            return Err(GroupError::InvalidNumber(key));
        }
        Ok((count, unique.len() != count))
    };
    let (first, first_repeats) = list(3)?;
    let (second, second_repeats) = list(4)?;
    let (products, _) = list(5)?;
    let output_quantity = cells[6]
        .trim_ascii()
        .parse::<i32>()
        .ok()
        .and_then(|value| u32::try_from(value).ok())
        .and_then(NonZeroU32::new)
        .ok_or(GroupError::InvalidNumber(id.0 + 6))?;
    Ok(GroupShape {
        ingredient_ids: [first, second],
        repeated_ingredients: [first_repeats, second_repeats],
        product_ids: products,
        output_quantity,
        lists_align: second == products && (first == 1 || first == second),
    })
}

/// Read the VFS rules once. Reference validation is opt-in and shares a single
/// prototype path search across the selected groups.
pub fn audit_schematics(
    vfs: &mut Vfs,
    selected: &[LegacyRecipeId],
) -> Result<SchematicAudit, Error> {
    let rules = load_mes(vfs, RULES)?;
    let groups = audit_groups(&rules.mes);
    let selected: BTreeSet<_> = selected.iter().map(|id| id.0).collect();
    let selected = selected
        .into_iter()
        .map(|base| {
            let id = LegacyRecipeId(base);
            let group = match groups.binary_search_by_key(&base, |group| group.id.0) {
                Ok(index) => match &groups[index].status {
                    GroupStatus::Supported(group) => Ok(group),
                    GroupStatus::Unsupported(error) | GroupStatus::MalformedOrIncomplete(error) => {
                        Err(*error)
                    }
                },
                Err(_) if (2000..6000).contains(&base) && base % 10 == 0 => {
                    Err(GroupError::Missing(base))
                }
                Err(_) => Err(GroupError::UnsupportedGroup(id)),
            };
            (id, group)
        })
        .collect::<Vec<_>>();
    let candidates = prototype_candidates(
        vfs,
        selected
            .iter()
            .filter_map(|(_, group)| group.as_ref().ok())
            .flat_map(|group| item_references(group).map(|(_, id)| id)),
    );
    let references = selected
        .into_iter()
        .map(|(id, group)| {
            (
                id,
                group.map(|group| resolve_schematic(vfs, &rules, group.clone(), &candidates)),
            )
        })
        .collect();
    let duplicate_keys = rules
        .duplicates
        .iter()
        .map(|entry| entry.id)
        .filter(|key| (2000..6000).contains(key) && key % 10 < 7)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(SchematicAudit {
        rules_source: rules.source,
        groups,
        duplicate_keys,
        references,
    })
}
