//! Original inventories schedule one replacement 12–24 game hours after access.
use super::*;
use crate::random::Random;
use serde::{Deserialize, Serialize};

pub(super) type Events = BTreeMap<String, BTreeMap<String, Restock>>;
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Restock {
    pub(super) merchant: String,
    due: Option<u64>,
    random: Random,
    once_completed: bool,
    error: Option<String>,
}

impl Restock {
    pub(super) fn new(merchant: &str) -> Self {
        Self {
            merchant: merchant.into(),
            due: None,
            random: Random::fresh(),
            once_completed: false,
            error: None,
        }
    }
    pub(super) fn schedule(&mut self, now: u64, npc: &str) {
        self.merchant = npc.to_owned();
        if !self.once_completed {
            self.error = None;
            self.due = Some(now.saturating_add(720 + self.random.below(721)));
        }
    }
}

pub(super) struct SourceSet {
    coins: [i32; 2],
    pub(super) items: Vec<(u32, u32)>,
}
impl SourceSet {
    pub(super) fn coins(&self, random: &mut Random) -> u32 {
        let [low, high] = self.coins;
        (i64::from(low) + random.below((i64::from(high) - i64::from(low) + 1) as u64) as i64).max(0)
            as u32
    }
    pub(super) fn parse(text: &str) -> Result<Self, String> {
        let (_, pairs) = text
            .split_once(':')
            .ok_or("Missing inventory source name")?;
        let mut pairs = pairs.split_ascii_whitespace().map(|pair| {
            let (a, b) = pair
                .split_once(',')
                .ok_or("Incomplete inventory source pair")?;
            Ok::<_, String>((
                a.parse::<i32>().map_err(|e| e.to_string())?,
                b.parse::<i32>().map_err(|e| e.to_string())?,
            ))
        });
        let (low, high) = pairs.next().transpose()?.unwrap_or_default();
        if low > high {
            return Err("Invalid inventory source gold range".into());
        }
        Ok(Self {
            coins: [low, high],
            items: pairs
                .map(|pair| {
                    let (rate, id) = pair?;
                    Ok::<_, String>((
                        rate.try_into().map_err(|_| "Negative inventory rate")?,
                        id.try_into().map_err(|_| "Negative inventory prototype")?,
                    ))
                })
                .collect::<Result<_, _>>()?,
        })
    }
}

impl InventoryReader {
    pub fn update_merchant_stock(
        &mut self,
        world: &mut InteractiveWorld,
        center: (i32, i32),
        size: (u32, u32),
    ) -> bool {
        let now = world.cache.scripts.game_minute;
        let Some(events) = world.cache.inventory.restocks.get(&world.map) else {
            return false;
        };
        let due: Vec<_> = events
            .iter()
            .filter(|(_, e)| e.due.is_some_and(|due| now >= due))
            .map(|(id, e)| (id.clone(), e.clone()))
            .collect();
        let mut changed = false;
        for (key, mut event) in due {
            event.due = None;
            let result = world.exchange(|world| {
                world.restock(
                    &mut self.vfs,
                    &key,
                    &mut event,
                    crate::camera::Viewport::new(center, size),
                )
            });
            match result {
                Ok(updated) => changed |= updated,
                Err(e) => {
                    eprintln!("Merchant restock {key}: {e}");
                    event.error = Some(e);
                }
            }
            world
                .cache
                .inventory
                .restocks
                .get_mut(&world.map)
                .unwrap()
                .insert(key, event);
        }
        changed
    }
}

impl InteractiveWorld {
    pub fn next_stock_minute(&self) -> Option<u64> {
        self.mobile_index.get()?;
        self.cache
            .inventory
            .restocks
            .get(&self.map)?
            .values()
            .filter_map(|event| event.due)
            .min()
    }

    fn restock(
        &mut self,
        vfs: &mut Vfs,
        key: &str,
        event: &mut Restock,
        viewport: crate::camera::Viewport,
    ) -> Result<bool, String> {
        let merchant = parse_id(&event.merchant)?;
        let stock = parse_id(key)?;
        let terms = self.records(vfs).npc_inventory(merchant)?;
        let npc = Owner::OriginalObject(self.map.clone(), merchant);
        let table = load_mes(vfs, "rules/invensource.mes").map_err(|e| e.to_string())?;
        if stock != merchant {
            let container = self.records(vfs).container(stock)?;
            let source = container.inventory_source.unwrap_or_default();
            if source == 0 {
                return Ok(false);
            }
            if container.subtype_flags & 0x400 == 0 {
                if self.cache.ordered.iter().any(|draw| {
                    let (_, (_, y, x, _, _, record, _)) = draw.key;
                    (x, y) == container.tile
                        && record
                            .checked_sub(mobile_draw_slot(0))
                            .and_then(|idx| {
                                self.cache
                                    .entries
                                    .get(&tile_sector_id((x, y)))?
                                    .0
                                    .mobiles
                                    .get(idx)
                            })
                            .is_some_and(|obj| obj.id == stock)
                        && viewport.intersects(draw.bounds)
                }) || self
                    .trade
                    .as_ref()
                    .is_some_and(|trade| trade.stock.key() == key)
                {
                    event.schedule(self.cache.scripts.game_minute, &event.merchant.clone());
                    return Ok(false);
                }
                if terms.substitute != Some(stock) {
                    return Ok(false);
                }
                if terms.source != 0 {
                    self.restock_owner(vfs, &table, npc, terms.source, event)?;
                }
            }
            self.restock_owner(
                vfs,
                &table,
                Owner::Container(self.map.clone(), stock),
                source,
                event,
            )?;
            event.once_completed = container.subtype_flags & 0x200 != 0;
        } else if terms.source != 0 {
            self.restock_owner(vfs, &table, npc, terms.source, event)?;
        }
        self.cache.inventory.revision += 1;
        Ok(true)
    }

    pub(super) fn restock_owner(
        &mut self,
        vfs: &mut Vfs,
        table: &crate::content::LoadedMes,
        owner: Owner,
        source: u32,
        event: &mut Restock,
    ) -> Result<(), String> {
        let text = table
            .mes
            .get(i32::try_from(source).map_err(|e| e.to_string())?)
            .ok_or_else(|| format!("Missing inventory source {source} in {}", table.source))?;
        let set = SourceSet::parse(text)?;
        let mut records = self.records(vfs);
        let layout = records.owner_layout(&self.cache.inventory.state, &owner)?;
        let mut grid = layout.grid;
        let mut retained = BTreeMap::new();
        for (id, item) in layout.items {
            if item.flags & 0x2000 != 0 {
                retained.insert(id, item);
                continue;
            }
            grid.items.remove(&id);
            self.cache.inventory.state.put(item, Place::Retired, None);
        }
        let coins = set.coins(&mut event.random);
        let mut generated = Vec::new();
        if coins > 0 {
            if let Owner::OriginalObject(_, id) = &owner {
                self.adjust_npc_gold(vfs, &id.key(), i64::from(coins))?;
            } else {
                generated.push((9076, Some(coins)));
            }
        }
        for (rate, base) in set.items {
            if event.random.below(100) < u64::from(rate) {
                generated.push((base.checked_add(20).ok_or("Invalid stock prototype")?, None));
            }
        }
        let state = &mut self.cache.inventory.state;
        let mut frames = FrameCache::default();
        for (proto, quantity) in generated {
            let (_, bytes) = crate::content::sector::read_static_prototype(
                vfs,
                crate::prototype::SerializedPrototypeId(proto),
            )
            .map_err(|e| e.to_string())?;
            let category = crate::prototype::decode_header(&bytes)
                .map_err(|e| e.to_string())?
                .object_type;
            if !(5..=14).contains(&category) {
                return Err(format!(
                    "Inventory source {source} specifies non-item prototype {proto}"
                ));
            }
            let mut item =
                Session::prototype_item(vfs, &mut frames, state.generated_id(), proto, category)?;
            if quantity.is_some() {
                item.stats.quantity = quantity;
            }
            if let Some(count) = item.stats.quantity
                && let Some(existing) = retained.values_mut().find(|i| i.same_stack(&item))
            {
                let qty = existing
                    .stats
                    .quantity
                    .unwrap_or_default()
                    .checked_add(count)
                    .filter(|n| *n <= i32::MAX as u32)
                    .ok_or("Stock quantity overflow")?;
                existing.stats.quantity = Some(qty);
                state.put(
                    existing.clone(),
                    Place::Owned(owner.clone()),
                    Some(grid.items[&existing.id].0),
                );
                continue;
            }
            let cells = item.footprint.cells;
            if let Some(slot) = grid.fit(item.id, cells, owner.rows(), None) {
                grid.items.insert(item.id, (slot, cells));
                state.next_generated = state
                    .next_generated
                    .checked_add(1)
                    .ok_or("Item ID limit exceeded")?;
                retained.insert(item.id, item.clone());
                state.put(item, Place::Owned(owner.clone()), Some(slot));
            }
        }
        Ok(())
    }
}
