//! Inspection of one legacy recipe and resolution into independent content.

use super::items::{FoodDefinition, ItemId, LoadedPrototype, load_prototype};
use super::{LoadedMes, Source, load_mes};
use crate::mes::{EntryId, Mes};
use crate::prototype::{DescriptionId, SerializedPrototypeId};
use crate::{resource_path, vfs::Vfs};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::num::NonZeroU32;

const RULES: &str = "rules/schematic.mes";
const TEXT: &str = "mes/schematic_text.mes";
const INTERFACE: &str = "art/interface/interface.mes";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyRecipeId(pub i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BasicPrototypeId(pub u32);

impl BasicPrototypeId {
    /// Map a positive basic lookup ID to the serialized numeric ID.
    pub fn serialized(self) -> Option<SerializedPrototypeId> {
        let value = self.0.checked_add(20)?;
        (self.0 != 0 && value <= i32::MAX as u32).then_some(SerializedPrototypeId(value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchematicTextId(pub EntryId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterfaceArtId(pub EntryId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyGroup {
    pub id: LegacyRecipeId,
    pub title: SchematicTextId,
    pub description: SchematicTextId,
    pub illustration: InterfaceArtId,
    pub ingredients: [BasicPrototypeId; 2],
    pub product: BasicPrototypeId,
    pub quantity: NonZeroU32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupError {
    UnsupportedGroup(LegacyRecipeId),
    Missing(EntryId),
    InvalidNumber(EntryId),
    Alternatives(EntryId),
}

impl fmt::Display for GroupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedGroup(id) => write!(
                f,
                "recipe {} must start at a multiple of ten from 2000 through 5990",
                id.0
            ),
            Self::Missing(key) => write!(f, "missing schematic entry {key}"),
            Self::InvalidNumber(key) => write!(f, "invalid numeric value at schematic entry {key}"),
            Self::Alternatives(key) => write!(
                f,
                "schematic entry {key} has alternatives; only one item per slot is supported"
            ),
        }
    }
}

impl std::error::Error for GroupError {}

impl LegacyGroup {
    /// Decode the seven entries of one learned or found recipe without I/O.
    pub fn decode(id: LegacyRecipeId, rules: &Mes) -> Result<Self, GroupError> {
        let base = id.0;
        if !(2000..6000).contains(&base) || base % 10 != 0 {
            return Err(GroupError::UnsupportedGroup(id));
        }
        let number = |offset, item: bool| -> Result<i32, GroupError> {
            let key = base + offset;
            let value = rules.get(key).ok_or(GroupError::Missing(key))?.trim_ascii();
            if item && value.split_ascii_whitespace().count() > 1 {
                let valid = value
                    .split_ascii_whitespace()
                    .all(|token| token.parse::<i32>().is_ok_and(|number| number > 0));
                return Err(if valid {
                    GroupError::Alternatives(key)
                } else {
                    GroupError::InvalidNumber(key)
                });
            }
            value
                .parse::<i32>()
                .ok()
                .filter(|value| {
                    if item || offset == 6 {
                        *value > 0
                    } else {
                        *value >= 0
                    }
                })
                .ok_or(GroupError::InvalidNumber(key))
        };
        let item = |offset| number(offset, true).map(|value| BasicPrototypeId(value as u32));
        Ok(Self {
            id,
            title: SchematicTextId(number(0, false)?),
            description: SchematicTextId(number(1, false)?),
            illustration: InterfaceArtId(number(2, false)?),
            ingredients: [item(3)?, item(4)?],
            product: item(5)?,
            quantity: NonZeroU32::new(number(6, false)? as u32)
                .expect("positive quantity validated"),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeId(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchematicDefinition {
    pub id: RecipeId,
    pub title: String,
    pub description: String,
    pub illustration: String,
    pub ingredients: [ItemId; 2],
    pub product: ItemId,
    pub quantity: NonZeroU32,
}

#[derive(Debug)]
pub struct ResolvedText {
    pub text: String,
    pub source: Source,
}

#[derive(Debug)]
pub struct Illustration {
    pub table_source: Source,
    pub asset_source: Source,
}

#[derive(Debug, Default)]
pub struct ItemResolution {
    /// Set only after the file's serialized ID matches the expected ID.
    pub prototype: Option<LoadedPrototype>,
    pub name: Option<ResolvedText>,
    pub definition: Option<FoodDefinition>,
}

#[derive(Debug)]
pub struct Diagnostic {
    pub field: &'static str,
    pub message: String,
}

#[derive(Debug)]
pub struct SchematicReport {
    pub rules_source: Source,
    pub legacy: LegacyGroup,
    pub title: Option<ResolvedText>,
    pub description: Option<ResolvedText>,
    pub illustration: Option<Illustration>,
    pub ingredients: [ItemResolution; 2],
    pub product: ItemResolution,
    pub definition: Option<SchematicDefinition>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
pub enum Error {
    Rules(super::Error),
    Group {
        source: Box<Source>,
        error: GroupError,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rules(error) => error.fmt(f),
            Self::Group { source, error } => {
                write!(f, "{} from {source}: {error}", source.virtual_path)
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Rules(error) => Some(error),
            Self::Group { error, .. } => Some(error),
        }
    }
}

/// Resolve a selected group. Missing references leave diagnostic entries and
/// their original IDs intact. A native definition requires every reference.
pub fn load_schematic(vfs: &mut Vfs, id: LegacyRecipeId) -> Result<SchematicReport, Error> {
    let rules = load_mes(vfs, RULES).map_err(Error::Rules)?;
    let legacy = LegacyGroup::decode(id, &rules.mes).map_err(|error| Error::Group {
        source: Box::new(rules.source.clone()),
        error,
    })?;
    let mut diagnostics = Vec::new();
    for key in id.0..=id.0 + 6 {
        if rules.duplicates.iter().any(|duplicate| duplicate.id == key) {
            warn(
                &mut diagnostics,
                "rules",
                format!("duplicate entry {key}; using the final value"),
            );
        }
    }
    let text = load_mes(vfs, TEXT);
    let title = resolve_text(&text, TEXT, legacy.title.0, "title", &mut diagnostics);
    let description = resolve_text(
        &text,
        TEXT,
        legacy.description.0,
        "description",
        &mut diagnostics,
    );
    let illustration = resolve_illustration(vfs, legacy.illustration, &mut diagnostics);
    let candidates = prototype_candidates(
        vfs,
        [legacy.ingredients[0], legacy.ingredients[1], legacy.product],
    );
    let mut description_tables = BTreeMap::new();
    let [first, second, product] = [
        ("ingredient 1", legacy.ingredients[0]),
        ("ingredient 2", legacy.ingredients[1]),
        ("product", legacy.product),
    ]
    .map(|(field, id)| {
        resolve_item(
            vfs,
            &candidates,
            &mut description_tables,
            field,
            id,
            &mut diagnostics,
        )
    });
    let mut report = SchematicReport {
        rules_source: rules.source,
        legacy,
        title,
        description,
        illustration,
        ingredients: [first, second],
        product,
        definition: None,
        diagnostics,
    };
    report.definition = native_definition(&report);
    Ok(report)
}

fn native_definition(report: &SchematicReport) -> Option<SchematicDefinition> {
    Some(SchematicDefinition {
        id: RecipeId(format!("arcanum:recipe/{}", report.legacy.id.0)),
        title: report.title.as_ref()?.text.clone(),
        description: report.description.as_ref()?.text.clone(),
        illustration: report
            .illustration
            .as_ref()?
            .asset_source
            .virtual_path
            .clone(),
        ingredients: [
            report.ingredients[0].definition.as_ref()?.id.clone(),
            report.ingredients[1].definition.as_ref()?.id.clone(),
        ],
        product: report.product.definition.as_ref()?.id.clone(),
        quantity: report.legacy.quantity,
    })
}

fn resolve_text(
    table: &Result<LoadedMes, super::Error>,
    path: &str,
    key: EntryId,
    field: &'static str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ResolvedText> {
    match table {
        Ok(table) => match table.mes.get(key) {
            Some(text) => {
                if table.duplicates.iter().any(|duplicate| duplicate.id == key) {
                    warn(
                        diagnostics,
                        field,
                        format!("duplicate text key {key} in {path}; using the final value"),
                    );
                }
                Some(ResolvedText {
                    text: text.into(),
                    source: table.source.clone(),
                })
            }
            None => {
                warn(
                    diagnostics,
                    field,
                    format!("missing key {key} in {path} from {}", table.source),
                );
                None
            }
        },
        Err(error) => {
            warn(
                diagnostics,
                field,
                format!("cannot read key {key} from {path}: {error}"),
            );
            None
        }
    }
}

fn resolve_illustration(
    vfs: &mut Vfs,
    art: InterfaceArtId,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Illustration> {
    let entry = resolve_text(
        &load_mes(vfs, INTERFACE),
        INTERFACE,
        art.0,
        "illustration",
        diagnostics,
    )?;
    let relative = match resource_path::normalize(&entry.text) {
        Ok(relative) => relative,
        Err(error) => {
            warn(
                diagnostics,
                "illustration",
                format!("invalid resource path for artwork key {}: {error}", art.0),
            );
            return None;
        }
    };
    let path = format!("art/interface/{relative}");
    match Source::resolve(vfs, &path) {
        Ok((asset_source, _)) => Some(Illustration {
            table_source: entry.source,
            asset_source,
        }),
        Err(error) => {
            warn(
                diagnostics,
                "illustration",
                format!("artwork key {}: {error}", art.0),
            );
            None
        }
    }
}

// The filename prefix is a discovery hint; file identity comes from the header.
fn prototype_candidates(
    vfs: &Vfs,
    ids: [BasicPrototypeId; 3],
) -> BTreeMap<SerializedPrototypeId, Vec<String>> {
    let wanted: BTreeSet<_> = ids
        .into_iter()
        .filter_map(BasicPrototypeId::serialized)
        .collect();
    let mut found = BTreeMap::<_, Vec<_>>::new();
    for path in vfs.paths_with_prefix("proto/") {
        let name = path.strip_prefix("proto/").expect("matching prefix");
        if name.contains('/') || !name.ends_with(".pro") {
            continue;
        }
        let end = name
            .find(|ch: char| !ch.is_ascii_digit())
            .unwrap_or(name.len());
        let Ok(id) = name[..end].parse() else {
            continue;
        };
        let id = SerializedPrototypeId(id);
        if wanted.contains(&id) && Source::resolve(vfs, path).is_ok() {
            found.entry(id).or_default().push(path.into());
        }
    }
    found
}

fn resolve_item(
    vfs: &mut Vfs,
    candidates: &BTreeMap<SerializedPrototypeId, Vec<String>>,
    tables: &mut BTreeMap<&'static str, Result<LoadedMes, super::Error>>,
    field: &'static str,
    id: BasicPrototypeId,
    diagnostics: &mut Vec<Diagnostic>,
) -> ItemResolution {
    let empty = ItemResolution::default;
    let Some(serialized) = id.serialized() else {
        warn(
            diagnostics,
            field,
            format!("basic prototype {} has no supported serialized ID", id.0),
        );
        return empty();
    };
    let path = match candidates
        .get(&serialized)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        [path] => path,
        [] => {
            warn(
                diagnostics,
                field,
                format!(
                    "basic prototype {} (serialized {}) has no visible prototype path",
                    id.0, serialized.0
                ),
            );
            return empty();
        }
        paths => {
            warn(
                diagnostics,
                field,
                format!(
                    "ambiguous serialized prototype {}: {}",
                    serialized.0,
                    paths.join(", ")
                ),
            );
            return empty();
        }
    };
    let prototype = match load_prototype(vfs, path) {
        Ok(prototype) if prototype.header.id == serialized => prototype,
        Ok(prototype) => {
            warn(
                diagnostics,
                field,
                format!(
                    "{path} from {} declares ID {}; expected {}",
                    prototype.source, prototype.header.id.0, serialized.0
                ),
            );
            return empty();
        }
        Err(error) => {
            warn(diagnostics, field, error.to_string());
            return empty();
        }
    };
    let Some(food) = prototype.food else {
        warn(
            diagnostics,
            field,
            format!(
                "{path} from {} has unsupported object type {}",
                prototype.source, prototype.header.object_type
            ),
        );
        return ItemResolution {
            prototype: Some(prototype),
            ..empty()
        };
    };

    let DescriptionId(key) = food.description;
    let text_path = if key < 30000 {
        "mes/description.mes"
    } else {
        "mes/gamedesc.mes"
    };
    let table = tables
        .entry(text_path)
        .or_insert_with(|| load_mes(vfs, text_path));
    let name = resolve_text(table, text_path, key, field, diagnostics);
    let definition = match (u32::try_from(food.weight), u32::try_from(food.worth)) {
        (Ok(base_weight), Ok(base_worth)) => name.as_ref().map(|name| FoodDefinition {
            id: ItemId(format!("arcanum:item/prototype-{}", serialized.0)),
            display_name: name.text.clone(),
            base_weight,
            base_worth,
        }),
        _ => {
            warn(
                diagnostics,
                field,
                format!("{path} has a negative base weight or worth"),
            );
            None
        }
    };
    ItemResolution {
        prototype: Some(prototype),
        name,
        definition,
    }
}

fn warn(diagnostics: &mut Vec<Diagnostic>, field: &'static str, message: String) {
    diagnostics.push(Diagnostic { field, message });
}

#[cfg(test)]
mod tests;
