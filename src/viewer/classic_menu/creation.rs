//! The main menu and creation bars use the same canvas, buttons, VFS and
//! fonts as the in-play menu. The middle sheet and barter panel are live HUDs.
use super::*;
use arcanum_rs::content::{
    character::CharacterStatistics,
    world::{CreationBuild, CreationCatalog, InventoryReader},
};

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::viewer) enum Screen {
    Main,
    Single,
    Pick,
    Pregen,
    Identity,
    Allocate,
    Shop,
}
impl Screen {
    pub(super) fn character(self) -> bool {
        matches!(
            self,
            Self::Pregen | Self::Identity | Self::Allocate | Self::Shop
        )
    }
    pub(in crate::viewer) fn panel(self) -> bool {
        matches!(self, Self::Allocate | Self::Shop)
    }
    pub(super) fn arrows(self) -> usize {
        match self {
            Self::Pregen => 2,
            Self::Identity => 8,
            _ => 0,
        }
    }
}

#[derive(Deserialize)]
pub(super) struct Layout {
    pub(super) start_prompt: String,
    single: String,
    identity: String,
    pregen: String,
    arrows: [String; 4],
    arrow_positions: [Point; 8],
    pub(super) portrait: Rect,
    pub(super) name: Rect,
    pub(super) gender: Rect,
    pub(super) race: Rect,
    pub(super) background: Rect,
    stats_title: Rect,
    stats_columns: [Rect; 4],
    stats_spacing: u32,
    description_title: Rect,
    description: Rect,
}
pub(in crate::viewer) struct CreationTheme {
    pub(super) layout: Layout,
    pub(super) single: Asset,
    identity: Asset,
    pregen: Asset,
    arrows: [Vec<Asset>; 4],
    catalog: CreationCatalog,
    stat_names: Mes,
}
impl CreationTheme {
    pub(super) fn load(
        layout: Layout,
        vfs: &mut Vfs,
        files: &mut Vfs,
    ) -> Result<Self, Box<dyn Error>> {
        let mut read = |path: &str, i| hud::resource(vfs, files, path, i).map(Arc::new);
        let single = read(&layout.single, 0)?;
        let identity = read(&layout.identity, 0)?;
        let pregen = read(&layout.pregen, 0)?;
        let mut arrows: [Vec<Asset>; 4] = Default::default();
        for (path, frames) in layout.arrows.iter().zip(&mut arrows) {
            let first = read(path, 0)?;
            let len = first.decoded.frame_count.min(3);
            frames.push(first);
            for i in 1..len {
                frames.push(read(path, i)?);
            }
        }
        Ok(Self {
            layout,
            single,
            identity,
            pregen,
            arrows,
            catalog: CreationCatalog::load(vfs)?,
            stat_names: load_mes(vfs, "mes/stat.mes")?.mes,
        })
    }
    fn arrow(&self, i: usize) -> &[Asset] {
        &self.arrows[if i < 2 { i } else { 2 + i % 2 }]
    }
    pub(super) fn arrow_rect(&self, i: usize) -> Rect {
        asset_rect(self.layout.arrow_positions[i], &self.arrow(i)[0])
    }
}

#[derive(Default)]
pub(in crate::viewer) struct Identity {
    pub(in crate::viewer) allocated: Option<CreationBuild>,
    pub(in crate::viewer) build: Option<CreationBuild>,
    stats: Option<CharacterStatistics>,
    image: Option<Asset>,
    pub(in crate::viewer) name: String,
    pub(super) before_edit: String,
    pub(super) editing: bool,
    race: i32,
    gender: i32,
    portrait: i32,
    background: i32,
    pub(in crate::viewer) pregen: Option<u32>,
    pub(super) description: usize,
}

fn cycle(choices: &[i32], value: i32, step: i32) -> Option<i32> {
    let at = choices.iter().position(|&n| n == value).unwrap_or(0);
    (!choices.is_empty())
        .then(|| choices[(at as i32 + step).rem_euclid(choices.len() as i32) as usize])
}

impl Menu {
    pub(in crate::viewer) fn main() -> Self {
        Self {
            front: Some(Screen::Main),
            ..Self::default()
        }
    }
    pub(in crate::viewer) fn goto(&mut self, screen: Screen) -> Command {
        self.front = Some(screen);
        self.hover = None;
        self.focus = None;
        self.down = None;
        self.message = None;
        self.identity.editing = false;
        Command::Stay
    }
    pub(super) fn labels<'a>(&self, layout: &'a super::Layout) -> &'a [i32] {
        match self.front {
            Some(Screen::Main) => &[460, 461, 462, 463],
            Some(Screen::Single) => &[50, 51, 52, 53, 54],
            Some(Screen::Pick) => &[420, 421, 422],
            _ => &layout.pause_labels,
        }
    }
    pub(super) fn activate_front(
        &mut self,
        control: Control,
        theme: &Classic,
        store: Result<&save::Store, &str>,
    ) -> Command {
        use Screen::*;
        self.message = None;
        match (self.front.unwrap(), control) {
            (Main, Control::Pause(0)) => self.goto(Single),
            (Main, Control::Pause(1)) => {
                self.options = Some(Default::default());
                Command::Stay
            }
            (Main, Control::Pause(2)) => Command::Credits,
            (Main, Control::Pause(3)) => {
                self.confirm = Some(Confirm::Quit);
                Command::Stay
            }
            (Single, Control::Pause(0)) => self.goto(Pick),
            (Single, Control::Pause(1)) => {
                self.slots_screen = Some(SlotScreen::Load);
                self.info = true;
                self.refresh(store, theme);
                Command::Stay
            }
            (Single, Control::Pause(2)) => Command::LastSave,
            (Single, Control::Pause(3)) => Command::Intro,
            (Single, Control::Pause(4)) | (Pick, Control::Pause(2)) | (_, Control::Back) => {
                self.back()
            }
            (Pick, Control::Pause(i @ 0..=1)) => {
                if i == 0 && theme.creation.catalog.pregens.is_empty() {
                    self.message = Some("No pre-generated characters are available.".into());
                    return Command::Stay;
                }
                self.identity = crate::viewer::classic_menu::creation::Identity {
                    gender: 1,
                    pregen: (i == 0)
                        .then(|| theme.creation.catalog.pregens.first().copied())
                        .flatten()
                        .map(|n| n as u32),
                    description: 2,
                    ..Default::default()
                };
                self.goto(if i == 0 { Pregen } else { Screen::Identity });
                Command::Preview
            }
            (Screen::Identity, Control::Name) => {
                if !self.identity.editing {
                    self.identity.before_edit = self.identity.name.clone();
                    self.identity.name.clear();
                    self.identity.editing = true;
                }
                Command::Stay
            }
            (Pregen, Control::Arrow(i)) => {
                let n = self.identity.pregen.unwrap_or(1) as i32;
                self.identity.pregen = cycle(
                    &theme.creation.catalog.pregens,
                    n,
                    if i == 0 { -1 } else { 1 },
                )
                .map(|n| n as u32);
                Command::Preview
            }
            (Screen::Identity, Control::Arrow(i)) => {
                let d = &mut self.identity;
                d.editing = false;
                let step = if i % 2 == 0 { -1 } else { 1 };
                let (portraits, backgrounds) = theme.creation.catalog.choices(d.race, d.gender);
                match i / 2 {
                    0 => d.portrait = cycle(&portraits, d.portrait, step).unwrap_or(0),
                    1 => {
                        let races = (0..8)
                            .filter(|&r| CreationCatalog::playable(r, d.gender))
                            .collect::<Vec<_>>();
                        d.race = cycle(&races, d.race, step).unwrap();
                        d.background = 0;
                        d.portrait = 0;
                        d.description = 1;
                    }
                    2 => {
                        d.background = cycle(&backgrounds, d.background, step).unwrap_or(0);
                        d.description = 2;
                    }
                    _ => {
                        if CreationCatalog::playable(d.race, 1 - d.gender) {
                            d.gender = 1 - d.gender;
                            d.background = 0;
                            d.portrait = 0;
                            d.description = 0;
                        }
                    }
                }
                Command::Preview
            }
            (Screen::Identity, Control::Next)
                if self.identity.name.trim().is_empty()
                    || self.identity.name == theme.label(500, "Choose Name") =>
            {
                self.message = Some(theme.label(506, "You must choose a name."));
                Command::Stay
            }
            (Screen::Identity | Pregen, Control::Next) => Command::Allocate,
            (Allocate, Control::Next) => Command::Shop,
            (Shop, Control::Next) => {
                self.confirm = Some(Confirm::Start);
                Command::Stay
            }
            _ => Command::Stay,
        }
    }

    pub(in crate::viewer) fn confirm_start(&mut self) {
        self.confirm = Some(Confirm::Start);
    }

    pub(in crate::viewer) fn over_navigation(&self) -> bool {
        self.confirm.is_some()
            || matches!(self.hover, Some(Control::Next | Control::Back))
            || self.down.is_some()
    }
    pub(in crate::viewer) fn prepare_preview(
        &mut self,
        theme: &Classic,
        reader: &mut InventoryReader,
    ) -> Result<(), String> {
        let d = &mut self.identity;
        d.allocated = None;
        d.build = None;
        d.stats = None;
        d.image = None;
        if d.portrait == 0 {
            d.portrait = theme
                .creation
                .catalog
                .choices(d.race, d.gender)
                .0
                .first()
                .copied()
                .unwrap_or(0);
        }
        let (build, stats) = reader.creation_preview(
            &theme.creation.catalog,
            d.pregen,
            d.race,
            d.gender,
            d.background,
            d.portrait,
        )?;
        if d.pregen.is_some() {
            d.name.clone_from(&stats.name);
        }
        let image = reader
            .portrait_size(stats.portrait, true)
            .or_else(|_| reader.portrait(stats.portrait))?;
        d.image = Some(image);
        d.stats = Some(stats);
        d.build = Some(build);
        Ok(())
    }

    pub(super) fn front_diagnostic(
        &self,
        theme: &Classic,
        size: Point,
        scale: u16,
    ) -> serde_json::Value {
        let screen = self.front.unwrap();
        let transform = theme.transform(size, scale);
        let mut controls = BTreeMap::new();
        let mut add = |name: String, c| {
            controls.insert(name, transform.rect(theme.rect(c)));
        };
        if self.confirm.is_some() {
            add("yes".into(), Control::Yes);
            add("no".into(), Control::No);
        } else if screen.character() {
            add("next".into(), Control::Next);
            add("back".into(), Control::Back);
            if screen == Screen::Identity {
                add("name".into(), Control::Name);
            }
            for (i, name) in [
                "portrait_prev",
                "portrait_next",
                "race_prev",
                "race_next",
                "background_prev",
                "background_next",
                "gender_prev",
                "gender_next",
            ]
            .into_iter()
            .enumerate()
            .take(screen.arrows())
            {
                add(name.into(), Control::Arrow(i));
            }
        } else {
            for (i, &id) in self.labels(&theme.layout).iter().enumerate() {
                add(theme.label(id, ""), Control::Pause(i));
            }
        }
        serde_json::json!({"screen":screen,"controls":controls,"message":self.message,"name":self.identity.name,"preview":self.identity.stats,"sources":theme.creation.catalog.sources,"confirmation":self.confirm.is_some()})
    }
}

impl Classic {
    pub(in crate::viewer) fn panel_canvas(&self, size: Point, scale: u16) -> (hud::Rect, u32) {
        let scale = u32::from(scale)
            .min(size[0] * 100 / self.layout.canvas[0])
            .min(size[1] * 100 / self.layout.canvas[1]);
        let [w, h] = self.layout.canvas.map(|v| v * scale / 100);
        (
            hud::Rect {
                x: ((size[0] - w) / 2) as i32,
                y: ((size[1] - h) / 2) as i32,
                w,
                h,
            },
            scale,
        )
    }
    pub(in crate::viewer) fn start_flag(&self, pregen: u32) -> Option<i32> {
        self.strings
            .get(550 + pregen as i32)?
            .trim()
            .parse()
            .ok()
            .filter(|n: &i32| *n > 0)
    }
    pub(super) fn render_creation(&mut self, out: &mut Frame, menu: &Menu) {
        let screen = menu.front.unwrap();
        if !screen.panel() {
            let bg = if screen == Screen::Pregen {
                &self.creation.pregen
            } else {
                &self.creation.identity
            };
            out.push(bg, asset_rect(self.layout.slots_at, bg));
            let d = &menu.identity;
            if let Some(image) = &d.image {
                out.push(image, self.creation.layout.portrait);
            }
            if let Some(stats) = &d.stats {
                let (name, body) = self.creation.catalog.description(
                    stats,
                    d.build.as_ref().map_or(0, CreationBuild::background_text),
                );
                let bg_title = name.to_owned();
                let show_bg =
                    d.description == 2 && (stats.background != 0 || screen == Screen::Pregen);
                let (title, description) = if show_bg {
                    (bg_title.clone(), body.to_owned())
                } else if d.description == 0 {
                    (
                        self.label(740 + stats.gender, ""),
                        self.label(742 + stats.gender, ""),
                    )
                } else {
                    (
                        self.label(745, "Race"),
                        self.label(770 + 2 * stats.race + stats.gender, ""),
                    )
                };
                let l = &self.creation.layout;
                let fields = [
                    (
                        l.name,
                        if d.editing {
                            format!("{}|", d.name)
                        } else if d.name.is_empty() {
                            self.label(500, "Choose Name")
                        } else {
                            d.name.clone()
                        },
                    ),
                    (l.gender, self.label(740 + stats.gender, "")),
                    (l.race, self.label(720 + stats.race, "")),
                    (l.background, bg_title),
                    (l.stats_title, self.label(739, "Initial Stats")),
                    (l.description_title, title),
                ];
                for (rect, text) in fields {
                    self.draw_text(out, rect, text, Style::Normal, true);
                }
                self.draw_text(
                    out,
                    self.creation.layout.description,
                    description,
                    Style::Wrapped,
                    true,
                );
                let values = [
                    stats.attributes[0],
                    stats.attributes[2],
                    stats.attributes[1],
                    stats.attributes[3],
                    stats.attributes[4],
                    stats.attributes[6],
                    stats.attributes[5],
                    stats.attributes[7],
                    stats.carry_capacity,
                    stats.damage_bonus,
                    stats.ac_adjustment,
                    stats.speed.unwrap_or_default(),
                    stats.heal_rate,
                    stats.poison_recovery,
                    stats.reaction_modifier,
                ];
                for (i, (n, id)) in values
                    .into_iter()
                    .zip([0, 2, 1, 3, 4, 6, 5, 7, 8, 9, 10, 11, 12, 13, 14])
                    .enumerate()
                {
                    let mut rect = self.creation.layout.stats_columns[i / 4];
                    rect[1] += self.creation.layout.stats_spacing * (i % 4) as u32;
                    self.draw_text(
                        out,
                        rect,
                        format!(
                            "{}: {n}",
                            self.creation.stat_names.get(500 + id).unwrap_or("")
                        ),
                        Style::Normal,
                        false,
                    );
                }
            }
            for i in 0..screen.arrows() {
                let frames = self.creation.arrow(i);
                let index = if menu.down == Some(Control::Arrow(i)) {
                    1
                } else if menu.hover == Some(Control::Arrow(i)) {
                    2
                } else {
                    0
                };
                out.push(
                    &frames[index.min(frames.len() - 1)],
                    self.creation.arrow_rect(i),
                );
            }
        }
        out.push(&self.art[2], asset_rect([0, 0], &self.art[2]));
        out.push(
            &self.art[3],
            asset_rect(self.layout.bottom_at, &self.art[3]),
        );
        for c in [Control::Next, Control::Back] {
            self.button_frame(out, menu, c);
        }
        if let Some(msg) = &menu.message {
            self.draw_message(out, self.layout.message, msg);
        }
        if let Some(id) = match menu.hover {
            Some(Control::Next) => Some(7003),
            Some(Control::Back) => Some(7002),
            _ => None,
        } {
            self.draw_text(
                out,
                self.layout.title,
                self.label(id, ""),
                Style::Normal,
                true,
            );
        }
    }
}
