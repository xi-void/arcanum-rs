use super::*;

impl Session {
    pub(super) fn equipped(
        &self,
        slot: EquipmentSlot,
    ) -> Option<&super::super::super::inventory::Change> {
        self.state
            .changes
            .values()
            .find(|change| change.place == Place::Equipped(slot))
    }

    pub(super) fn aptitude(&self) -> i32 {
        self.character.aptitude(&self.rules)
    }

    pub(super) fn weight(&self) -> i64 {
        self.state
            .changes
            .values()
            .filter(|entry| {
                matches!(
                    entry.place,
                    Place::Owned(Owner::Player) | Place::Equipped(_)
                )
            })
            .fold(0i64, |sum, entry| {
                sum.saturating_add(entry.item.weight(self.aptitude()))
            })
    }

    pub(super) fn statistics(&self) -> CharacterStatistics {
        let mut armor = 0;
        let mut resistance = [0; 5];
        let mut incomplete = false;
        let mut hidden_armor = 0;
        let mut hidden_resistance = [0; 5];
        let aptitude = self.aptitude();
        for change in self
            .state
            .changes
            .values()
            .filter(|change| matches!(change.place, Place::Equipped(_)))
        {
            let item = &change.item;
            let stats = &item.stats;
            incomplete |= stats.spells.iter().flatten().any(|&spell| spell != 10_000)
                || item.flags & 0x1e08 != 0;
            if item.category != 7 {
                continue;
            }
            let magic_armor = stats.adjust_magic(stats.magic_armor_class, aptitude);
            armor += i64::from(stats.armor_class.unwrap_or_default()) + magic_armor;
            let unidentified =
                stats.complexity.is_some_and(|value| value > 0) && item.flags & 1 == 0;
            if unidentified {
                hidden_armor += magic_armor;
            }
            let magic = stats.magic_resistance.unwrap_or_default();
            for (index, (total, base)) in resistance
                .iter_mut()
                .zip(stats.resistance.unwrap_or_default())
                .enumerate()
            {
                let magic = stats.adjust_magic(magic[index], aptitude);
                *total += i64::from(base.unwrap_or_default()) + magic;
                if unidentified {
                    hidden_resistance[index] += magic;
                }
            }
        }
        let mut stats = self.character.statistics(
            &self.rules,
            Some(self.weight()),
            armor,
            resistance,
            incomplete,
        );
        let identified = self.character.statistics(
            &self.rules,
            stats.weight,
            armor - hidden_armor,
            std::array::from_fn(|i| resistance[i] - hidden_resistance[i]),
            incomplete,
        );
        // CE item_total_defence uses only appraised armor and resistance.
        stats.defense = 2
            * ((24 * identified.armor_class
                + identified
                    .resistance
                    .into_iter()
                    .zip([25, 15, 10, 16, 15])
                    .map(|(value, weight)| value * weight)
                    .sum::<i32>())
                / 111);
        stats
    }

    pub(super) fn check_equipment(&self, item: &Item, slot: EquipmentSlot) -> Result<(), String> {
        check_item(item)?;
        check_removal(item)?;
        let stats = &item.stats;
        let flags = stats.equipment_flags.unwrap_or_default();
        if !slot.accepts(item.category, item.inventory_art, flags) {
            return Err(format!(
                "Item is incompatible with the {} slot",
                slot.name()
            ));
        }
        if self.character.background == 69 && stats.complexity.is_some_and(|value| value > 0) {
            return Err("Magick allergy prevents equipping magickal items".into());
        }
        if stats.wield_script {
            return Err("Item requires an unsupported wield script".into());
        }
        let art = item
            .effective
            .current_art
            .value()
            .ok_or("Unresolved equipment ART")?;
        if art >> 28 == 6 && art & 0x400 != 0 {
            return Err("Broken items cannot be equipped".into());
        }
        if slot == EquipmentSlot::Armor {
            if self
                .character
                .armor_size()
                .is_none_or(|size| flags & size == 0)
            {
                return Err("Body armor does not fit this character's race".into());
            }
            let gender = self.character.gender();
            if (flags & 8 != 0 && gender != 1) || (flags & 8 == 0 && flags & 16 != 0 && gender != 0)
            {
                return Err("Body armor does not fit this character's gender".into());
            }
        }
        let two_handed = |flags: u32| flags & 0xc == 0xc;
        if (slot == EquipmentSlot::Weapon
            && two_handed(flags)
            && self.equipped(EquipmentSlot::Shield).is_some())
            || (slot == EquipmentSlot::Shield
                && self
                    .equipped(EquipmentSlot::Weapon)
                    .and_then(|change| change.item.stats.equipment_flags)
                    .is_some_and(two_handed))
        {
            return Err("A fixed two-handed weapon requires both hands".into());
        }
        Ok(())
    }

    pub(super) fn load_rules(
        &mut self,
        vfs: &mut Vfs,
        backgrounds: &crate::content::LoadedMes,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let effects = load_mes(vfs, "rules/effect.mes")?;
        let block = self
            .character
            .background
            .checked_mul(10)
            .ok_or("Invalid background index")?;
        let background_value = |offset| {
            backgrounds
                .mes
                .get(block.checked_add(offset)?)?
                .trim()
                .parse::<i32>()
                .ok()
        };
        let effect = |id| {
            effects.mes.get(id).and_then(|value| {
                super::super::super::character::Adjustments::parse(value)
                    .map_err(|error| eprintln!("Character effect {id}: {error}"))
                    .ok()
            })
        };
        let mut terrain_flags: [Vec<(bool, bool)>; 4] = Default::default();
        if matches!(self.character.background, 65 | 67) {
            for (id, value) in load_mes(vfs, "art/tile/tilename.mes")?.mes.entries() {
                if (0..400).contains(&id) {
                    let flags = value
                        .split_once('/')
                        .and_then(|(_, tail)| tail.split_ascii_whitespace().next())
                        .unwrap_or("");
                    terrain_flags[id as usize / 100]
                        .push((flags.contains('n'), flags.contains('s')));
                }
            }
        }
        self.rules = Rules {
            terrain_flags,
            race: self.character.race_effect().and_then(effect),
            gender: (self.character.gender() == 0)
                .then(|| effect(330))
                .flatten(),
            background: (block != 0)
                .then(|| background_value(1).and_then(effect))
                .flatten(),
            encumbrance: effect(158),
        };
        Ok(())
    }

    pub(super) fn body_art(&self, armor: Option<&Item>) -> Result<u32, String> {
        let fragment = armor
            .map(|item| item.stats.use_art.ok_or("Unresolved armor use ART"))
            .transpose()?;
        self.character.body_art(fragment).map_err(str::to_owned)
    }

    fn create_starting_item(
        &mut self,
        vfs: &mut Vfs,
        frames: &mut FrameCache,
        prototype: u32,
        category: u32,
    ) -> Result<Item, String> {
        let item =
            Self::prototype_item(vfs, frames, self.state.generated_id(), prototype, category)?;
        self.state.next_generated += 1;
        Ok(item)
    }

    pub(super) fn prototype_item(
        vfs: &mut Vfs,
        frames: &mut FrameCache,
        id: OriginalId,
        prototype: u32,
        category: u32,
    ) -> Result<Item, String> {
        let prototype = crate::prototype::SerializedPrototypeId(prototype);
        let (source, bytes) = super::super::super::sector::read_static_prototype(vfs, prototype)
            .map_err(|error| error.to_string())?;
        let defaults = objects::prototype_defaults(&bytes, prototype, category)
            .map_err(|error| format!("{source}: {error}"))?;
        let object = objects::Instance {
            id,
            prototype,
            category,
            bitmap: [0; 5],
            fields: Vec::new(),
        };
        let proto = mobile::population::Prototype { source, defaults };
        let inventory_art = proto.defaults.inventory_art;
        Ok(Item {
            id,
            category,
            description: proto.defaults.description,
            prototype: prototype.0,
            instance_source: proto.source.clone(),
            prototype_source: proto.source.clone(),
            origin_map: String::new(),
            original: Place::Owned(Owner::Player),
            authored_slot: None,
            inventory_art,
            footprint: Footprint::load(vfs, frames, inventory_art),
            effective: Effective::new(&object, Some(&proto)),
            flags: proto
                .defaults
                .subtype_flags
                .ok_or("Unresolved starting item flags")?,
            scripted: proto.defaults.inventory_scripts,
            stats: proto.defaults.item.map(|stats| *stats).unwrap_or_default(),
            generated: true,
        })
    }
}

impl SectorCache {
    pub(in crate::content::world) fn update_character_sector(&mut self) {
        let Some(player) = &self.player else { return };
        let aptitude = self
            .entries
            .get(&tile_sector_id(player.tile()))
            .map_or(0, |(sector, _)| sector.aptitude);
        let environment = (62..=68)
            .contains(&self.inventory.character.background)
            .then(|| {
                let tile = player.tile();
                let encoded = self
                    .entries
                    .get(&tile_sector_id(tile))?
                    .0
                    .sound_surfaces
                    .as_ref()?[((tile.1 & 63) * 64 + (tile.0 & 63)) as usize];
                let (natural, sinkable) = self.inventory.rules.terrain_flags
                    [usize::from(encoded / 64)]
                .get(usize::from(encoded % 64))
                .copied()
                .unwrap_or_default();
                let outdoors = self.outdoor_at(tile)?;
                let dark = if self.inventory.character.background == 68 {
                    let (ambient, reference) = self.ambient?;
                    let point = project_world_tile(tile, reference)?;
                    let rgb = lighting::point(
                        self.light_emitters(),
                        point,
                        if outdoors {
                            ambient.outdoor
                        } else {
                            ambient.indoor
                        },
                    );
                    crate::shadow::grayscale(rgb) < 128
                } else {
                    false
                };
                Some(crate::content::character::Environment {
                    daylight: (6..18).contains(&self.hour),
                    outdoors,
                    natural,
                    sinkable,
                    dark,
                })
            })
            .flatten();
        if self.inventory.character.sector_aptitude != aptitude
            || self.inventory.character.environment != environment
        {
            self.inventory.character.environment = environment;
            self.inventory.character.sector_aptitude = aptitude;
            self.inventory.revision += 1;
            self.update_character_movement();
        }
    }

    pub(in crate::content::world) fn update_character_movement(&mut self) {
        if let Some(player) = &mut self.player {
            let statistics = self.inventory.statistics();
            player.speed = statistics.speed.unwrap_or(statistics.attributes[1]);
            player.can_run = statistics
                .encumbrance
                .is_none_or(|tier| (tier as u8) < Encumbrance::Significant as u8);
        }
    }

    pub(in crate::content::world) fn initialize_character(
        &mut self,
        vfs: &mut Vfs,
        frames: &mut FrameCache,
        start: Tile,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let inventory = &mut self.inventory;
        // CE supplies a generic PC when this prototype is absent.
        let pc = crate::prototype::SerializedPrototypeId(16086);
        if let Ok((source, bytes)) = super::super::super::sector::read_static_prototype(vfs, pc) {
            let defaults = objects::prototype_defaults(&bytes, pc, 15)
                .map_err(|error| format!("{source}: {error}"))?;
            if let Some(stats) = defaults.character {
                for (base, value) in inventory.character.base.iter_mut().zip(*stats) {
                    if let Some(value) = value {
                        *base = value;
                    }
                }
            }
            inventory.character.spell_flags = defaults.appearance.spell_flags.unwrap_or_default();
            inventory.character.tech_skills = defaults.tech_skills;
            inventory.character.colleges = defaults.colleges;
            inventory.character.mastery = defaults.mastery;
            inventory.character.fatigue_damage = defaults.fatigue_damage;
            inventory.character.critter_flags =
                defaults.appearance.critter_flags.unwrap_or_default();
            inventory.character.critter_flags2 =
                defaults.appearance.critter_flags2.unwrap_or_default();
            inventory.character.haggle = defaults.haggle;
            inventory.character.background = defaults.background.unwrap_or_default();
            inventory.character.original_palette =
                defaults.original_art.map_or(0, |art| (art >> 4) & 3);
            inventory.character.base_ac = defaults.armor_class.unwrap_or_default();
            inventory.character.hp = defaults.hp;
            inventory.character.fatigue = defaults.fatigue;
            inventory.character.base_resistance = defaults
                .resistance
                .unwrap_or_default()
                .map(Option::unwrap_or_default);
        }
        let backgrounds = load_mes(vfs, "rules/backgrnd.mes")?;
        inventory.load_rules(vfs, &backgrounds)?;
        let block = inventory.character.background * 10;
        let background_value = |offset| {
            backgrounds
                .mes
                .get(block + offset)?
                .trim()
                .parse::<i32>()
                .ok()
        };
        let gold = background_value(3)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or("Missing or invalid background gold in rules/backgrnd.mes")?;
        if gold > 0 {
            let mut item = inventory.create_starting_item(vfs, frames, 9076, 8)?;
            item.stats.quantity = Some(gold);
            inventory.state.relocate(
                item,
                Place::Owned(Owner::Player),
                Some((&Grid::default(), None)),
            )?;
        }
        if inventory.character.tech_skills[2] & 63 != 0 {
            // CE item_generate_inventory gives trained starting PCs crude lockpicks.
            let item = inventory.create_starting_item(vfs, frames, 15178 + 20, 14)?;
            let grid = Grid {
                items: inventory
                    .state
                    .changes
                    .values()
                    .filter(|change| change.place == Place::Owned(Owner::Player))
                    .filter_map(|change| {
                        Some((change.item.id, (change.slot?, change.item.footprint.cells)))
                    })
                    .collect(),
            };
            inventory
                .state
                .relocate(item, Place::Owned(Owner::Player), Some((&grid, None)))?;
        }
        let size = inventory
            .character
            .armor_size()
            .ok_or("Unsupported starting armor size")?;
        // item_generate_inventory uses the stock description IDs. Serialized IDs include the +20 offset.
        let (description, jacket) = match (inventory.character.gender(), size) {
            (0, 1) => (8134, 8118),
            (0, 2) => (8133, 8110),
            (0, 4) => (8135, 8126),
            (1, 1) => (8065, 8118),
            (1, 2) => (8049, 8110),
            (1, 4) => (8069, 8126),
            _ => return Err("Unsupported starting character gender or armor size".into()),
        };
        let prepare =
            |inventory: &mut Session, vfs: &mut Vfs, frames: &mut FrameCache, description: u32| {
                let item = inventory.create_starting_item(vfs, frames, description + 20, 7)?;
                inventory.check_equipment(&item, EquipmentSlot::Armor)?;
                let art = inventory.body_art(Some(&item))?;
                let appearance =
                    Appearance::load(vfs, frames, art).map_err(|error| error.to_string())?;
                Ok::<_, String>((item, appearance))
            };
        let (item, appearance) = prepare(inventory, vfs, frames, description).or_else(|first| {
            prepare(inventory, vfs, frames, jacket).map_err(|second| {
                format!("Starting outfit unavailable ({first}); fallback jacket: {second}")
            })
        })?;
        inventory
            .state
            .put(item, Place::Equipped(EquipmentSlot::Armor), None);
        self.player = Some(Player::new(Arc::new(appearance), start));
        self.update_character_movement();
        Ok(())
    }
}

impl InteractiveWorld {
    pub fn player_appearance(&self) -> Option<u32> {
        self.cache
            .player
            .as_ref()
            .map(|player| player.appearance.art)
    }

    pub fn character_statistics(&self) -> CharacterStatistics {
        self.cache.inventory.statistics()
    }

    pub fn inventory_counters(&self) -> [u64; 5] {
        let mut counts = [0; 5];
        for change in self
            .cache
            .inventory
            .state
            .changes
            .values()
            .filter(|change| change.place == Place::Owned(Owner::Player))
        {
            let item = &change.item;
            let counter = match (item.category, item.stats.ammo_type) {
                (8, _) => Some(0),
                (6, Some(kind @ 0..=3)) => Some(kind as usize + 1),
                _ => None,
            };
            if let Some(index) = counter {
                counts[index] += u64::from(item.stats.quantity.unwrap_or_default());
            }
        }
        counts
    }

    pub fn change_equipment(
        &mut self,
        vfs: &mut Vfs,
        slot: EquipmentSlot,
        incoming: Option<&str>,
        requested: Option<usize>,
    ) -> Result<Value, String> {
        let player = self.cache.player.as_ref().ok_or("Player is unavailable")?;
        let inventory = &self.cache.inventory;
        let worn = inventory.equipped(slot).map(|change| change.item.clone());
        let incoming = incoming
            .map(|key| {
                let id = parse_id(key)?;
                let item = &inventory
                    .state
                    .changes
                    .get(&id)
                    .filter(|change| change.place == Place::Owned(Owner::Player))
                    .ok_or("Equipment must come from the player inventory")?
                    .item;
                inventory.check_equipment(item, slot)?;
                Ok::<_, String>(item.clone())
            })
            .transpose()?;
        if let Some(item) = &incoming
            && (slot == EquipmentSlot::Weapon
                || slot == EquipmentSlot::Shield && item.category == 7)
        {
            // Check the attack ART just as CE sub_464D20 does, without changing the
            // current animation or activating a combat stance.
            let weapon_code = |item: &Item| {
                let fragment = item.stats.use_art.ok_or("Missing weapon ART fragment")?;
                Ok::<_, String>(((fragment >> 6) & 15) as usize)
            };
            let weapon = if slot == EquipmentSlot::Weapon {
                weapon_code(item)?
            } else {
                inventory
                    .equipped(EquipmentSlot::Weapon)
                    .map(|change| weapon_code(&change.item))
                    .transpose()?
                    .unwrap_or(1)
            };
            let shield = slot == EquipmentSlot::Shield
                || inventory
                    .equipped(EquipmentSlot::Shield)
                    .is_some_and(|change| change.item.category == 7);
            let weapon = if weapon == 7 && shield { 3 } else { weapon };
            let code = *b"ABCDEFGHIJKXYNZ"
                .get(weapon)
                .ok_or("Unsupported weapon ART subtype")?;
            let pose = mobile::Pose::decode(player.appearance.art, None)?;
            let stem = pose
                .path
                .strip_suffix("xaa.art")
                .ok_or("Invalid player standing ART")?;
            let path = format!(
                "{stem}{}{}u.art",
                if shield { 's' } else { 'x' },
                char::from(code.to_ascii_lowercase())
            );
            if vfs
                .resolve(&path)
                .map_err(|error| error.to_string())?
                .winner()
                .is_none()
            {
                return Err(format!(
                    "This body has no compatible weapon or shield ART ({path})"
                ));
            }
        }
        if worn.is_none() && incoming.is_none() {
            return Err("Equipment slot is empty".into());
        }
        let backpack = Place::Owned(Owner::Player);
        let displaced = if let Some(item) = worn {
            check_removal(&item)?;
            if item.stats.wield_script {
                return Err("Item requires an unsupported unwield script".into());
            }
            let mut records = self.records(vfs);
            let layout = records.layout(&inventory.state, &Owner::Player, None)?;
            let ignore = incoming.as_ref().map_or(item.id, |item| item.id);
            let cell = layout
                .grid
                .fit(
                    ignore,
                    item.footprint.cells,
                    Owner::Player.rows(),
                    requested,
                )
                .ok_or("No room for the displaced equipment in the player inventory")?;
            Some((item, cell))
        } else {
            None
        };
        let appearance = if slot == EquipmentSlot::Armor {
            let art = inventory.body_art(incoming.as_ref())?;
            if art != player.appearance.art {
                Some(Arc::new(
                    Appearance::load(vfs, &mut FrameCache::default(), art).map_err(|error| {
                        format!("Player cannot use body ART {art:#010x}: {error}")
                    })?,
                ))
            } else {
                None
            }
        } else {
            None
        };
        let sound = incoming
            .as_ref()
            .or(displaced.as_ref().map(|(item, _)| item))
            .map(|item| item.handling_sound(false, inventory.aptitude()));
        let equipped = incoming.as_ref().map(|item| item.id.key());
        let unequipped = displaced
            .as_ref()
            .map(|(item, cell)| (item.id.key(), *cell));
        let inventory = &mut self.cache.inventory;
        if let Some(item) = incoming {
            inventory.state.put(item, Place::Equipped(slot), None);
        }
        if let Some((item, cell)) = displaced {
            inventory.state.put(item, backpack, Some(cell));
        }
        if let Some(appearance) = appearance {
            self.cache
                .player
                .as_mut()
                .expect("validated player")
                .appearance = appearance;
        }
        inventory.revision += 1;
        self.cache.update_character_movement();
        if let Some(id) = sound {
            self.queue_sound(super::super::SoundEvent::Effect(id));
        }
        Ok(
            json!({"slot":slot,"equipped":equipped,"unequipped":unequipped,"statistics":self.character_statistics()}),
        )
    }
}

impl InventoryReader {
    pub fn change_equipment(
        &mut self,
        world: &mut InteractiveWorld,
        slot: EquipmentSlot,
        incoming: Option<&str>,
        requested: Option<usize>,
    ) -> Result<Value, String> {
        world.change_equipment(&mut self.vfs, slot, incoming, requested)
    }

    pub fn transfer_quantity(
        &mut self,
        world: &mut InteractiveWorld,
        item: &str,
        owners: (&str, &str),
        slot: Option<usize>,
        quantity: u32,
    ) -> Result<Value, String> {
        world.transfer_quantity(&mut self.vfs, item, owners.0, owners.1, slot, quantity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires installed original data"]
    fn original_loadout_replacement_and_encumbrance_are_atomic() {
        let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let open = || Vfs::from_game(&game, crate::vfs::Policy::Ce).unwrap();
        let mut vfs = open();
        let catalog = crate::content::maps::Catalog::load(&mut vfs).unwrap();
        let map = catalog.find("5011").unwrap();
        let tile = (80, 102);
        let region = map.files(&mut vfs).unwrap().bounds();
        let mut stream = WorldStream::new(open(), map.directory.clone(), region, tile)
            .unwrap()
            .with_player()
            .unwrap();
        let size = (936, 500);
        stream.prepare(&region.camera(tile).unwrap(), size).unwrap();
        let mut world = stream.into_interactive(|| {}, false).unwrap();
        world.start_player().unwrap();
        let armor = EquipmentSlot::Armor;
        let initial = world.inspect_inventory(&mut vfs, "player").unwrap();
        assert_eq!(initial["counters"], json!([400, 0, 0, 0, 0]));
        let worn = &initial["equipment"][6];
        assert_eq!(worn["prototype"], 8069);
        let suit = worn["id"].as_str().unwrap();
        let clad = world.player_appearance();
        let stats = world.character_statistics();
        assert_eq!(stats.attributes, [8; 8]);
        assert_eq!(stats.weight, Some(40));
        assert_eq!(stats.speed, Some(8));

        // Change body art during an active step. The world position, passage
        // through the current tile, and facing stay on the existing route.
        let player = world.cache.player.as_mut().unwrap();
        player.follow_route([1].into(), false);
        player.update(Duration::ZERO, |_, _| Some((81, 102)));
        let tick = Duration::from_millis(70);
        let position = player.sample(tick).anchor;
        let facing = player.facing();
        world
            .change_equipment(&mut vfs, armor, None, Some(30))
            .unwrap();
        let player = world.cache.player.as_ref().unwrap();
        assert_eq!(player.sample(tick).anchor, position);
        assert_eq!(player.facing(), facing);
        assert!(player.moving());
        assert!(
            player
                .sample(tick)
                .asset
                .source
                .virtual_path
                .ends_with("hmmuwxab.art")
        );
        assert!(
            world
                .change_equipment(&mut vfs, EquipmentSlot::Helmet, Some(suit), None)
                .is_err()
        );
        world
            .change_equipment(&mut vfs, armor, Some(suit), None)
            .unwrap();
        assert_eq!(world.player_appearance(), clad);

        // Use the original medium chainmail prototype for modifiers and use
        // real starter gold as the source of occupied grid cells.
        let add_mail = |world: &mut InteractiveWorld, vfs: &mut Vfs, cell| {
            let inventory = &mut world.cache.inventory;
            let item = inventory
                .create_starting_item(vfs, &mut FrameCache::default(), 8112, 7)
                .unwrap();
            let id = item.id.key();
            inventory
                .state
                .put(item, Place::Owned(Owner::Player), Some(cell));
            id
        };
        let mail = add_mail(&mut world, &mut vfs, 11);
        add_mail(&mut world, &mut vfs, 61);
        world.cache.update_character_movement();
        assert_eq!(world.character_statistics().speed, Some(6));
        assert_eq!(world.cache.player.as_ref().unwrap().speed, 6);

        // An explicit displaced location occupied by gold must roll back both
        // equipment placements and the current appearance.
        let before = world.inspect_inventory(&mut vfs, "player").unwrap();
        assert!(
            world
                .change_equipment(&mut vfs, armor, Some(&mail), Some(0))
                .is_err()
        );
        assert_eq!(world.inspect_inventory(&mut vfs, "player").unwrap(), before);
        assert_eq!(world.player_appearance(), clad);

        world
            .change_equipment(&mut vfs, armor, Some(&mail), None)
            .unwrap();
        let stats = world.character_statistics();
        assert_eq!(stats.armor_class, 10);
        assert_eq!(stats.resistance[0], 23);
        assert_eq!(stats.speed, Some(6));
        assert_ne!(world.player_appearance(), clad);

        for (prototype, kind, counter) in [(7060, 2, 3), (7061, 3, 4)] {
            let item = world
                .cache
                .inventory
                .create_starting_item(&mut vfs, &mut FrameCache::default(), prototype, 6)
                .unwrap();
            let total = item.stats.quantity.expect("original ammunition default");
            assert_eq!(item.stats.ammo_type, Some(kind));
            assert!(total > 1);
            let grid = world
                .records(&mut vfs)
                .layout(&world.cache.inventory.state, &Owner::Player, None)
                .unwrap()
                .grid;
            world
                .cache
                .inventory
                .state
                .relocate(item, Place::Owned(Owner::Player), Some((&grid, None)))
                .unwrap();
            assert_eq!(world.inventory_counters()[counter], u64::from(total));
        }
    }
}
