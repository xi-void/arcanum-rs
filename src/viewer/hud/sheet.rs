//! Classic advancement pages use the same component layout, input and labels as the HUD.
use super::*;
use arcanum_rs::content::{
    character::{
        CharacterStatistics,
        progression::{DEGREE_INT, Investment, ProgressionRules, SPELL_LEVEL, recipe_id},
    },
    schematics::SchematicReport,
    world::{InventoryReader, SchematicComponent},
};

#[derive(Clone, Copy, Debug, Default, Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(in crate::viewer) enum Page {
    #[default]
    Skills,
    Technology,
    Magick,
    Schematics,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(in crate::viewer) enum Entry {
    Attribute(usize),
    Health,
    Fatigue,
    Skill(usize),
    Spell(usize),
    Degree(usize),
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(in crate::viewer) enum Control {
    Page(Page),
    Group(usize),
    College(usize),
    Discipline(usize),
    Buy(Entry),
    Refund(Entry),
    Previous,
    Next,
    Combine,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Value {
    Attribute(usize),
    Points,
    Level,
    XpNext,
    Race,
    Gender,
    Age,
    Health,
    Fatigue,
    Magic,
    Tech,
    Alignment,
    Derived(usize),
    Resist(usize),
    Skill(usize),
    Training(usize),
    Spell(usize),
    Requirement(usize),
    Degree(usize),
    DegreeRequirement(usize),
    Discipline,
    Recipe,
    Description,
    Component(usize),
}
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Image {
    Spell(usize),
    Degree(usize),
    Illustration,
    Alignment,
    Portrait,
    Component(usize),
    Availability(usize),
}

#[derive(Deserialize)]
pub(super) struct Style {
    pub spell_icon_bases: [i32; 16],
    pub degree_icons: [i32; 8],
}

#[derive(Default)]
pub(super) struct Sheet {
    pub open: bool,
    page: Page,
    group: usize,
    college: usize,
    discipline: usize,
    rank: u32,
    pub aptitude: i32,
    pub levels: [u32; 4],
    known: u32,
    degree: u32,
    refund_spell: bool,
    refund_degree: bool,
    revision: Option<u64>,
    message: String,
}
impl Sheet {
    pub(super) fn visible(&self, c: &Element) -> bool {
        if c.sheet_panel
            && (!self.open
                || (self.page == Page::Schematics) != (c.sheet_page == Some(Page::Schematics)))
        {
            return false;
        }
        if c.sheet_page.is_some_and(|p| p != self.page || !self.open) {
            return false;
        }
        match c.action {
            Some(Action::Sheet(Control::Buy(Entry::Spell(r)))) => r == self.known as usize,
            Some(Action::Sheet(Control::Buy(Entry::Degree(r)))) => r == self.degree as usize,
            Some(Action::Sheet(Control::Refund(Entry::Spell(r)))) => {
                self.refund_spell && r.checked_add(1) == Some(self.known as usize)
            }
            Some(Action::Sheet(Control::Refund(Entry::Degree(r)))) => {
                self.refund_degree && r.checked_add(1) == Some(self.degree as usize)
            }
            _ => true,
        }
    }
    pub(super) fn selected(&self, action: Option<Action>) -> bool {
        self.open
            && match action {
                Some(Action::ToggleCharacter) => self.page != Page::Schematics,
                Some(Action::ToggleSchematics) => self.page == Page::Schematics,
                Some(Action::Sheet(Control::Page(p))) => p == self.page,
                Some(Action::Sheet(Control::Group(g))) => g == self.group,
                Some(Action::Sheet(Control::College(c))) => c == self.college,
                Some(Action::Sheet(Control::Discipline(d))) => d == self.discipline,
                _ => false,
            }
    }
    fn investment(&self, entry: Entry) -> Option<Investment> {
        Some(match entry {
            Entry::Attribute(i) if i < 8 => Investment::Attribute(i),
            Entry::Health => Investment::Health,
            Entry::Fatigue => Investment::Fatigue,
            Entry::Skill(r) if r < 4 => Investment::Skill(self.group * 4 + r),
            Entry::Spell(r) if r < 5 => Investment::Spell(self.college * 5 + r),
            Entry::Degree(r) if r < 7 => Investment::Degree {
                discipline: self.discipline,
                rank: r as u32 + 1,
            },
            _ => return None,
        })
    }
    fn text(
        &self,
        field: Value,
        s: &CharacterStatistics,
        r: &ProgressionRules,
        recipe: Option<&SchematicReport>,
        components: &[SchematicComponent],
    ) -> String {
        let label = |i| {
            r.sheet_labels
                .get(i as usize)
                .map(String::as_str)
                .unwrap_or("?")
        };
        let stat = |i| {
            r.stat_names
                .get(i as usize)
                .map(String::as_str)
                .unwrap_or("?")
        };
        let named = |id, value: String| format!("{}  {value}", label(id));
        match field {
            Value::Attribute(i) => s
                .attributes
                .get(i)
                .map_or_else(String::new, ToString::to_string),
            Value::Points => named(0, s.base[21].to_string()),
            Value::Level => named(1, s.level.to_string()),
            Value::XpNext => named(
                2,
                s.next_level_xp
                    .map_or_else(|| "—".into(), |x| (x - s.base[18]).to_string()),
            ),
            Value::Race => named(4, stat(30 + s.race).into()),
            Value::Gender => named(5, stat(28 + s.gender).into()),
            Value::Age => named(6, s.age.to_string()),
            Value::Health => s.current_health.to_string(),
            Value::Fatigue => s.current_fatigue.to_string(),
            Value::Magic => s.aptitude.max(0).to_string(),
            Value::Tech => (-s.aptitude).max(0).to_string(),
            Value::Alignment => (s.alignment / 10).to_string(),
            Value::Derived(i) => [
                s.carry_capacity / 10,
                s.damage_bonus,
                s.ac_adjustment,
                s.speed.unwrap_or(0),
                s.heal_rate,
                s.poison_recovery,
                s.reaction_modifier,
                s.max_followers,
            ]
            .get(i)
            .map_or_else(String::new, |v| format!("{}  {v}", stat(8 + i as i32))),
            Value::Resist(i) => [0, 4, 1, 3, 2].get(i).map_or_else(String::new, |&j| {
                format!("{}  {}%", label(12 + i as i32), s.resistance[j])
            }),
            Value::Skill(row) => r
                .skills
                .get(self.group * 4 + row)
                .cloned()
                .unwrap_or_default(),
            Value::Training(row) => s
                .skills
                .get(self.group * 4 + row)
                .and_then(|v| r.training.get(v.training as usize))
                .cloned()
                .unwrap_or_default(),
            Value::Spell(row) => r
                .spells
                .get(self.college * 5 + row)
                .map_or_else(String::new, |s| s.name.clone()),
            Value::Requirement(row) if row < 5 => r.spells[self.college * 5 + row]
                .willpower
                .as_ref()
                .map_or_else(Clone::clone, |wp| {
                    format!("Level {}   WP {wp}   IN 5", SPELL_LEVEL[row])
                }),
            Value::Degree(row) => recipe_id(self.discipline, row as u32 + 1)
                .and_then(|id| r.recipes.get(&id.0))
                .map_or_else(String::new, |rec| {
                    rec.as_ref().map_or_else(Clone::clone, |g| g.title.clone())
                }),
            Value::DegreeRequirement(row) if row < 7 => {
                format!("IN {}   ·   1 CP", DEGREE_INT[row + 1])
            }
            Value::Discipline => format!(
                "{} {}  {}%",
                r.disciplines[self.discipline],
                r.degree_titles[s.degrees[self.discipline] as usize],
                s.expertise[self.discipline]
            ),
            Value::Recipe => recipe
                .and_then(|rec| rec.title.as_ref())
                .map_or_else(|| "No learned schematic".into(), |v| v.text.clone()),
            Value::Description => recipe
                .and_then(|rec| rec.description.as_ref())
                .map_or_else(String::new, |v| v.text.clone()),
            Value::Component(i) => components
                .get(i)
                .map_or_else(String::new, |c| c.name.clone()),
            _ => String::new(),
        }
    }
}

impl Hud {
    pub(in crate::viewer) fn has_sheet(&self) -> bool {
        self.sheet_style.is_some()
            && self
                .components
                .iter()
                .any(|c| c.config.sheet_panel && !c.config.hidden)
    }
    pub(in crate::viewer) fn sheet_open(&self) -> bool {
        self.sheet.open
    }
    pub(in crate::viewer) fn sheet_message(&self) -> &str {
        &self.sheet.message
    }
    pub(in crate::viewer) fn close_sheet_action(&self) -> Action {
        if self.sheet.page == Page::Schematics {
            Action::ToggleSchematics
        } else {
            Action::ToggleCharacter
        }
    }
    pub(in crate::viewer) fn close_sheet(&mut self) {
        if self.sheet.open {
            self.sheet.open = false;
            self.sheet.revision = None;
            self.sheet.message.clear();
            self.focus = None;
            self.cancel_press();
            self.clear_layout();
        }
    }
    pub(in crate::viewer) fn sheet_action(
        &mut self,
        action: Action,
        world: &mut InteractiveWorld,
        reader: &mut InventoryReader,
    ) {
        if !self.has_sheet() || world.conversation().is_some() {
            return;
        }
        if self.sheet.open && action == self.close_sheet_action() {
            self.close_sheet();
            return;
        }
        self.sheet.message.clear();
        match action {
            Action::ToggleCharacter | Action::ToggleSchematics => {
                self.close_inventory();
                self.close_journal();
                self.cancel_press();
                self.focus = None;
                self.sheet.open = true;
                self.sheet.rank = 1;
                self.sheet.page = if action == Action::ToggleSchematics {
                    Page::Schematics
                } else {
                    Page::Skills
                };
                world.begin_advancement();
            }
            Action::Sheet(control) if self.sheet.open => match control {
                Control::Page(page) => {
                    self.sheet.page = page;
                    self.sheet.rank = 1;
                }
                Control::Group(i) if i < 4 => self.sheet.group = i,
                Control::College(i) if i < 16 => self.sheet.college = i,
                Control::Discipline(i) if i < 8 => {
                    self.sheet.discipline = i;
                    self.sheet.rank = 1;
                }
                Control::Buy(entry) | Control::Refund(entry) => {
                    if let Some(purchase) = self.sheet.investment(entry)
                        && let Err(e) =
                            world.invest(purchase, matches!(control, Control::Refund(_)))
                    {
                        self.sheet.message = e;
                    }
                }
                Control::Previous => self.sheet.rank = self.sheet.rank.saturating_sub(1).max(1),
                Control::Next => {
                    self.sheet.rank = (self.sheet.rank + 1).min(self.sheet.degree.max(1))
                }
                Control::Combine => {
                    self.sheet.message = reader
                        .craft(world, self.sheet.discipline, self.sheet.rank)
                        .unwrap_or_else(|e| e)
                }
                _ => {}
            },
            _ => return,
        }
        self.sheet.revision = None;
        self.clear_layout();
    }
    pub(in crate::viewer) fn refresh_sheet(
        &mut self,
        reader: &mut InventoryReader,
        world: &InteractiveWorld,
    ) {
        if !self.sheet.open || self.sheet.revision == Some(world.inventory_revision()) {
            return;
        }
        let s = world.character_statistics();
        let r = world.progression_rules();
        self.sheet.known = s.colleges[self.sheet.college];
        self.sheet.degree = s.degrees[self.sheet.discipline];
        self.sheet.refund_spell = self.sheet.known > 0
            && world.refundable(Investment::Spell(
                self.sheet.college * 5 + self.sheet.known.saturating_sub(1) as usize,
            ));
        self.sheet.refund_degree = world.refundable(Investment::Degree {
            discipline: self.sheet.discipline,
            rank: self.sheet.degree,
        });
        self.sheet.aptitude = s.aptitude;
        self.sheet.levels =
            std::array::from_fn(|i| s.skills[self.sheet.group * 4 + i].level as u32);
        let recipe = if self.sheet.page == Page::Schematics && self.sheet.degree > 0 {
            match reader.learned_recipe(world, self.sheet.discipline, self.sheet.rank) {
                Ok(report) => Some(report),
                Err(e) => {
                    self.sheet.message = e;
                    None
                }
            }
        } else {
            None
        };
        let components = recipe
            .as_ref()
            .and_then(|report| {
                reader
                    .schematic_components(world, report)
                    .map_err(|e| self.sheet.message = e)
                    .ok()
            })
            .unwrap_or_default();
        for i in 0..self.components.len() {
            let c = &self.components[i].config;
            if c.sheet_page.is_some_and(|p| p != self.sheet.page) {
                continue;
            }
            if let Some(field) = c.sheet_value {
                let text = self
                    .sheet
                    .text(field, &s, r, recipe.as_deref(), &components);
                self.write_label(i, &text);
            } else if let Some(image) = c.sheet_image {
                if matches!(image, Image::Portrait) {
                    self.components[i].art = reader.portrait(s.portrait).unwrap_or_else(|e| {
                        self.sheet.message = e;
                        generated_frame("empty", sprite([1, 1], [0; 4]))
                    });
                    continue;
                }
                if let Image::Component(row) = image {
                    self.components[i].art = components
                        .get(row)
                        .and_then(|c| c.icon.clone())
                        .unwrap_or_else(|| generated_frame("empty", sprite([1, 1], [0; 4])));
                    self.components[i].config.hover_message =
                        components.get(row).map(|c| c.name.clone());
                    continue;
                }
                let style = self.sheet_style.as_ref().unwrap();
                let interface = |id: &i32| r.interface.get(id).map(|p| (p.as_str(), 0));
                let art = match image {
                    Image::Spell(row) if row < (self.sheet.known as usize) => {
                        interface(&(style.spell_icon_bases[self.sheet.college] + row as i32 + 1))
                    }
                    Image::Degree(row) if row < (self.sheet.degree as usize) => {
                        interface(&style.degree_icons[self.sheet.discipline])
                    }
                    Image::Availability(row) => components
                        .get(row)
                        .filter(|c| c.expertise)
                        .and_then(|c| interface(&(if c.present { 831 } else { 830 }))),
                    Image::Alignment => r
                        .interface
                        .get(&254)
                        .map(|p| (p.as_str(), (10 + s.alignment / 100) as usize)),
                    Image::Illustration => recipe
                        .as_ref()
                        .and_then(|rec| rec.illustration.as_ref())
                        .map(|a| (a.asset_source.virtual_path.as_str(), 0)),
                    _ => None,
                };
                let frame = art.and_then(|(p, f)| {
                    reader
                        .sheet_art(p, f)
                        .map_err(|e| self.sheet.message = e)
                        .ok()
                });
                self.components[i].art =
                    frame.unwrap_or_else(|| generated_frame("empty", sprite([1, 1], [0; 4])));
            }
        }
        self.sheet.revision = Some(world.inventory_revision());
        self.clear_layout();
    }
    pub(super) fn sheet_diagnostic(&self) -> serde_json::Value {
        let controls: BTreeMap<_, _> = self
            .components
            .iter()
            .zip(&self.placed)
            .filter(|(c, _)| {
                matches!(
                    c.config.action,
                    Some(Action::ToggleCharacter | Action::ToggleSchematics | Action::Sheet(_))
                )
            })
            .filter_map(|(c, p)| {
                let r = p.as_ref()?.rect;
                Some((&c.config.id, [r.x, r.y, r.w as i32, r.h as i32]))
            })
            .collect();
        serde_json::json!({"open":self.sheet.open,"page":self.sheet.page,"group":self.sheet.group,"college":self.sheet.college,"discipline":self.sheet.discipline,"schematic_rank":self.sheet.rank,"feedback":self.sheet.message,"controls":controls})
    }
}
