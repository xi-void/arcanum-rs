//! Original college spell programs. A malformed or unsupported component disables that
//! program alone and retains its MES entry number for runtime diagnostics.
pub(crate) use crate::content::vitals::Channel;
use crate::mes::Mes;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(crate) enum Phase {
    Begin,
    Maintain,
    End,
    Callback,
    EndCallback,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Component {
    pub phase: Phase,
    pub targets: Vec<String>,
    pub apply: Vec<String>,
    pub palette: Option<u8>,
    pub spell_flags: u32,
    pub operation: Operation,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) enum Operation {
    Eye {
        index: i32,
        add: bool,
        play: Vec<String>,
    },
    Effect {
        id: i32,
        add: bool,
        count: i32,
        scaled: i32,
    },
    Flag {
        field: Field,
        mask: u32,
        on: bool,
    },
    Vital {
        heal: bool,
        channel: Channel,
        min: i32,
        max: i32,
        flags: Vec<String>,
    },
    Form(i32),
    Summon {
        prototype: u32,
        list: Option<usize>,
    },
    Destroy,
    Dispel,
    Identify,
    Move(String),
    Goal(String),
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize)]
pub(crate) enum Field {
    Object,
    Spell,
    Critter,
    Npc,
    Portal,
    Container,
    Item,
}

#[derive(Default, Clone, Debug, Serialize)]
pub(crate) struct Program {
    pub aggressive: bool,
    pub cost: i32,
    pub range: u32,
    pub targets: Vec<String>,
    pub radius: u32,
    pub maintain: Option<(i32, u64)>,
    pub duration: Option<u64>,
    pub triggers: Option<u32>,
    pub save: Option<(usize, i32)>,
    pub no_stack: bool,
    pub no_reflect: bool,
    pub maintain_targets: Vec<String>,
    pub disallowed_caster: u32,
    pub disallowed_target: u32,
    pub disallowed_critter: u32,
    pub cancels: u32,
    pub missile: Option<i32>,
    pub components: Vec<Component>,
}

fn words(s: &str) -> Vec<String> {
    s.split('|').map(|s| s.trim().to_owned()).collect()
}
fn clauses(s: &str) -> impl Iterator<Item = (&str, &str)> {
    s.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.split_once(':')
                .map(|(k, v)| (k.trim(), v.trim()))
                .unwrap_or((s, ""))
        })
}
fn num<T: std::str::FromStr>(s: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("invalid number {s:?}"))
}
fn pair(s: &str) -> Result<(&str, &str), String> {
    s.strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .and_then(|s| s.split_once('@'))
        .map(|(a, b)| (a.trim(), b.trim()))
        .ok_or_else(|| format!("invalid pair {s}"))
}

pub(crate) fn flag(s: &str) -> Result<(Field, u32), String> {
    use Field::*;
    Ok(match s {
        "of_dontdraw" => (Object, 0x00000100),
        "osf_invisible" => (Spell, 0x00000001),
        "osf_body_of_air" => (Spell, 0x00000004),
        "osf_body_of_earth" => (Spell, 0x00000008),
        "osf_body_of_fire" => (Spell, 0x00000010),
        "osf_body_of_water" => (Spell, 0x00000020),
        "osf_detecting_alignment" => (Spell, 0x00000080),
        "osf_detecting_traps" => (Spell, 0x00000100),
        "osf_detecting_invisible" => (Spell, 0x00000200),
        "osf_shielded" => (Spell, 0x00000400),
        "osf_anti_magic_shell" => (Spell, 0x00000800),
        "osf_bonds_of_magic" => (Spell, 0x00001000),
        "osf_full_reflection" => (Spell, 0x00002000),
        "osf_illusion" => (Spell, 0x00008000),
        "osf_stoned" => (Spell, 0x00010000),
        "osf_polymorphed" => (Spell, 0x00020000),
        "osf_shrunk" => (Spell, 0x00080000),
        "osf_water_walking" => (Spell, 0x00200000),
        "osf_charmed" => (Spell, 0x00800000),
        "osf_entangled" => (Spell, 0x01000000),
        "osf_spoken_with_dead" => (Spell, 0x02000000),
        "osf_tempus_fugit" => (Spell, 0x04000000),
        "osf_mind_controlled" => (Spell, 0x08000000),
        "osf_enshrouded" => (Spell, 0x20000000),
        "osf_familiar" => (Spell, 0x40000000),
        "osf_hardened_hands" => (Spell, 0x80000000),
        "opf_locked" => (Portal, 0x0001),
        "opf_magically_held" => (Portal, 0x0004),
        "ocof_locked" => (Container, 0x0001),
        "oif_identified" => (Item, 0x00000001),
        "ocf_undead" => (Critter, 0x00000004),
        "ocf_fleeing" => (Critter, 0x00000010),
        "ocf_paralyzed" => (Critter, 0x00000040),
        "ocf_blinded" => (Critter, 0x00000080),
        "ocf_monster" => (Critter, 0x00008000),
        "onf_kos_override" => (Npc, 0x00000800),
        "onf_familiar" => (Npc, 0x00008000),
        _ => return Err(format!("unknown object flag {s}")),
    })
}
fn mask(s: &str, field: Field) -> Result<u32, String> {
    s.split('|').try_fold(0, |m, s| {
        let (f, b) = flag(s.trim())?;
        if f != field {
            return Err(format!("unexpected {s} field"));
        }
        Ok(m | b)
    })
}
impl Program {
    pub(crate) fn remaining_dependencies(&self) -> std::collections::BTreeSet<&'static str> {
        let mut dependencies = std::collections::BTreeSet::new();
        if self.targets.iter().any(|t| t == "tgt_all_party_critters") {
            dependencies.insert("original_party_membership");
        }
        for c in &self.components {
            match c.operation {
                Operation::Flag {
                    field: Field::Spell,
                    mask: 0x100,
                    on: true,
                } => {
                    dependencies.insert("trap_discovery");
                }
                Operation::Flag {
                    field: Field::Spell,
                    mask: 0x00800000 | 0x08000000,
                    on: true,
                }
                | Operation::Flag {
                    field: Field::Critter,
                    mask: 0x10 | 0x40 | 0x80,
                    on: true,
                }
                | Operation::Flag {
                    field: Field::Npc,
                    mask: 0x800,
                    on: true,
                } => {
                    if matches!(
                        c.operation,
                        Operation::Flag {
                            field: Field::Critter,
                            mask: 0x80,
                            ..
                        }
                    ) {
                        dependencies.insert("sleep_and_noise");
                    }
                    if c.apply.iter().any(|a| a == "tgt_summoned") {
                        dependencies.insert("follower_ai_and_party_travel");
                    }
                }
                _ => {}
            }
        }
        dependencies
    }
    pub(crate) fn restores_summoned_art(&self) -> bool {
        self.components.iter().any(|c| {
            c.phase == Phase::Begin
                && c.apply.iter().any(|a| a == "tgt_summoned")
                && matches!(c.operation, Operation::Form(-1))
        })
    }
    pub fn decode(mes: &Mes, id: i32) -> Result<Self, String> {
        let base = 1000 + 50 * id;
        let mut p = Self {
            range: 99,
            ..Self::default()
        };
        let header = mes
            .get(base)
            .ok_or_else(|| format!("missing spell entry {base}"))?
            .to_ascii_lowercase();
        for (k, v) in clauses(&header) {
            match k {
                "aoe" => p.targets = words(v),
                "[begin]aoe" => p.targets = words(v),
                "radius" | "[begin]radius" => p.radius = num(v)?,
                "[end]aoe" | "[callback]aoe" | "[endcallback]aoe" => (),
                "[maintain]aoe" => p.maintain_targets = words(v),
                _ => return Err(format!("entry {base}: unsupported targeting {k}")),
            }
        }
        for offset in 1..50 {
            let Some(line) = mes.get(base + offset) else {
                continue;
            };
            let line = line.to_ascii_lowercase();
            if offset >= 5 {
                p.components.push(
                    Component::decode(&line)
                        .map_err(|e| format!("entry {}: {e}", base + offset))?,
                );
                continue;
            }
            for (k, v) in clauses(&line) {
                match k {
                    "cost" => p.cost = num::<u32>(v)?.try_into().map_err(|_| "cost overflow")?,
                    "range" => p.range = num(v)?,
                    "maintain" => {
                        let (cost, period) = pair(v)?;
                        let cost = num(cost)?;
                        let period = num::<u64>(period)?
                            .checked_mul(8)
                            .ok_or("upkeep period overflow")?;
                        if cost < 0 || period == 0 {
                            return Err("invalid upkeep".into());
                        }
                        p.maintain = Some((cost, period))
                    }
                    "duration" => {
                        let (period, stat) = pair(v)?;
                        if stat != "-1" {
                            return Err("stat-dependent duration".into());
                        }
                        p.duration = Some(
                            num::<u64>(period)?
                                .checked_mul(8)
                                .ok_or("duration overflow")?,
                        )
                    }
                    "durationtriggercount" => p.triggers = Some(num(v)?),
                    "resist" => {
                        let (s, n) = pair(v)?;
                        p.save = Some((
                            match s {
                                "stat_strength" => 0,
                                "stat_dexterity" => 1,
                                "stat_constitution" => 2,
                                "stat_willpower" => 6,
                                _ => return Err(format!("unknown resistance {s}")),
                            },
                            num(n)?,
                        ))
                    }
                    "missile" => p.missile = Some(num(v)?),
                    "no_stack" => p.no_stack = num::<u32>(v)? != 0,
                    "disallowed_sf" => p.disallowed_caster = mask(v, Field::Spell)?,
                    "disallowed_tsf" => p.disallowed_target = mask(v, Field::Spell)?,
                    "disallowed_tcf" => p.disallowed_critter = mask(v, Field::Critter)?,
                    "cancels_sf" => p.cancels = mask(v, Field::Spell)?,
                    "info" => p.aggressive = v == "aggressive",
                    "iq" | "chargebegincost" => (),
                    "no_reflect" => p.no_reflect = num::<u32>(v)? != 0,
                    k if k.starts_with("ai_") => (),
                    _ => {
                        return Err(format!(
                            "entry {}: unsupported parameter {k}",
                            base + offset
                        ));
                    }
                }
            }
        }
        if p.components.is_empty() || p.targets.is_empty() {
            return Err(format!("incomplete spell at {base}"));
        }
        Ok(p)
    }
}

impl Component {
    fn decode(line: &str) -> Result<Self, String> {
        let (prefix, opcode) = line.split_once("type:").ok_or("missing component type")?;
        let mut input = clauses(prefix);
        let phase = match input.next().map(|c| c.0) {
            Some("[begin]") => Phase::Begin,
            Some("[end]") => Phase::End,
            Some("[maintain]") => Phase::Maintain,
            Some("[callback]") => Phase::Callback,
            Some("[endcallback]") => Phase::EndCallback,
            _ => return Err("unknown phase".into()),
        };
        let mut targets = vec![];
        let mut apply = vec![];
        let mut spell_flags = 0;
        for (k, v) in input {
            match k {
                "aoe" => targets = words(v),
                "apply_aoe" => apply = words(v),
                "aoe_sf" => spell_flags = mask(v, Field::Spell)?,
                _ => return Err(format!("unknown component filter {k}")),
            }
        }
        let mut input = clauses(opcode);
        let (kind, _) = input.next().ok_or("missing opcode")?;
        let args: Vec<_> = input.collect();
        let at = |i| {
            args.get(i)
                .map(|&(s, _)| s)
                .ok_or_else(|| format!("missing {kind} operand"))
        };
        let get = |k| args.iter().find(|(key, _)| *key == k).map(|a| a.1);
        let integer = |k, default| get(k).map(num::<i32>).unwrap_or(Ok(default));
        let add = |i| match at(i)? {
            "add" => Ok(true),
            "remove" => Ok(false),
            s => Err(format!("invalid {kind} operand {s}")),
        };
        let palette = get("palette").map(num::<u8>).transpose()?;
        if palette.is_some_and(|p| p > 3) {
            return Err("invalid actor palette".into());
        }
        let operation = match kind {
            "effect" => Operation::Effect {
                id: num(at(0)?)?,
                add: add(1)?,
                count: integer("count", 1)?,
                scaled: integer("scaled", 0)?,
            },
            "eyecandy" => Operation::Eye {
                index: num(at(0)?)?,
                add: add(1)?,
                play: get("play").map(words).unwrap_or_default(),
            },
            "objflag" => {
                let (field, mask) = flag(at(1)?)?;
                let expected = match field {
                    Field::Spell => "spell_flags",
                    Field::Critter => "critter_flags",
                    Field::Npc => "npc_flags",
                    Field::Container => "container_flags",
                    Field::Portal => "portal_flags",
                    Field::Object => "flags",
                    Field::Item => "item_flags",
                };
                if at(0)?.strip_prefix("obj_f_") != Some(expected) {
                    return Err("incompatible flag field".into());
                }
                Operation::Flag {
                    field,
                    mask,
                    on: match at(2)? {
                        "flag_on" => true,
                        "flag_off" => false,
                        s => return Err(format!("invalid flag action {s}")),
                    },
                }
            }
            "damage" | "heal" => {
                let dmg = get("dmg").unwrap_or("0");
                let (a, b) = dmg.split_once('-').unwrap_or((dmg, dmg));
                let (min, max) = (num(a)?, num(b)?);
                if min < 0 || max < min {
                    return Err("invalid damage range".into());
                }
                Operation::Vital {
                    heal: kind == "heal",
                    channel: match get("dmgtype").unwrap_or("dmg_normal") {
                        "dmg_normal" => Channel::Normal,
                        "dmg_magic" => Channel::Magic,
                        "dmg_fire" => Channel::Fire,
                        "dmg_electrical" => Channel::Electrical,
                        "dmg_poison" => Channel::Poison,
                        "dmg_fatigue" => Channel::Fatigue,
                        s => return Err(format!("unknown damage type {s}")),
                    },
                    min,
                    max,
                    flags: get("dmg_flags").map(words).unwrap_or_default(),
                }
            }
            "trait" if at(0)? == "art_num" => Operation::Form(num(at(1)?)?),
            "summon" => Operation::Summon {
                prototype: num(get("proto").ok_or("missing summon prototype")?)?,
                list: get("list").map(num).transpose()?,
            },
            "destroy" if at(0)? == "summoned object" => Operation::Destroy,
            "dispel" => Operation::Dispel,
            "identify" if at(0)? == "if_display_all" => Operation::Identify,
            "movement" => {
                Operation::Move(get("move_location").ok_or("missing destination")?.into())
            }
            "agoal" => Operation::Goal(at(0)?.into()),
            _ => return Err(format!("unsupported component {kind}")),
        };
        Ok(Self {
            phase,
            targets,
            apply,
            palette,
            spell_flags,
            operation,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callbacks_and_owner_are_independent_of_spell_number() {
        let table = Mes::parse_text(concat!(
            "{1000}{AoE: Tgt_None, [Begin]AoE: Tgt_Tile_Empty}\n",
            "{1001}{Cost: 7, Maintain: (2 @ 3)}\n",
            "{1005}{[Begin], Type: Summon, Proto: 55}\n",
            "{1006}{[Begin], Apply_AoE: Tgt_Summoned, Type: EyeCandy, 4, Add, Play: Callback}\n",
            "{1007}{[Begin], Apply_AoE: Tgt_Summoned, Type: Trait, art_num, -1}\n",
            "{1008}{[End], Type: EyeCandy, 4, Add, Play: Reverse | End_Callback}\n",
            "{1009}{[EndCallback], AoE: Tgt_Summoned, Type: Destroy, Summoned Object}"
        ))
        .unwrap();
        let p = Program::decode(&table, 0).unwrap();
        assert_eq!(p.targets, ["tgt_tile_empty"]);
        assert_eq!(p.maintain, Some((2, 24)));
        assert!(p.restores_summoned_art());
        assert_eq!(p.components[1].apply, ["tgt_summoned"]);
        assert!(
            matches!(&p.components[3].operation, Operation::Eye { index:4, play, .. } if play == &["reverse", "end_callback"])
        );
        assert_eq!(p.components[4].phase, Phase::EndCallback);
        assert!(matches!(p.components[4].operation, Operation::Destroy));
    }
}
