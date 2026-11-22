//! Read-only item inspection and binary-independent item definitions.

use super::{Error, Source, load_mes, read_bounded};
use crate::{prototype, vfs::Vfs};

/// A native key, such as `arcanum:item/prototype-10087`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemId(pub String);

impl ItemId {
    pub(super) fn from_prototype(id: prototype::SerializedPrototypeId) -> Self {
        Self(format!("arcanum:item/prototype-{}", id.0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoodDefinition {
    pub id: ItemId,
    pub display_name: String,
    /// Base weight in the game's stone units.
    pub base_weight: u32,
    /// Base worth in game currency units, before runtime adjustments.
    pub base_worth: u32,
}

/// Damage points by channel. Channels can affect different character pools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageKind {
    Normal,
    Poison,
    Electrical,
    Fire,
    Fatigue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageComponent {
    pub kind: DamageKind,
    pub minimum: u32,
    pub maximum: u32,
}

/// Compatibility category shared by weapons and ammunition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmmoKind {
    Arrows,
    Bullets,
    Charges,
    Fuel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmmoRequirement {
    pub kind: AmmoKind,
    pub units_per_attack: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaponDefinition {
    pub id: ItemId,
    pub display_name: String,
    /// Base weight in game stone units.
    pub base_weight: u32,
    /// Base worth in game currency units.
    pub base_worth: u32,
    pub damage: Vec<DamageComponent>,
    /// Base speed rating used by attack timing and action-point calculations.
    pub speed_factor: i32,
    pub range_tiles: u32,
    pub ammunition: Option<AmmoRequirement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmmoDefinition {
    pub id: ItemId,
    pub display_name: String,
    pub kind: AmmoKind,
    /// Base weight factor in game stone units, before stack scaling.
    pub base_weight: u32,
    /// Base worth in game currency units.
    pub base_worth: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WeaponAmmoDefinition {
    Weapon(WeaponDefinition),
    Ammo(AmmoDefinition),
}

#[derive(Debug)]
pub struct ResolvedWeaponAmmo {
    pub definition: WeaponAmmoDefinition,
    pub name_source: Source,
}

#[derive(Debug)]
pub enum ItemDefinitionError {
    Text(Error),
    MissingName {
        id: prototype::DescriptionId,
        source: Box<Source>,
    },
    InvalidField {
        field: &'static str,
        value: i32,
    },
    InvalidDamage {
        kind: DamageKind,
        minimum: i32,
        maximum: i32,
    },
}

impl std::fmt::Display for ItemDefinitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text(error) => error.fmt(f),
            Self::MissingName { id, source } => {
                write!(
                    f,
                    "missing description key {} in {} from {source}",
                    id.0, source.virtual_path
                )
            }
            Self::InvalidField { field, value } => write!(f, "unsupported {field} value {value}"),
            Self::InvalidDamage {
                kind,
                minimum,
                maximum,
            } => {
                write!(f, "invalid {kind:?} damage bounds {minimum}–{maximum}")
            }
        }
    }
}

impl std::error::Error for ItemDefinitionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Text(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct PrototypeInspection {
    pub prototype: LoadedPrototype,
    pub weapon_ammo: Option<prototype::WeaponAmmoFields>,
}

impl PrototypeInspection {
    /// Resolve the stored description key from the caller's VFS and convert
    /// the decoded fields into a native weapon or ammunition definition.
    pub fn resolve_weapon_ammo(
        &self,
        vfs: &mut Vfs,
    ) -> Result<Option<ResolvedWeaponAmmo>, ItemDefinitionError> {
        let Some(fields) = self.weapon_ammo else {
            return Ok(None);
        };
        let common = fields.item();
        let names = load_mes(vfs, description_path(common.description))
            .map_err(ItemDefinitionError::Text)?;
        let Some(name) = names
            .mes
            .get(common.description.0)
            .filter(|name| !name.trim().is_empty())
        else {
            return Err(ItemDefinitionError::MissingName {
                id: common.description,
                source: Box::new(names.source),
            });
        };
        let id = ItemId::from_prototype(self.prototype.header.id);
        let display_name = name.to_owned();
        let base_weight = unsigned("base weight", common.weight)?;
        let base_worth = unsigned("base worth", common.worth)?;
        let definition = match fields {
            prototype::WeaponAmmoFields::Weapon(weapon) => {
                let kinds = [
                    DamageKind::Normal,
                    DamageKind::Poison,
                    DamageKind::Electrical,
                    DamageKind::Fire,
                    DamageKind::Fatigue,
                ];
                let mut damage = Vec::new();
                for (index, kind) in kinds.into_iter().enumerate() {
                    // CE's sparse array lookup returns zero for absent indices.
                    let minimum = weapon.damage_lower[index].unwrap_or(0);
                    let maximum = weapon.damage_upper[index].unwrap_or(0);
                    if minimum < 0 || maximum < minimum {
                        return Err(ItemDefinitionError::InvalidDamage {
                            kind,
                            minimum,
                            maximum,
                        });
                    }
                    if maximum > 0 {
                        damage.push(DamageComponent {
                            kind,
                            minimum: minimum as u32,
                            maximum: maximum as u32,
                        });
                    }
                }
                let ammunition = if weapon.ammo_type == 10000 {
                    None
                } else {
                    Some(AmmoRequirement {
                        kind: ammo_kind(weapon.ammo_type)?,
                        units_per_attack: unsigned("ammo consumption", weapon.ammo_consumption)?,
                    })
                };
                WeaponAmmoDefinition::Weapon(WeaponDefinition {
                    id,
                    display_name,
                    base_weight,
                    base_worth,
                    damage,
                    ammunition,
                    speed_factor: weapon.speed_factor,
                    range_tiles: unsigned("range", weapon.range)?,
                })
            }
            prototype::WeaponAmmoFields::Ammo(ammo) => WeaponAmmoDefinition::Ammo(AmmoDefinition {
                id,
                display_name,
                base_weight,
                base_worth,
                kind: ammo_kind(ammo.ammo_type)?,
            }),
        };
        Ok(Some(ResolvedWeaponAmmo {
            definition,
            name_source: names.source,
        }))
    }
}

fn unsigned(field: &'static str, value: i32) -> Result<u32, ItemDefinitionError> {
    u32::try_from(value).map_err(|_| ItemDefinitionError::InvalidField { field, value })
}

fn ammo_kind(value: i32) -> Result<AmmoKind, ItemDefinitionError> {
    match value {
        0 => Ok(AmmoKind::Arrows),
        1 => Ok(AmmoKind::Bullets),
        2 => Ok(AmmoKind::Charges),
        3 => Ok(AmmoKind::Fuel),
        _ => Err(ItemDefinitionError::InvalidField {
            field: "ammunition kind",
            value,
        }),
    }
}

pub(super) fn description_path(id: prototype::DescriptionId) -> &'static str {
    if id.0 < 30000 {
        "mes/description.mes"
    } else {
        "mes/gamedesc.mes"
    }
}

#[derive(Debug)]
pub struct LoadedPrototype {
    pub source: Source,
    pub header: prototype::Header,
    pub description: Option<prototype::DescriptionId>,
    pub food: Option<prototype::FoodFields>,
}

/// Load a prototype through the VFS. Weapon, Ammo, Armor, Key and Generic
/// expose the common description prefix. Food exposes its selected fields,
/// and other types expose their header.
pub fn load_prototype(vfs: &mut Vfs, path: &str) -> Result<LoadedPrototype, Error> {
    Ok(load(vfs, path, false)?.prototype)
}

/// Inspect selected subtype fields while keeping the lighter identity lookup
/// available for callers that only need prototype references.
pub fn inspect_prototype(vfs: &mut Vfs, path: &str) -> Result<PrototypeInspection, Error> {
    load(vfs, path, true)
}

fn load(vfs: &mut Vfs, path: &str, detailed: bool) -> Result<PrototypeInspection, Error> {
    let (source, bytes) = read_bounded(vfs, path, prototype::MAX_PROTOTYPE_BYTES)?;
    let result = prototype::decode_header(&bytes).and_then(|header| {
        let food = if header.is_food() {
            Some(prototype::decode_food(&bytes)?.1)
        } else {
            None
        };
        let weapon_ammo = if detailed && header.is_weapon_or_ammo() {
            Some(prototype::decode_weapon_ammo(&bytes)?.1)
        } else {
            None
        };
        let description = if let Some(fields) = food {
            Some(fields.description)
        } else if let Some(fields) = &weapon_ammo {
            Some(fields.item().description)
        } else {
            prototype::decode_item_description(&bytes, header)?
        };
        Ok((header, description, food, weapon_ammo))
    });
    match result {
        Ok((header, description, food, weapon_ammo)) => Ok(PrototypeInspection {
            prototype: LoadedPrototype {
                source,
                header,
                description,
                food,
            },
            weapon_ammo,
        }),
        Err(error) => Err(Error::Prototype {
            provenance: Box::new(source),
            error,
        }),
    }
}
