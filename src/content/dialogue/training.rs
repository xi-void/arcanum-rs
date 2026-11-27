//! CE's T: service dialogue and ET eligibility branch. Payment and the tier
//! change are staged together with the authored response effects.
use super::*;
use crate::content::character::service_price_percent;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Service {
    Offer(Vec<usize>),
    Quote(usize),
    Pay { skill: usize, price: i32 },
    Expert { skill: usize, destination: i32 },
}

impl Service {
    pub(super) fn offer(text: &str) -> Option<Self> {
        if !text.get(..2)?.eq_ignore_ascii_case("t:") {
            return None;
        }
        let mut skills = Vec::new();
        for arg in text[2..].split(',') {
            let (a, b) = arg.split_once('-').unwrap_or((arg, arg));
            let a = a.trim().parse::<usize>().ok()?;
            let b = b.trim().parse::<usize>().ok()?;
            if b >= 16 || a > b {
                return None;
            }
            skills.extend(a..=b);
        }
        // The Classic dialog window has four skill choices and a return.
        (skills.len() <= 4).then_some(Self::Offer(skills))
    }
    pub(super) fn expert(effect: &str) -> Option<Self> {
        let raw = effect.trim();
        if !raw.get(..2)?.eq_ignore_ascii_case("et") {
            return None;
        }
        let (skill, destination) = skill_operands(&raw[2..])?;
        (destination > 0).then_some(Self::Expert { skill, destination })
    }
}

impl Conversation {
    pub(super) fn training_turn(
        &self,
        vfs: &mut Vfs,
        service: Service,
        back: i32,
        stats: &CharacterStatistics,
        scripts: &mut State,
    ) -> Result<Turn, String> {
        let pc = |start| {
            self.generic(start)
                .map(str::to_owned)
                .ok_or_else(|| format!("Missing PC training response {start}"))
        };
        let npc = |vfs: &mut Vfs, start: i32| {
            if self.override_script > 0
                && let Ok(path) = super::super::script::path(vfs, self.override_script)
                && let Some(name) = path
                    .strip_prefix("scr/")
                    .and_then(|s| s.strip_suffix(".scr"))
                && let Ok(resource) = Dialogue::load(vfs, &format!("dlg/{name}.dlg"))
                && let Some(line) = resource.lines.get(&(9999 + start / 1000))
            {
                let text = if stats.gender == 1 {
                    &line.text
                } else {
                    &line.alternate
                };
                if !text.is_empty() {
                    return Ok(text.clone());
                }
            }
            let gender = |g| if g == 1 { 'm' } else { 'f' };
            let dumb = self.npc_intelligence.is_some_and(|i| i <= 4);
            let start = start
                + if dumb {
                    0
                } else {
                    50 * self.social_class.unwrap_or(0)
                };
            let path = format!(
                "mes/gd_cls_{}{}2{}.mes",
                if dumb { "dumb_" } else { "" },
                gender(self.npc_gender.ok_or("Unknown trainer gender")?),
                gender(stats.gender)
            );
            first_line(vfs, &path, start, 50)
        };
        let mut choices = Vec::new();
        let mut choice = |text, step| {
            choices.push(Response {
                id: None,
                text,
                next: back,
                condition: String::new(),
                effect: String::new(),
                available: true,
                barter: false,
                service: step,
            });
        };
        let text = match service {
            Service::Offer(skills) => {
                let names = load_mes(vfs, "mes/skill.mes").map_err(|e| e.to_string())?;
                for i in skills {
                    choice(
                        names
                            .mes
                            .get(i as i32)
                            .ok_or("Missing skill name")?
                            .to_owned(),
                        Some(Service::Quote(i)),
                    );
                }
                choice(pc(800)?, None);
                npc(vfs, 3000)?
            }
            Service::Quote(i) => {
                let s = stats.skills[i];
                if s.training != 0 || s.level < 1 {
                    choice(pc(600)?, None);
                    npc(vfs, if s.training != 0 { 4000 } else { 5000 })?
                } else {
                    let reaction = scripts
                        .interaction
                        .as_ref()
                        .and_then(|s| s.reaction)
                        .ok_or("Unknown trainer reaction")?;
                    let price = service_price_percent(reaction);
                    choice(pc(1)?, Some(Service::Pay { skill: i, price }));
                    choice(pc(100)?, None);
                    npc(vfs, 1000)?.replace("%d", &price.to_string())
                }
            }
            Service::Pay { skill, price } => {
                let facts = scripts
                    .interaction
                    .as_mut()
                    .ok_or("Missing training context")?;
                if facts.gold[0] < i64::from(price) {
                    choice(pc(600)?, None);
                    npc(vfs, 2000)?
                } else {
                    if facts.skills[skill].training != 0 {
                        return Err("Training already received".into());
                    }
                    facts.train(skill, 1)?;
                    facts.adjust(0, -i64::from(price))?;
                    facts.adjust(1, i64::from(price))?;
                    let path = format!(
                        "mes/gd_cls_{}pc2{}.mes",
                        if stats.attributes[4] <= 4 {
                            "dumb_"
                        } else {
                            ""
                        },
                        if self.npc_gender == Some(1) { 'm' } else { 'f' }
                    );
                    choice(
                        first_line(vfs, &path, 1000 + 50 * self.social_class.unwrap_or(0), 50)?,
                        None,
                    );
                    npc(vfs, 6000)?
                }
            }
            Service::Expert { skill, .. } => {
                let key = if stats.skills[skill].level < 9 {
                    6200
                } else {
                    6300
                };
                let path = format!(
                    "mes/gd_{}{}2{}.mes",
                    if self.npc_intelligence.is_some_and(|n| n <= 4) {
                        "dumb_"
                    } else {
                        ""
                    },
                    if self.npc_gender == Some(1) { 'm' } else { 'f' },
                    if stats.gender == 1 { 'm' } else { 'f' }
                );
                choice(pc(600)?, None);
                first_line(vfs, &path, key, 100)?
            }
        };
        let mut turn = Turn {
            line: self.turn.line,
            text,
            responses: choices,
            voice: None,
            speech_id: None,
            dependencies: Vec::new(),
            conditions: self.state.clone(),
        };
        turn.text = substitute(&turn.text, &self.speaker, turn.line, &mut turn.dependencies);
        Ok(turn)
    }
}
fn first_line(vfs: &mut Vfs, path: &str, start: i32, count: i32) -> Result<String, String> {
    let table = load_mes(vfs, path).map_err(|e| e.to_string())?;
    table
        .mes
        .entries()
        .find(|(n, s)| (start..start + count).contains(n) && !s.is_empty())
        .map(|(_, s)| s.to_owned())
        .ok_or_else(|| format!("Missing training text {start} in {path}"))
}
