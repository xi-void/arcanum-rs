//! Inspection of one legacy recipe and resolution into independent content.

use super::items::{FoodDefinition, ItemId, LoadedPrototype, description_path, load_prototype};
use super::{LoadedMes, Source, load_mes};
use crate::mes::{EntryId, Mes};
use crate::prototype::{DescriptionId, SerializedPrototypeId};
use crate::{resource_path, vfs};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::num::NonZeroU32;
use vfs::Vfs;

pub mod audit;

const RULES: &str = "rules/schematic.mes";
const TEXT: &str = "mes/schematic_text.mes";
const INTERFACE: &str = "art/interface/interface.mes";
const MAX_VARIANTS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecipeVariant<I> {
    pub ingredients: [I; 2],
    pub product: I,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyGroup {
    pub id: LegacyRecipeId,
    pub title: SchematicTextId,
    pub description: SchematicTextId,
    pub illustration: InterfaceArtId,
    pub variants: Vec<RecipeVariant<BasicPrototypeId>>,
    pub quantity: NonZeroU32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupError {
    UnsupportedGroup(LegacyRecipeId),
    Missing(EntryId),
    InvalidNumber(EntryId),
    UnsupportedMapping { key: EntryId, reason: &'static str },
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
            Self::UnsupportedMapping { key, reason } => {
                write!(f, "unsupported schematic mapping at entry {key}: {reason}")
            }
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
        let value = |offset| {
            let key = base + offset;
            rules.get(key).ok_or(GroupError::Missing(key))
        };
        let number = |offset| -> Result<i32, GroupError> {
            value(offset)?
                .trim_ascii()
                .parse::<i32>()
                .ok()
                .filter(|number| *number >= 0)
                .ok_or(GroupError::InvalidNumber(base + offset))
        };
        let title = SchematicTextId(number(0)?);
        let description = SchematicTextId(number(1)?);
        let illustration = InterfaceArtId(number(2)?);
        let item_list = |offset| -> Result<Vec<BasicPrototypeId>, GroupError> {
            let mut items = Vec::new();
            for token in value(offset)?.split_ascii_whitespace() {
                if items.len() == MAX_VARIANTS {
                    return Err(GroupError::UnsupportedMapping {
                        key: base + offset,
                        reason: "more than three items",
                    });
                }
                let id = token
                    .parse::<i32>()
                    .ok()
                    .filter(|number| *number > 0)
                    .ok_or(GroupError::InvalidNumber(base + offset))?;
                items.push(BasicPrototypeId(id as u32));
            }
            if items.is_empty() {
                return Err(GroupError::InvalidNumber(base + offset));
            }
            Ok(items)
        };
        let first = item_list(3)?;
        let second = item_list(4)?;
        let products = item_list(5)?;
        let unsupported = |offset, reason| GroupError::UnsupportedMapping {
            key: base + offset,
            reason,
        };
        if first.iter().any(|id| *id != first[0]) {
            return Err(unsupported(3, "first ingredient must be fixed"));
        }
        if first.len() != 1 && first.len() != second.len() {
            return Err(unsupported(
                3,
                "fixed ingredient repetitions must align with variants",
            ));
        }
        if second.iter().collect::<BTreeSet<_>>().len() != second.len() {
            return Err(unsupported(4, "duplicate second-ingredient alternative"));
        }
        if second.len() != products.len() {
            return Err(unsupported(
                5,
                "each second-ingredient alternative needs one output",
            ));
        }
        let variants = second
            .into_iter()
            .zip(products)
            .map(|(second, product)| RecipeVariant {
                ingredients: [first[0], second],
                product,
            })
            .collect();
        Ok(Self {
            id,
            title,
            description,
            illustration,
            variants,
            quantity: NonZeroU32::new(number(6)? as u32)
                .ok_or(GroupError::InvalidNumber(base + 6))?,
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
    /// In legacy recipes, order preserves the original preference.
    pub variants: Vec<RecipeVariant<ItemId>>,
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
    /// Native identity backed by a validated prototype and description reference.
    pub id: Option<ItemId>,
    pub definition: Option<FoodDefinition>,
}

/// Categories of independent issues found while validating a schematic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagnosticKind {
    MissingPrototype,
    AmbiguousPrototype,
    PrototypeIdentityMismatch,
    InvalidPrototype,
    MissingDescription,
    MissingIllustration,
    MalformedMapping,
    DuplicateKey,
    Other,
}

impl fmt::Display for DiagnosticKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingPrototype => "Missing prototype resource",
            Self::AmbiguousPrototype => "Ambiguous prototype candidate",
            Self::PrototypeIdentityMismatch => "Prototype identity mismatch",
            Self::InvalidPrototype => "Unsupported or invalid prototype",
            Self::MissingDescription => "Missing name or description reference",
            Self::MissingIllustration => "Missing illustration reference",
            Self::MalformedMapping => "Malformed or unsupported schematic mapping",
            Self::DuplicateKey => "Duplicate MES key",
            Self::Other => "Other validation error",
        })
    }
}

#[derive(Debug)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
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
    pub items: BTreeMap<BasicPrototypeId, ItemResolution>,
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
    let candidates = prototype_candidates(vfs, item_references(&legacy).map(|(_, id)| id));
    Ok(resolve_schematic(
        vfs,
        &rules,
        legacy,
        &candidates,
        &mut MesTables::new(),
    ))
}

fn item_references(legacy: &LegacyGroup) -> impl Iterator<Item = (&'static str, BasicPrototypeId)> {
    legacy.variants.iter().flat_map(|variant| {
        [
            ("ingredient 1", variant.ingredients[0]),
            ("ingredient 2", variant.ingredients[1]),
            ("product", variant.product),
        ]
    })
}

type MesTables = BTreeMap<&'static str, Result<LoadedMes, super::Error>>;

fn mes_table<'a>(
    vfs: &mut Vfs,
    tables: &'a mut MesTables,
    path: &'static str,
) -> &'a Result<LoadedMes, super::Error> {
    tables.entry(path).or_insert_with(|| load_mes(vfs, path))
}

fn resolve_schematic(
    vfs: &mut Vfs,
    rules: &LoadedMes,
    legacy: LegacyGroup,
    candidates: &BTreeMap<SerializedPrototypeId, Vec<String>>,
    tables: &mut MesTables,
) -> SchematicReport {
    let id = legacy.id;
    let mut diagnostics = Vec::new();
    for key in id.0..=id.0 + 6 {
        if rules.duplicates.iter().any(|duplicate| duplicate.id == key) {
            warn(
                &mut diagnostics,
                DiagnosticKind::DuplicateKey,
                "rules",
                format!("duplicate entry {key}; using the final value"),
            );
        }
    }
    let text = mes_table(vfs, tables, TEXT);
    let title = resolve_text(
        text,
        TEXT,
        legacy.title.0,
        "title",
        DiagnosticKind::MissingDescription,
        &mut diagnostics,
    );
    let description = resolve_text(
        text,
        TEXT,
        legacy.description.0,
        "description",
        DiagnosticKind::MissingDescription,
        &mut diagnostics,
    );
    let illustration = resolve_illustration(vfs, tables, legacy.illustration, &mut diagnostics);
    let mut references = BTreeMap::new();
    for (field, id) in item_references(&legacy) {
        references.entry(id).or_insert(field);
    }
    let items = references
        .into_iter()
        .map(|(id, field)| {
            let item = resolve_item(vfs, candidates, tables, field, id, &mut diagnostics);
            (id, item)
        })
        .collect();
    let mut report = SchematicReport {
        rules_source: rules.source.clone(),
        legacy,
        title,
        description,
        illustration,
        items,
        definition: None,
        diagnostics,
    };
    report.definition = native_definition(&report);
    report
}

fn native_definition(report: &SchematicReport) -> Option<SchematicDefinition> {
    let item = |id| report.items.get(&id)?.id.clone();
    let variants = report
        .legacy
        .variants
        .iter()
        .map(|variant| {
            Some(RecipeVariant {
                ingredients: [item(variant.ingredients[0])?, item(variant.ingredients[1])?],
                product: item(variant.product)?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
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
        variants,
        quantity: report.legacy.quantity,
    })
}

fn resolve_text(
    table: &Result<LoadedMes, super::Error>,
    path: &str,
    key: EntryId,
    field: &'static str,
    missing_kind: DiagnosticKind,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ResolvedText> {
    match table {
        Ok(table) => match table.mes.get(key) {
            Some(text) => {
                if table.duplicates.iter().any(|duplicate| duplicate.id == key) {
                    warn(
                        diagnostics,
                        DiagnosticKind::DuplicateKey,
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
                    missing_kind,
                    field,
                    format!("missing key {key} in {path} from {}", table.source),
                );
                None
            }
        },
        Err(error) => {
            warn(
                diagnostics,
                if resource_is_missing(error) {
                    missing_kind
                } else {
                    DiagnosticKind::Other
                },
                field,
                format!("cannot read key {key} from {path}: {error}"),
            );
            None
        }
    }
}

fn resolve_illustration(
    vfs: &mut Vfs,
    tables: &mut MesTables,
    art: InterfaceArtId,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Illustration> {
    let entry = resolve_text(
        mes_table(vfs, tables, INTERFACE),
        INTERFACE,
        art.0,
        "illustration",
        DiagnosticKind::MissingIllustration,
        diagnostics,
    )?;
    let relative = match resource_path::normalize(&entry.text) {
        Ok(relative) => relative,
        Err(error) => {
            warn(
                diagnostics,
                DiagnosticKind::Other,
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
                if resource_is_missing(&error) {
                    DiagnosticKind::MissingIllustration
                } else {
                    DiagnosticKind::Other
                },
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
    ids: impl IntoIterator<Item = BasicPrototypeId>,
) -> BTreeMap<SerializedPrototypeId, Vec<String>> {
    let wanted: BTreeSet<_> = ids
        .into_iter()
        .filter_map(BasicPrototypeId::serialized)
        .collect();
    let mut found = BTreeMap::<_, Vec<_>>::new();
    if wanted.is_empty() {
        return found;
    }
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
    tables: &mut MesTables,
    field: &'static str,
    id: BasicPrototypeId,
    diagnostics: &mut Vec<Diagnostic>,
) -> ItemResolution {
    let empty = ItemResolution::default;
    let Some(serialized) = id.serialized() else {
        warn(
            diagnostics,
            DiagnosticKind::InvalidPrototype,
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
                DiagnosticKind::MissingPrototype,
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
                DiagnosticKind::AmbiguousPrototype,
                field,
                format!(
                    "basic prototype {} has ambiguous serialized prototype {}: {}",
                    id.0,
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
                DiagnosticKind::PrototypeIdentityMismatch,
                field,
                format!(
                    "{path} from {} declares ID {}; expected {}",
                    prototype.source, prototype.header.id.0, serialized.0
                ),
            );
            return empty();
        }
        Err(error) => {
            let kind = match &error {
                super::Error::Prototype { .. } | super::Error::TooLarge { .. } => {
                    DiagnosticKind::InvalidPrototype
                }
                error if resource_is_missing(error) => DiagnosticKind::MissingPrototype,
                _ => DiagnosticKind::Other,
            };
            warn(diagnostics, kind, field, error.to_string());
            return empty();
        }
    };
    let Some(DescriptionId(key)) = prototype.description else {
        warn(
            diagnostics,
            DiagnosticKind::InvalidPrototype,
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

    let text_path = description_path(DescriptionId(key));
    let name = resolve_text(
        mes_table(vfs, tables, text_path),
        text_path,
        key,
        field,
        DiagnosticKind::MissingDescription,
        diagnostics,
    );
    let mut native_id = name.as_ref().map(|_| ItemId::from_prototype(serialized));
    let definition = prototype.food.and_then(|food| {
        match (u32::try_from(food.weight), u32::try_from(food.worth)) {
            (Ok(base_weight), Ok(base_worth)) => {
                name.as_ref()
                    .zip(native_id.as_ref())
                    .map(|(name, id)| FoodDefinition {
                        id: id.clone(),
                        display_name: name.text.clone(),
                        base_weight,
                        base_worth,
                    })
            }
            _ => {
                warn(
                    diagnostics,
                    DiagnosticKind::InvalidPrototype,
                    field,
                    format!("{path} has a negative base weight or worth"),
                );
                native_id = None;
                None
            }
        }
    });
    ItemResolution {
        prototype: Some(prototype),
        name,
        id: native_id,
        definition,
    }
}

fn resource_is_missing(error: &super::Error) -> bool {
    match error {
        super::Error::Resource {
            error: vfs::Error::NotFound(_),
            ..
        } => true,
        super::Error::Resource {
            error: vfs::Error::Io { source, .. },
            ..
        } => source.kind() == std::io::ErrorKind::NotFound,
        _ => false,
    }
}

fn warn(
    diagnostics: &mut Vec<Diagnostic>,
    kind: DiagnosticKind,
    field: &'static str,
    message: String,
) {
    diagnostics.push(Diagnostic {
        kind,
        field,
        message,
    });
}

#[cfg(test)]
mod tests;
