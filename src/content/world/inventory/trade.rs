//! Merchant terms and inventory operations used by dialogue and barter.
use super::*;
use crate::content::script::{Actor, Attachment, Event, Invocation, Local, Outcome, Script};
use serde::{Deserialize, Serialize};

pub(in crate::content::world) type Accounts = BTreeMap<String, BTreeMap<String, Account>>;

#[derive(Clone, Default, Serialize, Deserialize)]
pub(in crate::content::world) struct Account {
    pub(super) gold: u32,
    pub(super) reaction_delta: i32,
}

pub(in crate::content::world) struct Merchant {
    pub(in crate::content::world) npc: String,
    pub(super) stock: Owner,
    pub(in crate::content::world) response: usize,
    pub(in crate::content::world) destination: i32,
    name: String,
    retail: i32,
    pub(in crate::content::world) reaction: i32,
    predicate: Option<(Script, Attachment)>,
    portrait: Option<Arc<LoadedFrame>>,
    fence: bool,
    buy_all: bool,
    buys: BTreeSet<u32>,
    sources: Vec<u32>,
    buy_list: Option<String>,
}

pub(super) struct NpcInventory {
    predicate: (i32, Local),
    portrait: Option<i32>,
    gold: u32,
    retail: i32,
    flags: u32,
    pub(super) substitute: Option<OriginalId>,
    pub(super) source: u32,
    social: (Option<i32>, Option<i32>, u32),
}

impl InventoryReader {
    pub fn open_starting_shop(
        &mut self,
        world: &mut InteractiveWorld,
    ) -> Result<Option<String>, String> {
        let Some(npc) = world
            .cache
            .entries
            .values()
            .flat_map(|(sector, _)| &sector.mobiles)
            .find(|m| m.category == 16 && !m.conflict)
        else {
            return Ok(None);
        };
        let name = self.npc_name(npc, true);
        let id = npc.id;
        let key = id.key();
        let terms = world.records(&mut self.vfs).npc_inventory(id)?;
        let (base, race, flags) = terms.social;
        let reaction = super::super::npc::initial_reaction(
            base,
            Some(terms.flags),
            race,
            flags,
            &world.character_statistics(),
        )
        .unwrap_or(50);
        let table = load_mes(&mut self.vfs, "rules/invensource.mes").map_err(|e| e.to_string())?;
        world.exchange(|world| {
            let mut event = stock::Restock::new(&key);
            if terms.source > 0 {
                world.restock_owner(
                    &mut self.vfs,
                    &table,
                    Owner::OriginalObject(world.map.clone(), id),
                    terms.source,
                    &mut event,
                )?;
            }
            if let Some(chest) = terms.substitute {
                let source = world
                    .records(&mut self.vfs)
                    .container(chest)?
                    .inventory_source
                    .unwrap_or_default();
                if source > 0 {
                    world.restock_owner(
                        &mut self.vfs,
                        &table,
                        Owner::Container(world.map.clone(), chest),
                        source,
                        &mut event,
                    )?;
                }
            }
            world.open_trade(&mut self.vfs, &key, name, reaction, (0, 0))
        })?;
        Ok(Some(key))
    }
}

impl Records<'_> {
    pub(super) fn npc_inventory(&mut self, id: OriginalId) -> Result<NpcInventory, String> {
        let (_, bytes) = self.read(id)?;
        let obj = objects::decode_instance(&bytes).map_err(|e| e.to_string())?;
        if obj.id != id || obj.category != 16 {
            return Err("Expected a merchant NPC".into());
        }
        let defaults = &self
            .prototypes
            .get(self.vfs, &obj)
            .map_err(|e| e.to_string())?
            .defaults;
        let field = |n| obj.field(n).and_then(FieldValue::scalar);
        let candidate = obj
            .field(142)
            .map_or(defaults.substitute, FieldValue::reference);
        let leader = obj
            .field(129)
            .map_or(defaults.npc_leader, FieldValue::reference);
        let race = obj.field(66).map_or_else(
            || defaults.character.as_ref().and_then(|s| s[27]),
            |v| {
                v.array_entry(27)
                    .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
            },
        );
        let flags = obj
            .appearance()
            .critter_flags
            .or(defaults.appearance.critter_flags)
            .unwrap_or_default();
        let portrait = [(4, 384), (0x8000, 383), (8, 385), (0x20000000, 434)]
            .into_iter()
            .find_map(|(flag, art)| (flags & flag != 0).then_some(art))
            .or_else(|| {
                [375, 378, 376, 377, 379, 380, 381, 382, 376, 382, 381]
                    .get(usize::try_from(race?).ok()?)
                    .copied()
            });
        let mut result = NpcInventory {
            social: (
                field(143).map(|n| n as i32).or(defaults.reaction_base),
                race,
                flags,
            ),
            portrait,
            predicate: obj
                .field(31)
                .map_or(defaults.buy_script, objects::purchase_attachment),
            gold: field(78).unwrap_or(defaults.merchant_gold),
            retail: field(141).map_or(defaults.retail, |v| v as i32),
            flags: field(128).or(defaults.npc_flags).unwrap_or_default(),
            source: field(85).or(defaults.inventory_source).unwrap_or_default(),
            substitute: None,
        };
        if leader.is_none()
            && let Some(candidate) = candidate
            && self.index()?.category(candidate) == Some(2)
        {
            let tile = obj.tile().ok_or("Missing merchant tile")?;
            let stock = self.container(candidate)?;
            if tile
                .0
                .abs_diff(stock.tile.0)
                .max(tile.1.abs_diff(stock.tile.1))
                <= 20
            {
                result.substitute = Some(candidate);
            }
        }
        Ok(result)
    }

    pub(super) fn owner_layout(&mut self, state: &State, owner: &Owner) -> Result<Layout, String> {
        let container = if let Owner::Container(_, id) = owner {
            Some(self.container(*id)?)
        } else {
            None
        };
        self.layout(state, owner, container.as_ref())
    }
}

impl InteractiveWorld {
    pub(in crate::content::world) fn reaction_delta(&self, npc: &str) -> i32 {
        self.cache
            .inventory
            .accounts
            .get(&self.map)
            .and_then(|a| a.get(npc))
            .map_or(0, |a| a.reaction_delta)
    }

    pub fn merchant_portrait(&self) -> Option<&Arc<LoadedFrame>> {
        self.trade.as_ref()?.portrait.as_ref()
    }
    pub fn trading(&self) -> bool {
        self.trade.is_some()
    }
    pub(super) fn merchant_owns(&self, key: &str) -> bool {
        self.trade.as_ref().is_some_and(|t| {
            t.npc == key
                || match t.stock {
                    Owner::Container(_, id) => OriginalId::parse_key(key) == Some(id),
                    _ => false,
                }
        })
    }

    pub fn trade_target(&self) -> Option<&str> {
        self.trade.as_ref().map(|t| t.npc.as_str())
    }
    pub fn trade_diagnostic(&self) -> Option<Value> {
        let trade = self.trade.as_ref()?;
        Some(
            json!({"npc":trade.npc,"name":trade.name,"stock":Place::Owned(trade.stock.clone()).report(),"gold":self.account(&trade.npc)?.gold,
            "reaction":trade.reaction.saturating_add(self.reaction_delta(&trade.npc)),"retail_multiplier":trade.retail,"inventory_sources":trade.sources,"buy_list":trade.buy_list,"restock":self.cache.inventory.restocks.get(&self.map).and_then(|events|events.get(&trade.stock.key()))}),
        )
    }
    pub(super) fn npc_gold(&self, vfs: &mut Vfs, npc: &str) -> Result<u32, String> {
        match self.account(npc) {
            Some(account) => Ok(account.gold),
            None => Ok(self.records(vfs).npc_inventory(parse_id(npc)?)?.gold),
        }
    }
    fn account(&self, npc: &str) -> Option<&Account> {
        self.cache.inventory.accounts.get(&self.map)?.get(npc)
    }

    pub(in crate::content::world) fn open_trade(
        &mut self,
        vfs: &mut Vfs,
        npc: &str,
        name: String,
        reaction: i32,
        (response, destination): (usize, i32),
    ) -> Result<(), String> {
        let id = parse_id(npc)?;
        let mut records = self.records(vfs);
        let terms = records.npc_inventory(id)?;
        let mut sources = vec![terms.source];
        let stock = if let Some(chest) = terms.substitute {
            sources.extend(records.container(chest)?.inventory_source);
            Owner::Container(self.map.clone(), chest)
        } else {
            Owner::OriginalObject(self.map.clone(), id)
        };
        sources.retain(|&id| id != 0);
        let stock_source = if let Owner::Container(_, id) = stock {
            records.container(id)?.inventory_source.unwrap_or_default()
        } else {
            terms.source
        };
        let layout = records.owner_layout(&self.cache.inventory.state, &stock)?;
        let portrait = terms.portrait.and_then(|id| {
            let table = load_mes(vfs, "art/interface/interface.mes").ok()?;
            let path = format!("art/interface/{}", table.mes.get(id)?.trim());
            FrameCache::default().load(vfs, &path, 0, 0, false).ok()
        });
        let (script, initial) = terms.predicate;
        let predicate = if script > 0 {
            Some((
                Script::load(vfs, script)?,
                Attachment {
                    owner: npc.into(),
                    event: Event::BuyObject,
                    script,
                    initial,
                },
            ))
        } else {
            None
        };
        let mut merchant = Merchant {
            predicate,
            portrait,
            npc: npc.into(),
            stock: stock.clone(),
            name,
            retail: terms.retail,
            reaction: reaction.saturating_sub(self.reaction_delta(npc)),
            response,
            destination,
            fence: terms.flags & 0x4000 != 0,
            buy_all: false,
            buys: BTreeSet::new(),
            sources,
            buy_list: None,
        };
        let path = "rules/invensourcebuy.mes";
        if !vfs
            .resolve(path)
            .map_err(|e| e.to_string())?
            .candidates()
            .is_empty()
        {
            let table = load_mes(vfs, path).map_err(|e| e.to_string())?;
            merchant.buy_list = Some(table.source.to_string());
            for &id in &merchant.sources {
                if let Some(entry) = table.mes.get(id as i32) {
                    for token in entry.split_ascii_whitespace() {
                        if token.eq_ignore_ascii_case("all") {
                            merchant.buy_all = true;
                            break;
                        }
                        merchant.buys.insert(
                            token
                                .parse()
                                .map_err(|_| format!("Invalid buy list for source {id}"))?,
                        );
                    }
                }
            }
        }
        let mut gold = self.account(npc).map_or(terms.gold, |a| a.gold);
        let mut changes = Vec::new();
        for (id, (slot, _)) in layout.grid.items {
            let mut item = layout.items[&id].clone();
            if item.category == 8 {
                gold = gold
                    .checked_add(item.stats.quantity.ok_or("Invalid original gold stack")?)
                    .filter(|&v| v <= i32::MAX as u32)
                    .ok_or("Merchant gold overflow")?;
                item.stats.quantity = Some(0);
                changes.push((item, Place::Spent, None));
            } else if item.flags & 1 == 0 && item.stats.complexity.unwrap_or_default() > 0 {
                item.flags |= 1;
                changes.push((item, Place::Owned(stock.clone()), Some(slot)));
            }
        }
        let session = &mut self.cache.inventory;
        for (item, place, slot) in changes {
            session.state.put(item, place, slot);
        }
        session
            .accounts
            .entry(self.map.clone())
            .or_default()
            .entry(npc.into())
            .or_default()
            .gold = gold;
        session.revision += 1;
        if stock_source != 0 {
            session
                .restocks
                .entry(self.map.clone())
                .or_default()
                .entry(stock.key())
                .or_insert_with(|| super::stock::Restock::new(npc))
                .schedule(self.cache.scripts.game_minute, npc);
        }
        self.trade = Some(merchant);
        Ok(())
    }

    pub(super) fn quote(&self, item: &Item, buying: bool) -> Result<(u32, i32), String> {
        let t = self.trade.as_ref().ok_or("Trading is closed")?;
        check_item(item, &self.cache.inventory.state)?;
        check_removal(item)?;
        if item.category == 8 {
            return Err("Gold is the currency".into());
        }
        let character = &self.cache.inventory.character;
        let training = (character.basic_skills[9] >> 6) & 3;
        if buying {
            if training < 3 && item.flags & 2 != 0 {
                return Err("Merchant will not sell this item".into());
            }
        } else {
            if item.stats.worth == 0
                || item
                    .effective
                    .current_art
                    .value()
                    .is_some_and(|a| a >> 28 == 6 && a & 0x400 != 0)
            {
                return Err("Item has no resale value".into());
            }
            if !t.fence && item.flags & 0x8000 != 0 {
                return Err("Merchant refuses stolen goods".into());
            }
            if training < 2
                && let Some((script, attachment)) = &t.predicate
            {
                let invocation = Invocation {
                    attachment,
                    triggerer: Actor::Item(item.stats.name),
                    position: 0,
                    hour: self.cache.hour,
                    follows_player: None,
                    npc_flags: None,
                    locations: [None, None],
                    met: None,
                };
                if !matches!(
                    script
                        .inspect(&invocation, &self.cache.scripts)
                        .map_err(|e| format!(
                            "Merchant purchase script {}: {e}",
                            script.source.virtual_path
                        ))?
                        .outcome,
                    Outcome::Return { run_default: true }
                ) {
                    return Err("Merchant refuses this item".into());
                }
            }
            if training < 2
                && !t.buy_all
                && !item
                    .prototype
                    .checked_sub(20)
                    .is_some_and(|bp| t.buys.contains(&bp))
            {
                return Err("Merchant does not buy this item".into());
            }
        }
        let stats = self.character_statistics();
        let level = crate::content::character::SkillValue::level(
            character.basic_skills[9],
            self.cache.inventory.rules.skill_modifier(9),
            stats.attributes[6],
        );
        let haggle = (4 * level + 10 + if stats.attributes[4] >= 20 { 10 } else { 0 }).min(95);
        let worth = item.worth();
        let reaction = t.reaction.saturating_add(self.reaction_delta(&t.npc));
        let cost = unit_price(buying, worth, t.retail, haggle, reaction, item.stats.hp)?;
        let offer = if buying {
            cost
        } else {
            cost.min(self.account(&t.npc).ok_or("Missing merchant purse")?.gold)
        };
        if offer == 0 {
            return Err("Merchant has no gold".into());
        }
        Ok((offer, worth))
    }

    fn adjust_player_gold(&mut self, vfs: &mut Vfs, amount: i64) -> Result<(), String> {
        if amount == 0 {
            return Ok(());
        }
        let state = &mut self.cache.inventory.state;
        let mut records = Records {
            vfs,
            map: &self.map,
            index: self.mobile_index.get(),
            frames: FrameCache::default(),
            prototypes: Prototypes::default(),
        };
        let layout = records.layout(state, &Owner::Player, None)?;
        let mut stacks: Vec<_> = layout
            .items
            .into_values()
            .filter(|item| item.category == 8)
            .collect();
        stacks.sort_by_key(|item| layout.grid.items[&item.id].0);
        let balance: i64 = stacks
            .iter()
            .map(|i| i64::from(i.stats.quantity.unwrap_or_default()))
            .sum();
        if !(0..=i64::from(i32::MAX)).contains(&(balance + amount)) {
            return Err("Insufficient gold or currency limit exceeded".into());
        }
        if amount > 0 && stacks.is_empty() {
            let mut gold = Session::prototype_item(
                records.vfs,
                &mut records.frames,
                state.generated_id(),
                9076,
                8,
            )?;
            gold.stats.quantity = Some(amount as u32);
            state.relocate(
                gold,
                Place::Owned(Owner::Player),
                Some((&layout.grid, None)),
            )?;
            state.next_generated += 1;
        } else {
            let mut delta = amount;
            for mut stack in stacks {
                let before = i64::from(stack.stats.quantity.unwrap_or_default());
                let change = delta.max(-before);
                let after = (before + change) as u32;
                stack.stats.quantity = Some(after);
                let slot = layout.grid.items[&stack.id].0;
                let (place, slot) = if after == 0 {
                    (Place::Spent, None)
                } else {
                    (Place::Owned(Owner::Player), Some(slot))
                };
                state.put(stack, place, slot);
                delta -= change;
                if delta == 0 {
                    break;
                }
            }
        }
        self.cache.inventory.revision += 1;
        Ok(())
    }

    pub(super) fn adjust_npc_gold(
        &mut self,
        vfs: &mut Vfs,
        npc: &str,
        amount: i64,
    ) -> Result<(), String> {
        let before = self.npc_gold(vfs, npc)?;
        let after = i64::from(before) + amount;
        if !(0..=i64::from(i32::MAX)).contains(&after) {
            return Err("Merchant has insufficient gold or currency limit exceeded".into());
        }
        self.cache
            .inventory
            .accounts
            .entry(self.map.clone())
            .or_default()
            .entry(npc.into())
            .or_default()
            .gold = after as u32;
        self.cache.inventory.revision += 1;
        Ok(())
    }

    // Only owned items and currency participate. Ground art and sector state are untouched.
    pub(in crate::content::world) fn exchange<T>(
        &mut self,
        apply: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let state = self.cache.inventory.state.clone();
        let character = self.cache.inventory.character.clone();
        let accounts = self.cache.inventory.accounts.clone();
        let revision = self.cache.inventory.revision;
        let sounds = self.sounds.len();
        let result = apply(self);
        if result.is_err() {
            self.cache.inventory.state = state;
            self.cache.inventory.character = character;
            self.cache.inventory.accounts = accounts;
            self.cache.inventory.revision = revision;
            self.sounds.truncate(sounds);
            self.cache.update_character_movement();
        }
        result
    }

    pub(super) fn trade_item(
        &mut self,
        vfs: &mut Vfs,
        key: &str,
        from: &str,
        to: &str,
        slot: Option<usize>,
        quantity: Option<u32>,
    ) -> Result<Value, String> {
        let t = self.trade.as_ref().ok_or("Trading is closed")?;
        let buying = self.merchant_owns(from) && to == "player";
        if !buying && !(from == "player" && self.merchant_owns(to)) {
            return Err("Invalid trade participants".into());
        }
        let npc = t.npc.clone();
        let stock = t.stock.clone();
        let item = self
            .records(vfs)
            .item(parse_id(key)?, &self.cache.inventory.state)?;
        let (price, worth) = self.quote(&item, buying)?;
        let units = quantity.or(item.stats.quantity).unwrap_or(1);
        let total = i64::from(units) * i64::from(price);
        if units == 0 || total > i64::from(i32::MAX) {
            return Err("Invalid trade quantity or total".into());
        }
        let (source, dest, payment) = if buying {
            (stock, Owner::Player, total)
        } else {
            (Owner::Player, stock, -total)
        };
        self.exchange(|world| {
            if payment > 0 {
                world.adjust_player_gold(vfs, -payment)?;
            }
            world.adjust_npc_gold(vfs, &npc, payment)?;
            let mut result = world.move_item(
                vfs,
                key,
                Move::Exchange(source, dest, slot),
                world.cache.hour,
                quantity,
            )?;
            if payment < 0 {
                world.adjust_player_gold(vfs, -payment)?;
            }
            if let Some(id) = result["merged_into"].as_str().or(result["id"].as_str())
                && let Some(change) = world.cache.inventory.state.changes.get_mut(&parse_id(id)?)
            {
                change.item.flags |= 1;
            }
            let profit = if buying {
                total - i64::from(worth) * i64::from(units)
            } else {
                i64::from(worth) * i64::from(units) - total
            };
            if profit < 0 && total > 0 {
                world.queue_sound(super::super::SoundEvent::Inventory(8158));
            }
            if profit > 50
                && world
                    .trade
                    .as_ref()
                    .unwrap()
                    .reaction
                    .saturating_add(world.reaction_delta(&npc))
                    <= 80
            {
                let gain = match profit {
                    ..200 => profit / 40,
                    200..1000 => (profit - 200) / 100 + 5,
                    _ => 15,
                };
                let account = world
                    .cache
                    .inventory
                    .accounts
                    .get_mut(&world.map)
                    .and_then(|records| records.get_mut(&npc))
                    .ok_or("Missing merchant purse")?;
                account.reaction_delta = account.reaction_delta.saturating_add(gain as i32);
            }
            result["unit_price"] = json!(price);
            result["total"] = json!(total);
            Ok(result)
        })
    }
}

fn unit_price(
    buying: bool,
    worth: i32,
    retail: i32,
    haggle: i32,
    reaction: i32,
    hp: [i32; 3],
) -> Result<u32, String> {
    let w = i128::from(worth);
    let m = i128::from(retail);
    let h = i128::from(haggle);
    let cost = if buying {
        let factor = crate::content::character::service_price_percent(reaction);
        ((w + w * m * (100 - h) / 10000) * i128::from(factor) / 100)
            .max(w + 1)
            .max(3)
    } else {
        let w = if m > 100 {
            w * (3 * (100 - m) / 8 + 100) / 100
        } else {
            w
        };
        let mut cost = w * (h / 2 + 50) / 100;
        let max = i128::from(hp[0]) + i128::from(hp[1]);
        if hp[2] > 0 && max > 0 {
            cost = cost * (max - i128::from(hp[2])) / max;
        }
        if cost == 1 { 2 } else { cost }
    };
    u32::try_from(cost)
        .ok()
        .filter(|&c| c > 0 && c <= i32::MAX as u32)
        .ok_or_else(|| "Item has no valid trade price".into())
}

impl InteractiveWorld {
    pub(in crate::content::world) fn script_inventory(
        &self,
        vfs: &mut Vfs,
        npc: &str,
        reaction: Option<i32>,
    ) -> Result<crate::content::script::Interaction, String> {
        use crate::content::script::Interaction;
        let id = parse_id(npc)?;
        let mut records = self.records(vfs);
        let terms = records.npc_inventory(id)?;
        let state = &self.cache.inventory.state;
        let stats = self.character_statistics();
        let values = stats.script_values();
        let mut facts = Interaction {
            alignment_base: stats.base[19],
            alignment_modifier: self.cache.inventory.rules.social(0),
            reaction_modifiers: [
                self.cache.inventory.rules.social(1),
                self.cache.inventory.rules.social(2),
            ],
            quest_levels: Some(self.progression_rules().quest_levels.clone()),
            vitals: [
                [stats.current_health, stats.max_health],
                [stats.current_fatigue, stats.max_fatigue],
            ]
            .map(|p| p.map(|n| n.clamp(i32::MIN.into(), i32::MAX.into()) as i32)),
            stats: values,
            skills: stats.skills,
            reaction,
            aptitude: self.character_statistics().aptitude,
            gold: [
                self.inventory_counters()[0] as i64,
                i64::from(self.account(npc).map_or(terms.gold, |a| a.gold)),
            ],
            substitute: terms.substitute.map(|id| id.key()),
            // Live spell effects and summoned followers have no producers in exploration yet.
            spell_flags: self.cache.inventory.character.spell_flags,
            ..Interaction::default()
        };
        let owners = [
            Some(Owner::Player),
            Some(Owner::OriginalObject(self.map.clone(), id)),
            terms
                .substitute
                .map(|id| Owner::Container(self.map.clone(), id)),
        ];
        for (side, owner) in owners.into_iter().enumerate() {
            let Some(owner) = owner else {
                continue;
            };
            let mut entries = Vec::new();
            let members = match &owner {
                Owner::Player => BTreeSet::new(),
                Owner::OriginalObject(_, id) | Owner::Container(_, id) => {
                    records.index()?.members(*id)
                }
            };
            for id in members {
                if state.changes.contains_key(&id) {
                    continue;
                }
                let (_, bytes) = records.read(id)?;
                let object = objects::decode_instance(&bytes).map_err(|e| e.to_string())?;
                let proto = records
                    .prototypes
                    .get(records.vfs, &object)
                    .map_err(|e| e.to_string())?;
                let name = object
                    .field(21)
                    .and_then(FieldValue::scalar)
                    .or_else(|| proto.defaults.item.as_ref().map(|stats| stats.name))
                    .ok_or("Missing inventory item name")?;
                entries.push((
                    object
                        .field(71)
                        .and_then(FieldValue::scalar)
                        .unwrap_or(u32::MAX) as usize,
                    id,
                    name,
                ));
            }
            for change in state.changes.values() {
                if matches!(&change.place,Place::Owned(current) if current==&owner)
                    || owner == Owner::Player && matches!(change.place, Place::Equipped(_))
                {
                    entries.push((
                        change.slot.unwrap_or(usize::MAX),
                        change.item.id,
                        change.item.stats.name,
                    ));
                }
            }
            entries.sort_unstable();
            facts.items.extend(
                entries
                    .into_iter()
                    .map(|(_, id, name)| (side as u8, name, id.key())),
            );
        }
        Ok(facts)
    }
    pub(in crate::content::world) fn commit_script_inventory(
        &mut self,
        vfs: &mut Vfs,
        npc: &str,
        scripts: &mut crate::content::script::State,
    ) -> Result<(), String> {
        use crate::content::script::Transfer;
        let Some(facts) = scripts.interaction.take() else {
            return Ok(());
        };
        if facts.operations.is_empty() {
            return Ok(());
        }
        let actor = Owner::OriginalObject(self.map.clone(), parse_id(npc)?);
        let substitute = facts
            .substitute
            .as_deref()
            .map(parse_id)
            .transpose()?
            .map(|id| Owner::Container(self.map.clone(), id));
        let owner = |side| match side {
            0 => Ok(Owner::Player),
            1 => Ok(actor.clone()),
            2 => substitute.clone().ok_or("Missing substitute inventory"),
            _ => Err("Invalid script actor"),
        };
        let combat = facts
            .operations
            .contains(&Transfer::Attack)
            .then(|| parse_id(npc))
            .transpose()?;
        self.exchange(|world| {
            for operation in facts.operations {
                match operation {
                    Transfer::Attack => (),
                    Transfer::Heal { fatigue, amount } => {
                        let inv = &mut world.cache.inventory;
                        let damage = if fatigue {
                            &mut inv.character.fatigue_damage
                        } else {
                            &mut inv.character.health_damage
                        };
                        *damage = damage.saturating_sub(amount).max(0);
                        if fatigue && *damage == 0 {
                            inv.character.recover_at = None;
                        }
                        inv.revision += 1;
                    }
                    Transfer::Reaction(n) => {
                        world.adjust_npc_gold(vfs, npc, 0)?;
                        let account = world
                            .cache
                            .inventory
                            .accounts
                            .get_mut(&world.map)
                            .and_then(|row| row.get_mut(npc))
                            .ok_or("Unresolved NPC reaction")?;
                        account.reaction_delta = account.reaction_delta.saturating_add(n);
                    }
                    Transfer::Training(skill, tier) => {
                        let stats = world.character_statistics();
                        world.cache.inventory.character.train(skill, tier, &stats)?;
                        world.cache.inventory.revision += 1;
                    }
                    Transfer::Alignment(change) => {
                        let inv = &mut world.cache.inventory;
                        inv.character.base[19] = change.apply(inv.character.base[19]);
                        inv.revision += 1;
                    }
                    Transfer::QuestExperience(level) => {
                        let inv = &mut world.cache.inventory;
                        let xp = inv.rules.progression.reward(level)?;
                        inv.character.gain_experience(xp, &inv.rules);
                        inv.revision += 1;
                    }
                    Transfer::Gold { actor: 0, delta } => world.adjust_player_gold(vfs, delta)?,
                    Transfer::Gold { actor: 1, delta } => world.adjust_npc_gold(vfs, npc, delta)?,
                    Transfer::Gold { .. } => return Err("Invalid currency owner".into()),
                    Transfer::Item { id, from, to } => {
                        world.move_item(
                            vfs,
                            &id,
                            Move::Exchange(owner(from)?, owner(to)?, None),
                            world.cache.hour,
                            None,
                        )?;
                    }
                }
            }
            world.cache.update_character_movement();
            if let Some(id) = combat {
                world.cache.inventory.combat.provoked = Some(id);
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::unit_price;

    #[test]
    fn price_preserves_original_integer_rounding_and_damage() {
        assert_eq!(unit_price(true, 3, 110, 10, 43, [0; 3]), Ok(5));
        assert_eq!(unit_price(false, 3, 110, 10, 43, [0; 3]), Ok(2));
        assert!(unit_price(false, 2, 110, 10, 43, [0; 3]).is_err());
        assert_eq!(unit_price(false, 100, 100, 10, 43, [100, 0, 50]), Ok(27));
        assert_eq!(unit_price(true, 20, 0, 10, 100, [0; 3]), Ok(21));
    }
}
