//! Developer grants reuse native backpack placement, currency and progression.
use super::*;
use crate::content::character::progression::{Investment, SKILL_KEYS, recipe_id};
use crate::developer::PlayerCommand;
use crate::prototype::SerializedPrototypeId;

fn named_index<'a>(
    kind: &str,
    input: &str,
    names: impl Iterator<Item = (usize, &'a str)>,
) -> Result<usize, String> {
    let numeric = input.parse::<usize>().ok();
    let mut matches = names
        .filter(|(i, name)| {
            numeric == Some(*i)
                || (!name.is_empty() && *name != "?" && name.eq_ignore_ascii_case(input))
        })
        .map(|(i, _)| i);
    let id = matches
        .next()
        .ok_or_else(|| format!("Unknown {kind} '{input}'. See progression and help player."))?;
    if matches.any(|i| i != id) {
        return Err(format!("Ambiguous {kind} '{input}'. Use its numeric ID."));
    }
    Ok(id)
}

impl InteractiveWorld {
    pub fn toggle_god_mode(&mut self) -> bool {
        let enabled = &mut self.cache.inventory.combat.god_mode;
        *enabled = !*enabled;
        *enabled
    }

    pub fn player_command(
        &mut self,
        vfs: &mut Vfs,
        command: PlayerCommand,
    ) -> Result<Value, String> {
        let inv = &mut self.cache.inventory;
        let rules = &inv.rules.progression;
        let pc = &mut inv.character;
        let result = match command {
            PlayerCommand::AddItem(prototype, quantity) => {
                return self.grant_items(vfs, prototype, quantity);
            }
            PlayerCommand::AddGold(amount) => {
                if amount <= 0 {
                    return Err("Gold amount must be positive".into());
                }
                self.adjust_player_gold(vfs, i64::from(amount))?;
                self.cache.update_character_movement();
                return Ok(json!({"added":amount, "gold":self.inventory_counters()[0]}));
            }
            PlayerCommand::AddXp(amount) => {
                if amount <= 0 {
                    return Err("Experience amount must be positive".into());
                }
                pc.gain_experience(amount, &inv.rules);
                json!({"xp":pc.base[18], "level":pc.base[17], "points":pc.base[21]})
            }
            PlayerCommand::AddPoints(amount) => {
                pc.base[21] = pc.base[21]
                    .checked_add(amount)
                    .filter(|&n| amount > 0 && n <= 56)
                    .ok_or("Grant must be positive and leave at most 56 unspent points")?;
                json!({"points":pc.base[21]})
            }
            PlayerCommand::SetStat(name, value) => {
                let aliases = ["ST", "DX", "CN", "BE", "IN", "PE", "WP", "CH"];
                let id = named_index(
                    "stat",
                    &name,
                    aliases
                        .into_iter()
                        .enumerate()
                        .chain(rules.stat_names.iter().map(String::as_str).enumerate()),
                )?;
                let previous = pc.base.get(id).copied();
                pc.set_base_stat(id, value)?;
                if previous != Some(value) {
                    inv.purchases.retain(|p| *p != Investment::Attribute(id));
                }
                json!({"stat":id,"base":value})
            }
            PlayerCommand::SetSkill(name, rank) => {
                let id = named_index(
                    "skill",
                    &name,
                    SKILL_KEYS
                        .into_iter()
                        .enumerate()
                        .chain(rules.skills.iter().map(String::as_str).enumerate()),
                )?;
                let previous = pc
                    .basic_skills
                    .iter()
                    .chain(&pc.tech_skills)
                    .nth(id)
                    .map(|v| v & 63);
                pc.set_skill_investment(id, rank)?;
                if previous != Some(rank) {
                    inv.purchases.retain(|p| *p != Investment::Skill(id));
                }
                json!({"skill":id,"investment":rank})
            }
            PlayerCommand::LearnSpell(name) => {
                let id = named_index(
                    "spell",
                    &name,
                    rules.spells.iter().map(|s| s.name.as_str()).enumerate(),
                )?;
                let college = id / 5;
                pc.set_college_rank(college, pc.colleges[college].max((id % 5 + 1) as u32));
                json!({"spell":id,"learned_spells":pc.colleges.iter().sum::<u32>()})
            }
            PlayerCommand::LearnAllSpells => {
                for college in 0..pc.colleges.len() {
                    pc.set_college_rank(college, 5);
                }
                json!({"learned_spells":pc.colleges.iter().sum::<u32>()})
            }
            PlayerCommand::LearnTech(name) => {
                if name.eq_ignore_ascii_case("all") {
                    for discipline in 0..pc.degrees.len() {
                        pc.set_degree_rank(discipline, 7);
                    }
                } else {
                    let id = named_index(
                        "degree (schematic ID or name)",
                        &name,
                        rules.recipes.iter().map(|(&id, r)| {
                            (id as usize, r.as_ref().map_or("", |r| r.title.as_str()))
                        }),
                    )?;
                    let (discipline, rank) = (0..pc.degrees.len())
                        .flat_map(|d| (1..=7).map(move |r| (d, r)))
                        .find(|&(d, r)| recipe_id(d, r).is_some_and(|key| key.0 as usize == id))
                        .ok_or("Unknown discipline or rank")?;
                    pc.set_degree_rank(discipline, pc.degrees[discipline].max(rank));
                }
                json!({"learned_degrees":pc.degrees.iter().sum::<u32>()})
            }
        };
        inv.revision += 1;
        self.cache.update_character_movement();
        Ok(result)
    }

    fn grant_items(
        &mut self,
        vfs: &mut Vfs,
        prototype: SerializedPrototypeId,
        quantity: u32,
    ) -> Result<Value, String> {
        if quantity == 0 || quantity > i32::MAX as u32 {
            return Err("Quantity must be between 1 and 2147483647".into());
        }
        let template = Session::load_item_prototype(
            vfs,
            &mut FrameCache::default(),
            self.cache.inventory.state.generated_id(),
            prototype,
        )?;
        let stack = matches!(template.category, 6 | 8);
        if stack && template.stats.quantity.is_none() {
            return Err("Missing quantity on original stack prototype".into());
        }
        let count = if stack { 1 } else { quantity };
        if count as usize > Owner::Player.rows() * COLUMNS {
            return Err("Too many individual items to fit in the backpack".into());
        }
        self.exchange(|world| {
            let mut items = Vec::new();
            for _ in 0..count {
                let state = &mut world.cache.inventory.state;
                let mut item = template.clone();
                item.id = state.generated_id();
                state.next_generated = state.next_item_counter()?;
                if stack {
                    item.stats.quantity = Some(quantity);
                }
                let placed = world.place_item(vfs, item, Move::Grant, world.cache.hour, None)?;
                items.push(placed.get("merged_into").unwrap_or(&placed["id"]).clone());
            }
            Ok(json!({"prototype":prototype.0,"quantity":quantity,"items":items}))
        })
    }
}
