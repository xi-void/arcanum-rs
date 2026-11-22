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
    pub description: Option<prototype::DescriptionId>,
    pub food: Option<prototype::FoodFields>,
}

/// Load a prototype through the VFS. Armor and Generic expose the common
/// description prefix, Food exposes its selected fields, and other types
/// expose their header.
pub fn load_prototype(vfs: &mut Vfs, path: &str) -> Result<LoadedPrototype, Error> {
    let (source, bytes) = read_bounded(vfs, path, prototype::MAX_PROTOTYPE_BYTES)?;
    let result = prototype::decode_header(&bytes).and_then(|header| {
        let food = if header.is_food() {
            Some(prototype::decode_food(&bytes)?.1)
        } else {
            None
        };
        let description = match food {
            Some(fields) => Some(fields.description),
            None => prototype::decode_item_description(&bytes, header)?,
        };
        Ok((header, description, food))
    });
    match result {
        Ok((header, description, food)) => Ok(LoadedPrototype {
            source,
            header,
            description,
            food,
        }),
        Err(error) => Err(Error::Prototype {
            provenance: Box::new(source),
            error,
        }),
    }
}
