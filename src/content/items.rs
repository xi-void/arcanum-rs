//! Read-only item inspection and a small, binary-independent Food definition.

use super::{Error, Source, read_bounded};
use crate::{prototype, vfs::Vfs};

/// A native key, such as `arcanum:item/prototype-10087`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemId(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoodDefinition {
    pub id: ItemId,
    pub display_name: String,
    /// Base weight in the game's stone units.
    pub base_weight: u32,
    /// Base worth in game currency units, before runtime adjustments.
    pub base_worth: u32,
}

#[derive(Debug)]
pub struct LoadedPrototype {
    pub source: Source,
    pub header: prototype::Header,
    pub food: Option<prototype::FoodFields>,
}

/// Load the selected resource. Other object types expose their header only.
pub fn load_prototype(vfs: &mut Vfs, path: &str) -> Result<LoadedPrototype, Error> {
    let (source, bytes) = read_bounded(vfs, path, prototype::MAX_PROTOTYPE_BYTES)?;
    let result = prototype::decode_header(&bytes).and_then(|header| {
        let food = if header.is_food() {
            Some(prototype::decode_food(&bytes)?.1)
        } else {
            None
        };
        Ok((header, food))
    });
    match result {
        Ok((header, food)) => Ok(LoadedPrototype {
            source,
            header,
            food,
        }),
        Err(error) => Err(Error::Prototype {
            provenance: Box::new(source),
            error,
        }),
    }
}
