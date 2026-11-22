//! Standalone equipment associations and additive armour-class effects.

use crate::content::effects::{EffectDefinition, EffectOperation};
use crate::content::items::{
    AmmoDefinition, ArmourDefinition, FoodDefinition, GenericDefinition, ItemId, KeyDefinition,
    WeaponDefinition,
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
            Self::IdentityExhausted => f.write_str("runtime identity space exhausted"),
        }
    }
}

impl std::error::Error for Error {}

/// Owns mutable state. IDs are local to the issuing runtime and never reused
/// within it. Definitions stay borrowed and immutable.
#[derive(Default)]
pub struct Runtime<'content> {
    last_id: u64,
    actor_base_ac: BTreeMap<ActorId, i32>,
    items: BTreeMap<ItemInstanceId, ItemInstance<'content>>,
    activations: BTreeMap<ActivationId, EffectActivation<'content>>,
}

impl<'content> Runtime<'content> {
    pub fn create_actor(&mut self, base_ac: i32) -> Result<ActorId, Error> {
        let id = ActorId(self.allocate_id()?);
        self.actor_base_ac.insert(id, base_ac);
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
        self.actor_base_ac
            .get(&actor)
            .copied()
            .ok_or(Error::UnknownActor(actor))
    }

    pub fn armour_class(&self, actor: ActorId) -> Result<i32, Error> {
        let base = self.base_armour_class(actor)?;
        checked_ac(self.total_ac(actor, base, |_| true))
    }

    pub fn set_base_armour_class(&mut self, actor: ActorId, base_ac: i32) -> Result<(), Error> {
        self.base_armour_class(actor)?;
        checked_ac(self.total_ac(actor, base_ac, |_| true))?;
        self.actor_base_ac.insert(actor, base_ac);
        Ok(())
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
        let base = self.base_armour_class(actor)?;
        self.require_wearer(item, actor)?;
        checked_ac(self.total_ac(actor, base, |grant| grant.source != item))?;
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
        let base = self.base_armour_class(target)?;
        self.require_wearer(source, target)?;
        let points = definition
            .operations
            .iter()
            .try_fold(0i128, |total, &operation| match operation {
                EffectOperation::AddArmourClass { points } => Ok(total + i128::from(points)),
                _ => Err(Error::UnsupportedOperation(operation)),
            })?;
        let armour_class_points = checked_ac(points)?;
        checked_ac(self.total_ac(target, base, |_| true) + points)?;
        let id = ActivationId(self.allocate_id()?);
        self.activations.insert(
            id,
            EffectActivation {
                id,
                source,
                target,
                definition,
                armour_class_points,
            },
        );
        Ok(id)
    }

    /// Remove exactly one recorded application, leaving its source equipped.
    pub fn remove_activation(&mut self, id: ActivationId) -> Result<(), Error> {
        let target = self.activation(id)?.target;
        let base = self.base_armour_class(target)?;
        checked_ac(self.total_ac(target, base, |grant| grant.id != id))?;
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

    fn total_ac(
        &self,
        actor: ActorId,
        base_ac: i32,
        keep: impl Fn(&EffectActivation<'content>) -> bool,
    ) -> i128 {
        // A wide accumulator makes signed contributions independent of order.
        i128::from(base_ac)
            + self
                .activations
                .values()
                .filter(|grant| grant.target == actor && keep(grant))
                .map(|grant| i128::from(grant.armour_class_points))
                .sum::<i128>()
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

#[cfg(test)]
mod tests;
