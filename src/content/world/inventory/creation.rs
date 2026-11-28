//! CE background.c, portrait.c and mainmenu_ui.c operate on the same PC fields
//! used by the in-game character sheet and inventory.
use super::*;

#[derive(Clone)]
pub struct CreationBuild {
    pub(super) character: Character,
    pub(super) purchases: Vec<crate::content::character::progression::Investment>,
}

pub struct CreationCatalog {
    backgrounds: crate::content::LoadedMes,
    descriptions: crate::mes::Mes,
    portraits: Vec<(i32, String)>,
    pub sources: Vec<String>,
    pub pregens: Vec<i32>,
}

impl CreationCatalog {
    pub fn load(vfs: &mut Vfs) -> Result<Self, Box<dyn std::error::Error>> {
        let backgrounds = load_mes(vfs, "rules/backgrnd.mes")?;
        let descriptions = load_mes(vfs, "mes/gameback.mes")?;
        let mut sources = vec![
            backgrounds.source.to_string(),
            descriptions.source.to_string(),
        ];
        let mut portraits = Vec::new();
        for (path, start, end) in [
            ("portrait/gameport.mes", 1000, i32::MAX),
            ("portrait/userport.mes", 1, 999),
        ] {
            if start == 1 && vfs.resolve(path)?.candidates().is_empty() {
                continue;
            }
            let table = load_mes(vfs, path)?;
            sources.push(table.source.to_string());
            for (id, name) in table
                .mes
                .entries()
                .filter(|(id, _)| (start..=end).contains(id))
            {
                portraits.push((id, name.trim().to_ascii_uppercase()));
            }
        }
        // CE enables the bonus pre-generated PCs when Victoria and her ART exist.
        let victoria = crate::prototype::SerializedPrototypeId(16095);
        let bonus = crate::content::sector::read_static_prototype(vfs, victoria)
            .ok()
            .and_then(|(_, bytes)| objects::prototype_defaults(&bytes, victoria, 15).ok())
            .and_then(|d| d.current_art)
            .and_then(|aid| mobile::Pose::decode(aid, None).ok())
            .is_some_and(|pose| vfs.resolve(&pose.path).is_ok_and(|r| r.winner().is_some()));
        let pregens = (1..=if bonus { 12 } else { 8 })
            .filter(|i| {
                !vfs.paths_with_prefix(&format!("proto/{:06} - ", 16086 + i))
                    .is_empty()
            })
            .collect();
        Ok(Self {
            pregens,
            backgrounds,
            descriptions: descriptions.mes,
            portraits,
            sources,
        })
    }

    pub fn description(&self, stats: &CharacterStatistics, authored: i32) -> (&str, &str) {
        let id = if authored > 0 {
            Some(authored)
        } else {
            self.backgrounds
                .mes
                .get(stats.background * 10)
                .and_then(|s| s.trim().parse().ok())
        };
        let text = id.and_then(|k| self.descriptions.get(k)).unwrap_or("");
        let (name, body) = text.split_once('\n').unwrap_or((text, ""));
        (name.trim(), body.trim())
    }

    pub fn choices(&self, race: i32, gender: i32) -> (Vec<i32>, Vec<i32>) {
        let Some(code) = ["HU", "DW", "EL", "HE", "GN", "HA", "HO", "HG"].get(race as usize) else {
            return Default::default();
        };
        let prefix = format!("{code}{}", if gender == 0 { 'F' } else { 'M' });
        let portraits = self
            .portraits
            .iter()
            .filter(|(_, name)| name.starts_with(&prefix) || name.starts_with("ANY"))
            .map(|(id, _)| *id)
            .collect();
        let backgrounds = self
            .backgrounds
            .mes
            .entries()
            .filter(|(id, _)| id % 10 == 2)
            .filter(|(_, s)| {
                let s = s.trim().to_ascii_uppercase();
                s.is_empty() || s.contains(&prefix) || s.contains("ANY")
            })
            .map(|(id, _)| id / 10)
            .collect();
        (portraits, backgrounds)
    }

    pub fn playable(race: i32, gender: i32) -> bool {
        (0..8).contains(&race) && (gender == 1 || gender == 0 && matches!(race, 0 | 2 | 3 | 6))
    }
}

impl CreationBuild {
    pub fn set_name(&mut self, name: String) {
        self.character.name = name;
    }
    pub fn background_text(&self) -> i32 {
        self.character.background_text
    }
}

impl InventoryReader {
    pub fn equip_starting_possessions(
        &mut self,
        world: &mut InteractiveWorld,
    ) -> Result<(), String> {
        let player = self.target(world, crate::content::world::abilities::Aim::Player)?;
        let counts = world.inventory_counters();
        for slot in [EquipmentSlot::Weapon, EquipmentSlot::Armor] {
            let session = &world.cache.inventory;
            if session.equipped(slot).is_some() {
                continue;
            }
            let mut eligible: Vec<_> = session
                .state
                .changes
                .values()
                .filter(|c| {
                    c.place == Place::Owned(Owner::Player)
                        && session.check_equipment(&c.item, slot).is_ok()
                })
                .filter_map(|c| {
                    let rank = if slot == EquipmentSlot::Weapon {
                        session.weapon_score(&player, &c.item, counts)?
                    } else {
                        i64::from(c.item.worth())
                    };
                    Some((rank, c.order, c.item.id))
                })
                .collect();
            eligible.sort_unstable_by(|a, b| b.cmp(a));
            for (_, _, id) in eligible {
                match world.change_equipment(&mut self.vfs, slot, Some(&id.key()), None) {
                    Ok(_) => break,
                    Err(reason) => eprintln!("Starting {} {}: {reason}", slot.name(), id.key()),
                }
            }
        }
        Ok(())
    }

    pub fn creation_preview(
        &mut self,
        catalog: &CreationCatalog,
        pregen: Option<u32>,
        race: i32,
        gender: i32,
        background: i32,
        portrait: i32,
    ) -> Result<(CreationBuild, CharacterStatistics), String> {
        let mut character = Session::original_character(&mut self.vfs, 16086 + pregen.unwrap_or(0))
            .map_err(|e| e.to_string())?;
        if pregen.is_none() {
            let (portraits, backgrounds) = catalog.choices(race, gender);
            if !CreationCatalog::playable(race, gender)
                || !backgrounds.contains(&background)
                || !portraits.contains(&portrait)
            {
                return Err("Invalid character creation selection".into());
            }
            character.base[26] = gender;
            character.base[27] = race;
            character.sound_effect = Some(10 * (gender + 2 * race + 1));
            character.background = background;
            character.background_text = 0;
            character.portrait = portrait;
        }
        let mut session = Session {
            character,
            ..Default::default()
        };
        session
            .load_rules(&mut self.vfs, &catalog.backgrounds)
            .map_err(|e| e.to_string())?;
        let stats = session.statistics();
        Ok((
            CreationBuild {
                character: session.character,
                purchases: vec![],
            },
            stats,
        ))
    }
}

impl InteractiveWorld {
    pub fn resume_creation_allocations(&mut self, build: &CreationBuild) {
        self.cache.inventory.purchases.clone_from(&build.purchases);
    }
    pub fn creation_build(&self) -> CreationBuild {
        CreationBuild {
            character: self.cache.inventory.character.clone(),
            purchases: self.cache.inventory.purchases.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires the installed original game"]
    fn installed_creation_bodies_backgrounds_and_manuals() {
        let root = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let mut vfs = Vfs::from_game(&root, crate::vfs::Policy::Ce).unwrap();
        let catalog = CreationCatalog::load(&mut vfs).unwrap();
        let mut reader = InventoryReader::new(vfs);
        let mut combinations = 0;
        for race in 0..8 {
            for gender in 0..2 {
                if !CreationCatalog::playable(race, gender) {
                    continue;
                }
                let (portraits, backgrounds) = catalog.choices(race, gender);
                assert!(backgrounds.contains(&0));
                let (build, s) = reader
                    .creation_preview(&catalog, None, race, gender, 0, portraits[0])
                    .unwrap();
                assert_eq!(
                    (s.race, s.gender, s.portrait, s.base[21]),
                    (race, gender, portraits[0], 5)
                );
                reader.portrait(s.portrait).unwrap();
                let art = build.character.body_art(None).unwrap();
                Appearance::load(&mut reader.vfs, &mut FrameCache::default(), art).unwrap();
                combinations += 1;
            }
        }
        assert_eq!(combinations, 12);
        for index in &catalog.pregens {
            let (_, stats) = reader
                .creation_preview(&catalog, Some(*index as u32), 0, 1, 0, 0)
                .unwrap();
            assert!(!stats.name.is_empty());
        }
        let mut frames = FrameCache::default();
        for background in [0, 55] {
            let portrait = catalog.choices(0, 1).0[0];
            let (build, stats) = reader
                .creation_preview(&catalog, None, 0, 1, background, portrait)
                .unwrap();
            let mut pc = Session {
                character: build.character,
                ..Default::default()
            };
            pc.load_rules(&mut reader.vfs, &catalog.backgrounds)
                .unwrap();
            let functional = stats.attributes[4];
            let captured = if background == 55 { 1 } else { functional };
            assert_eq!(stats.speech_intelligence, captured);
            assert_eq!(
                catalog
                    .backgrounds
                    .mes
                    .get(background * 10 + 3)
                    .unwrap()
                    .trim(),
                if background == 55 { "0" } else { "400" }
            );
            let book = pc
                .create_starting_item(&mut reader.vfs, &mut frames, 15208, 14)
                .unwrap();
            let base = stats.expertise[1];
            let id = book.id;
            assert!(book.carry.is_some() && !book.scripted);
            pc.state.put(book, Place::Owned(Owner::Player), Some(0));
            pc.refresh_worn();
            assert_eq!(pc.statistics().expertise[1], base + captured);
            pc.character.base[4] += 1;
            pc.refresh_worn();
            assert_eq!(pc.state.changes[&id].item.insert_stat, Some(captured as u8));
            pc.state.changes.get_mut(&id).unwrap().place = Place::Ground("test".into(), (1, 1));
            pc.refresh_worn();
            assert_eq!(pc.statistics().expertise[1], base);
        }
        assert_eq!(
            crate::content::script::slide_sequence(&mut reader.vfs, 999).unwrap(),
            (45..=55).collect::<Vec<_>>()
        );
    }
}
