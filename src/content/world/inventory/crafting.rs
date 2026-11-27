//! Learned schematics share the original recipe resolver and the player's inventory grid.
use super::*;
use crate::content::{
    character::progression::recipe_id,
    schematics::{self, LegacyRecipeId, SchematicReport},
};

type BasicId = schematics::BasicPrototypeId;
struct Component<'a> {
    reference: BasicId,
    item: Option<&'a Item>,
}
fn components<'a>(
    world: &'a InteractiveWorld,
    report: &SchematicReport,
) -> Option<([Component<'a>; 2], BasicId)> {
    let inv = &world.cache.inventory.state;
    let mut best = None;
    let mut matches = 0;
    for variant in &report.legacy.variants {
        let mut first = None;
        let inputs = variant.ingredients.map(|reference| {
            let item = inv
                .changes
                .values()
                .filter(|c| {
                    c.place == Place::Owned(Owner::Player)
                        && Some(c.item.prototype) == reference.serialized().map(|p| p.0)
                })
                .filter(|c| {
                    if c.item.category == 6 {
                        c.item.stats.quantity.unwrap_or(0) > u32::from(Some(c.item.id) == first)
                    } else {
                        Some(c.item.id) != first
                    }
                })
                .min_by_key(|c| c.slot)
                .map(|c| &c.item);
            first = item.map(|i| i.id);
            Component { reference, item }
        });
        let n = inputs.iter().filter(|s| s.item.is_some()).count();
        if best.is_none() || n > matches {
            best = Some((inputs, variant.product));
            matches = n;
        }
        if n == 2 {
            break;
        }
    }
    best
}
fn adequate(stats: &ItemStats, expertise: &[i32; 8]) -> bool {
    let required = -i64::from(stats.complexity.unwrap_or(0));
    required <= 0
        || expertise
            .get(stats.discipline as usize)
            .is_some_and(|&n| i64::from(n) >= required)
}
pub struct SchematicComponent {
    pub icon: Option<Arc<LoadedFrame>>,
    pub present: bool,
    pub expertise: bool,
    pub name: String,
}
impl InventoryReader {
    pub fn learned_recipe(
        &mut self,
        world: &InteractiveWorld,
        discipline: usize,
        rank: u32,
    ) -> Result<Arc<SchematicReport>, String> {
        let id = recipe_id(discipline, rank).ok_or("Invalid schematic degree")?;
        if world.cache.inventory.character.degrees[discipline] < rank {
            return Err("This degree has not been learned".into());
        }
        self.schematic_reports
            .entry(id.0)
            .or_insert_with(|| {
                schematics::load_schematic(&mut self.vfs, LegacyRecipeId(id.0))
                    .map(Arc::new)
                    .map_err(|e| e.to_string())
            })
            .clone()
    }

    pub fn portrait(&mut self, id: i32) -> Result<Arc<LoadedFrame>, String> {
        self.portrait_size(id, false)
    }
    pub fn portrait_size(&mut self, id: i32, large: bool) -> Result<Arc<LoadedFrame>, String> {
        self.portraits
            .entry((id, large))
            .or_insert_with(|| {
                let table = if (1..1000).contains(&id) {
                    "portrait/userport.mes"
                } else {
                    "portrait/gameport.mes"
                };
                let names = load_mes(&mut self.vfs, table).map_err(|e| e.to_string())?;
                let key = if id <= 0 { 1000 } else { id };
                let name = names
                    .mes
                    .get(key)
                    .ok_or_else(|| format!("Portrait {key} absent in {table}"))?;
                let path = format!(
                    "portrait/{}{}.bmp",
                    name.trim(),
                    if large { "_b" } else { "" }
                );
                let (source, bytes) = read_bounded(&mut self.vfs, &path, 4 * 1024 * 1024)
                    .map_err(|e| e.to_string())?;
                let bmp = crate::content::map_atlas::decode_bmp(&bytes)
                    .map_err(|e| format!("{source}: {e}"))?;
                let frame = crate::art::SpriteFrame {
                    width: bmp.size.0,
                    height: bmp.size.1,
                    pixels: bmp.pixels,
                    hotspot: (0, 0),
                    offset: (0, 0),
                };
                Ok(Arc::new(LoadedFrame {
                    source,
                    input_bytes: bytes.len(),
                    decoded: crate::art::DecodedFrame {
                        frame,
                        frame_count: 1,
                        rotation_count: 1,
                        stored_rotation: 0,
                        stored_palette: 0,
                        palette_key: [0; 3],
                        frames_per_second: 0,
                        action_frame: 0,
                        ignored_trailing_bytes: 0,
                    },
                }))
            })
            .clone()
    }

    pub fn sheet_art(&mut self, path: &str, frame: usize) -> Result<Arc<LoadedFrame>, String> {
        self.frames
            .load(&mut self.vfs, path, frame, 0, false)
            .map_err(|e| e.to_string())
    }

    pub fn schematic_components(
        &mut self,
        world: &InteractiveWorld,
        report: &SchematicReport,
    ) -> Result<Vec<SchematicComponent>, String> {
        let (inputs, _) = components(world, report).ok_or("No supported recipe variant")?;
        let expertise = world.character_statistics().expertise;
        inputs
            .into_iter()
            .map(|input| {
                let resolved = report
                    .items
                    .get(&input.reference)
                    .ok_or("Missing component definition")?;
                let proto = resolved
                    .prototype
                    .as_ref()
                    .ok_or("Missing component prototype")?;
                let (_, bytes) = read_bounded(
                    &mut self.vfs,
                    &proto.source.virtual_path,
                    MAX_PROTOTYPE_BYTES,
                )
                .map_err(|e| e.to_string())?;
                let defaults =
                    objects::prototype_defaults(&bytes, proto.header.id, proto.header.object_type)
                        .map_err(|e| e.to_string())?;
                let stats = input
                    .item
                    .map(|i| &i.stats)
                    .or(defaults.item.as_deref())
                    .ok_or("Missing component properties")?;
                let mut icon = |id| {
                    super::super::super::inventory::inventory_frame(
                        &mut self.vfs,
                        &mut self.frames,
                        id,
                    )
                    .ok()
                };
                let art = stats
                    .paper_doll_art
                    .filter(|n| n >> 28 == 6)
                    .and_then(|n| icon(Some(n | 0x3000)))
                    .or_else(|| icon(defaults.inventory_art));
                Ok(SchematicComponent {
                    icon: art,
                    present: input.item.is_some(),
                    expertise: adequate(stats, &expertise),
                    name: resolved
                        .name
                        .as_ref()
                        .map_or_else(String::new, |n| n.text.clone()),
                })
            })
            .collect()
    }

    pub fn craft(
        &mut self,
        world: &mut InteractiveWorld,
        discipline: usize,
        rank: u32,
    ) -> Result<String, String> {
        let report = self.learned_recipe(world, discipline, rank)?;
        let definition = report.definition.as_ref().ok_or_else(|| {
            format!(
                "Incomplete schematic {}: {:?}",
                report.legacy.id.0, report.diagnostics
            )
        })?;
        let expertise = world.character_statistics().expertise;
        let (inputs, product) = components(world, &report).ok_or("No supported recipe variant")?;
        let mut ingredients = Vec::new();
        for component in inputs {
            let item = component
                .item
                .ok_or("Required components are missing from the backpack")?;
            check_item(item, &world.cache.inventory.state)?;
            check_removal(item)?;
            if !adequate(&item.stats, &expertise) {
                return Err("Insufficient expertise for this component".into());
            }
            ingredients.push(item.id);
        }
        let header = report.items[&product]
            .prototype
            .as_ref()
            .ok_or("Unresolved product prototype")?
            .header;
        world.exchange(|world| {
            for id in ingredients {
                let state = &mut world.cache.inventory.state;
                let mut c = state.changes[&id].clone();
                if c.item.category == 6
                    && let Some(q) = c.item.stats.quantity.as_mut()
                    && *q > 1
                {
                    *q -= 1;
                    state.put(c.item, c.place, c.slot);
                } else {
                    state.put(c.item, Place::Consumed, None);
                }
            }
            let mut grid = world
                .records(&mut self.vfs)
                .layout(&world.cache.inventory.state, &Owner::Player, None)?
                .grid;
            let inv = &mut world.cache.inventory;
            for _ in 0..definition.quantity.get() {
                let item = inv.create_starting_item(
                    &mut self.vfs,
                    &mut self.frames,
                    header.id.0,
                    header.object_type,
                )?;
                let id = item.id;
                let cells = item.footprint.cells;
                let slot = grid
                    .fit(id, cells, Owner::Player.rows(), None)
                    .ok_or("No room for the product in the backpack")?;
                inv.state.put(item, Place::Owned(Owner::Player), Some(slot));
                grid.items.insert(id, (slot, cells));
            }
            inv.revision += 1;
            world.cache.update_character_movement();
            Ok(format!(
                "Created {} × {}",
                definition.quantity, definition.title
            ))
        })
    }
}
