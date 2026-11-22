//! Standalone equipment associations, additive AC and bounded resistance effects.

use crate::content::effects::{EffectDefinition, EffectOperation};
use crate::content::items::{
    AmmoDefinition, ArmourDefinition, FoodDefinition, GenericDefinition, ItemId, KeyDefinition,
    ResistanceKind, WeaponDefinition,
};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActorId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ItemInstanceId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActivationId(u64);

/// An immutable native definition borrowed from the caller.
#[derive(Debug, Clone, Copy)]
pub enum ItemDefinitionRef<'content> {
    Ammo(&'content AmmoDefinition),
    Armour(&'content ArmourDefinition),
    Food(&'content FoodDefinition),
    Generic(&'content GenericDefinition),
    Key(&'content KeyDefinition),
    Weapon(&'content WeaponDefinition),
}

impl<'content> ItemDefinitionRef<'content> {
    pub fn id(self) -> &'content ItemId {
        match self {
            Self::Ammo(item) => &item.id,
            Self::Armour(item) => &item.id,
            Self::Food(item) => &item.id,
            Self::Generic(item) => &item.id,
            Self::Key(item) => &item.id,
            Self::Weapon(item) => &item.id,
        }
    }
}

#[derive(Debug)]
pub struct ItemInstance<'content> {
    pub definition: ItemDefinitionRef<'content>,
    pub equipped_by: Option<ActorId>,
}

#[derive(Debug)]
pub struct EffectActivation<'content> {
    pub id: ActivationId,
    pub source: ItemInstanceId,
    pub target: ActorId,
    pub definition: &'content EffectDefinition,
    /// The signed contribution captured when this activation was granted.
    pub armour_class_points: i32,
    /// The strongest Fire minimum granted by this activation, if any.
    pub fire_resistance_floor: Option<i32>,
    /// The signed Electrical adjustment in percentage points granted at activation.
    pub electrical_resistance_points: i32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    UnknownActor(ActorId),
    UnknownItem(ItemInstanceId),
    UnknownActivation(ActivationId),
    AlreadyEquipped {
        item: ItemInstanceId,
        wearer: ActorId,
    },
    NotEquippedBy {
        item: ItemInstanceId,
        actor: ActorId,
    },
    UnsupportedOperation(EffectOperation),
    ArmourClassOverflow,
    ElectricalResistanceOverflow,
    IdentityExhausted,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownActor(id) => write!(f, "unknown actor {id:?}"),
            Self::UnknownItem(id) => write!(f, "unknown item instance {id:?}"),
            Self::UnknownActivation(id) => write!(f, "unknown or removed activation {id:?}"),
            Self::AlreadyEquipped { item, wearer } => {
                write!(f, "{item:?} is already equipped by {wearer:?}")
            }
            Self::NotEquippedBy { item, actor } => {
                write!(f, "{item:?} is not equipped by {actor:?}")
            }
            Self::UnsupportedOperation(operation) => {
                write!(f, "unsupported runtime effect operation {operation:?}")
            }
            Self::ArmourClassOverflow => f.write_str("armour class exceeds signed 32-bit range"),
            Self::ElectricalResistanceOverflow => {
                f.write_str("Electrical resistance exceeds signed 32-bit range")
            }
            Self::IdentityExhausted => f.write_str("runtime identity space exhausted"),
        }
    }
}

impl std::error::Error for Error {}

struct ActorState {
    base_ac: i32,
    underlying_fire_resistance: i32,
    underlying_electrical_resistance: i32,
}

/// Owns mutable state. IDs are local to the issuing runtime and never reused
/// within it. Definitions stay borrowed and immutable.
#[derive(Default)]
pub struct Runtime<'content> {
    last_id: u64,
    actors: BTreeMap<ActorId, ActorState>,
    items: BTreeMap<ItemInstanceId, ItemInstance<'content>>,
    activations: BTreeMap<ActivationId, EffectActivation<'content>>,
}

impl<'content> Runtime<'content> {
    pub fn create_actor(&mut self, base_ac: i32) -> Result<ActorId, Error> {
        let id = ActorId(self.allocate_id()?);
        self.actors.insert(
            id,
            ActorState {
                base_ac,
                underlying_fire_resistance: 0,
                underlying_electrical_resistance: 0,
            },
        );
        Ok(id)
    }

    pub fn instantiate_item(
        &mut self,
        definition: ItemDefinitionRef<'content>,
    ) -> Result<ItemInstanceId, Error> {
        let id = ItemInstanceId(self.allocate_id()?);
        self.items.insert(
            id,
            ItemInstance {
                definition,
                equipped_by: None,
            },
        );
        Ok(id)
    }

    pub fn base_armour_class(&self, actor: ActorId) -> Result<i32, Error> {
        Ok(self.actor(actor)?.base_ac)
    }

    pub fn armour_class(&self, actor: ActorId) -> Result<i32, Error> {
        let base = self.base_armour_class(actor)?;
        checked_ac(self.total_ac(actor, base, |_| true))
    }

    pub fn set_base_armour_class(&mut self, actor: ActorId, base_ac: i32) -> Result<(), Error> {
        self.base_armour_class(actor)?;
        checked_ac(self.total_ac(actor, base_ac, |_| true))?;
        self.actors
            .get_mut(&actor)
            .expect("validated actor")
            .base_ac = base_ac;
        Ok(())
    }

    /// Stored Fire resistance in signed percentage points, before active floors.
    pub fn underlying_fire_resistance(&self, actor: ActorId) -> Result<i32, Error> {
        Ok(self.actor(actor)?.underlying_fire_resistance)
    }

    pub fn set_underlying_fire_resistance(
        &mut self,
        actor: ActorId,
        value: i32,
    ) -> Result<(), Error> {
        let state = self
            .actors
            .get_mut(&actor)
            .ok_or(Error::UnknownActor(actor))?;
        state.underlying_fire_resistance = value;
        Ok(())
    }

    /// Evaluate the active lower bounds, before any final query clamp.
    pub fn fire_resistance_after_effects(&self, actor: ActorId) -> Result<i32, Error> {
        let underlying = self.underlying_fire_resistance(actor)?;
        Ok(self
            .activations
            .values()
            .filter(|grant| grant.target == actor)
            .filter_map(|grant| grant.fire_resistance_floor)
            .fold(underlying, i32::max))
    }

    /// Ordinary Fire resistance query, clamped after evaluating active floors.
    pub fn fire_resistance(&self, actor: ActorId) -> Result<i32, Error> {
        Ok(self.fire_resistance_after_effects(actor)?.clamp(0, 95))
    }

    /// Stored Electrical resistance in signed percentage points, before adjustments.
    pub fn underlying_electrical_resistance(&self, actor: ActorId) -> Result<i32, Error> {
        Ok(self.actor(actor)?.underlying_electrical_resistance)
    }

    pub fn set_underlying_electrical_resistance(
        &mut self,
        actor: ActorId,
        value: i32,
    ) -> Result<(), Error> {
        self.actor(actor)?;
        checked_electrical(self.total_electrical(actor, value, |_| true))?;
        self.actors
            .get_mut(&actor)
            .expect("validated actor")
            .underlying_electrical_resistance = value;
        Ok(())
    }

    /// Apply the active additions before the bounds stage. Electrical currently has no bounds.
    pub fn electrical_resistance_after_effects(&self, actor: ActorId) -> Result<i32, Error> {
        let underlying = self.underlying_electrical_resistance(actor)?;
        checked_electrical(self.total_electrical(actor, underlying, |_| true))
    }

    /// Ordinary Electrical resistance query, clamped after evaluating active effects.
    pub fn electrical_resistance(&self, actor: ActorId) -> Result<i32, Error> {
        Ok(self
            .electrical_resistance_after_effects(actor)?
            .clamp(0, 95))
    }

    pub fn item(&self, item: ItemInstanceId) -> Result<&ItemInstance<'content>, Error> {
        self.items.get(&item).ok_or(Error::UnknownItem(item))
    }

    pub fn activation(&self, id: ActivationId) -> Result<&EffectActivation<'content>, Error> {
        self.activations
            .get(&id)
            .ok_or(Error::UnknownActivation(id))
    }

    /// Associate an item with an actor. Grants are explicit separate actions.
    pub fn equip(&mut self, item: ItemInstanceId, actor: ActorId) -> Result<(), Error> {
        self.base_armour_class(actor)?;
        let instance = self.items.get_mut(&item).ok_or(Error::UnknownItem(item))?;
        if let Some(wearer) = instance.equipped_by {
            return Err(Error::AlreadyEquipped { item, wearer });
        }
        instance.equipped_by = Some(actor);
        Ok(())
    }

    /// End the association and remove every activation granted by this instance.
    pub fn unequip(&mut self, item: ItemInstanceId, actor: ActorId) -> Result<(), Error> {
        self.actor(actor)?;
        self.require_wearer(item, actor)?;
        self.validate_remaining_totals(actor, |grant| grant.source != item)?;
        self.activations.retain(|_, grant| grant.source != item);
        self.items
            .get_mut(&item)
            .expect("validated item")
            .equipped_by = None;
        Ok(())
    }

    /// Apply one independent copy of a supported effect to the source's wearer.
    pub fn activate_effect(
        &mut self,
        source: ItemInstanceId,
        target: ActorId,
        definition: &'content EffectDefinition,
    ) -> Result<ActivationId, Error> {
        let actor = self.actor(target)?;
        self.require_wearer(source, target)?;
        let mut points = 0i128;
        let mut fire_resistance_floor: Option<i32> = None;
        let mut electrical_points = 0i128;
        for &operation in &definition.operations {
            match operation {
                EffectOperation::AddArmourClass { points: adjustment } => {
                    points += i128::from(adjustment)
                }
                EffectOperation::ResistanceFloor {
                    channel: ResistanceKind::Fire,
                    minimum_percent,
                } => {
                    fire_resistance_floor = fire_resistance_floor.max(Some(minimum_percent));
                }
                EffectOperation::AddResistance {
                    channel: ResistanceKind::Electrical,
                    percentage_points,
                } => electrical_points += i128::from(percentage_points),
                _ => return Err(Error::UnsupportedOperation(operation)),
            }
        }
        let armour_class_points = checked_ac(points)?;
        let electrical_resistance_points = checked_electrical(electrical_points)?;
        checked_ac(self.total_ac(target, actor.base_ac, |_| true) + points)?;
        checked_electrical(
            self.total_electrical(target, actor.underlying_electrical_resistance, |_| true)
                + electrical_points,
        )?;
        let id = ActivationId(self.allocate_id()?);
        self.activations.insert(
            id,
            EffectActivation {
                id,
                source,
                target,
                definition,
                armour_class_points,
                fire_resistance_floor,
                electrical_resistance_points,
            },
        );
        Ok(id)
    }

    /// Remove exactly one recorded application, leaving its source equipped.
    pub fn remove_activation(&mut self, id: ActivationId) -> Result<(), Error> {
        let target = self.activation(id)?.target;
        self.validate_remaining_totals(target, |grant| grant.id != id)?;
        self.activations.remove(&id);
        Ok(())
    }

    fn require_wearer(&self, item: ItemInstanceId, actor: ActorId) -> Result<(), Error> {
        if self.item(item)?.equipped_by == Some(actor) {
            Ok(())
        } else {
            Err(Error::NotEquippedBy { item, actor })
        }
    }

    fn actor(&self, actor: ActorId) -> Result<&ActorState, Error> {
        self.actors.get(&actor).ok_or(Error::UnknownActor(actor))
    }

    fn total_ac(
        &self,
        actor: ActorId,
        base_ac: i32,
        keep: impl Fn(&EffectActivation<'content>) -> bool,
    ) -> i128 {
        // A wide accumulator makes signed contributions independent of order.
        i128::from(base_ac) + self.sum_adjustments(actor, keep, |grant| grant.armour_class_points)
    }

    fn total_electrical(
        &self,
        actor: ActorId,
        underlying: i32,
        keep: impl Fn(&EffectActivation<'content>) -> bool,
    ) -> i128 {
        i128::from(underlying)
            + self.sum_adjustments(actor, keep, |grant| grant.electrical_resistance_points)
    }

    fn sum_adjustments(
        &self,
        actor: ActorId,
        keep: impl Fn(&EffectActivation<'content>) -> bool,
        points: impl Fn(&EffectActivation<'content>) -> i32,
    ) -> i128 {
        self.activations
            .values()
            .filter(|grant| grant.target == actor && keep(grant))
            .map(|grant| i128::from(points(grant)))
            .sum()
    }

    fn validate_remaining_totals(
        &self,
        target: ActorId,
        keep: impl Fn(&EffectActivation<'content>) -> bool,
    ) -> Result<(), Error> {
        let actor = self.actor(target)?;
        checked_ac(self.total_ac(target, actor.base_ac, &keep))?;
        checked_electrical(self.total_electrical(
            target,
            actor.underlying_electrical_resistance,
            keep,
        ))?;
        Ok(())
    }

    fn allocate_id(&mut self) -> Result<u64, Error> {
        self.last_id = self
            .last_id
            .checked_add(1)
            .ok_or(Error::IdentityExhausted)?;
        Ok(self.last_id)
    }
}

fn checked_ac(value: i128) -> Result<i32, Error> {
    i32::try_from(value).map_err(|_| Error::ArmourClassOverflow)
}

fn checked_electrical(value: i128) -> Result<i32, Error> {
    i32::try_from(value).map_err(|_| Error::ElectricalResistanceOverflow)
}

#[cfg(test)]
mod tests;
