//! Leader relationships used by original dialogue, SCR, movement and combat.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::content) struct Follower {
    #[serde(default)]
    pub leader_level: i32,
    pub name: u32,
    pub forced: bool,
    pub waiting: bool,
    #[serde(flatten)]
    pub orders: FollowerOrders,
    #[serde(default)]
    pub dead: bool,
}

/// Formation and targeting persist with whichever relationship grants control.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(in crate::content) struct FollowerOrders {
    pub spread: bool,
    #[serde(default)]
    pub destination: Option<(u32, u32)>,
    #[serde(default)]
    pub attack: Option<crate::sector::objects::OriginalId>,
    #[serde(default)]
    pub back_off: bool,
}

impl FollowerOrders {
    pub fn clear_target(&mut self) {
        self.destination = None;
        self.attack = None;
        self.back_off = false;
    }

    pub fn set_spacing(&mut self, spread: bool) {
        self.clear_target();
        self.spread = spread;
    }

    pub fn route(
        &self,
        at: (u32, u32),
        leader: (u32, u32),
        distance: u32,
    ) -> Option<((u32, u32), bool)> {
        if let Some(destination) = self.destination {
            return (destination != at).then_some((destination, false));
        }
        let (near, far) = if self.spread { (3, 5) } else { (1, 2) };
        (distance < near || distance > far).then_some((leader, distance < near))
    }

    pub fn idle(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Copy, Debug)]
pub(in crate::content) enum Order {
    Join(bool),
    Resume(bool),
    Wait,
    Leave,
    Spread(bool),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::content) struct Candidate {
    pub name: u32,
    pub level: i32,
    pub alignment: i32,
    pub flags: u32,
    pub packet: Option<[i32; 17]>,
    pub other_leader: bool,
}

/// CE ai_check_follow applies the same relationship thresholds during recruitment and while following.
pub(in crate::content) fn eligibility(
    packet: &[i32; 17],
    reaction: i32,
    levels: [i32; 2],
    alignments: [i32; 2],
) -> Result<(), &'static str> {
    if reaction <= packet[5] {
        return Err("NPC dislikes the player");
    }
    let difference = i64::from(alignments[0]) - i64::from(alignments[1]);
    if difference > i64::from(packet[6]) || -difference > i64::from(packet[7]) {
        return Err("NPC refuses the player's alignment");
    }
    if i64::from(levels[1]) >= i64::from(levels[0]) + i64::from(packet[8]) {
        return Err("Player level is too low for this follower");
    }
    Ok(())
}
impl State {
    pub(in crate::content) fn following(&self, key: &str) -> bool {
        self.followers
            .get(key)
            .is_some_and(|f| !f.waiting && !f.dead)
    }

    pub(in crate::content) fn follower_count(&self) -> usize {
        self.followers
            .values()
            .filter(|f| !f.waiting && !f.dead && !f.forced)
            .count()
    }

    pub(in crate::content) fn order(
        &mut self,
        key: &str,
        order: Order,
        scripted: bool,
    ) -> Result<(), String> {
        match order {
            Order::Join(force) | Order::Resume(force) => {
                let facts = self
                    .interaction
                    .as_ref()
                    .ok_or("Follower eligibility unavailable")?;
                let npc = facts.candidate.ok_or("Missing follower NPC")?;
                let existing = self.followers.get(key);
                if matches!(order, Order::Resume(_)) && existing.is_none_or(|f| !f.waiting) {
                    return Err("NPC is not waiting for the player".into());
                }
                if !scripted && facts.skills[11].training < 3 && npc.flags & 0x400 == 0 {
                    let params = npc.packet.ok_or("Missing NPC AI packet")?;
                    if npc.other_leader {
                        return Err("NPC already follows another leader".into());
                    }
                    eligibility(
                        &params,
                        facts.reaction.ok_or("Unknown NPC reaction")?,
                        [facts.stats[17].ok_or("Missing player level")?, npc.level],
                        [facts.stats[19].ok_or("Missing alignment")?, npc.alignment],
                    )?;
                    if !force
                        && self.follower_count() >= facts.stats[15].unwrap_or(0).max(0) as usize
                    {
                        return Err("Player has reached the follower limit".into());
                    }
                }
                self.jilted.remove(key);
                self.followers.insert(
                    key.to_owned(),
                    Follower {
                        dead: false,
                        leader_level: facts.stats[17].unwrap_or(1),
                        name: npc.name,
                        forced: force || npc.flags & 0x400 != 0,
                        waiting: false,
                        orders: FollowerOrders {
                            spread: existing.map_or(npc.flags & 0x10 != 0, |f| f.orders.spread),
                            ..Default::default()
                        },
                    },
                );
            }
            Order::Wait => {
                if let Some(f) = self.followers.get_mut(key) {
                    f.waiting = true;
                    f.orders.clear_target();
                }
            }
            Order::Spread(spread) => {
                if let Some(f) = self.followers.get_mut(key) {
                    f.orders.set_spacing(spread);
                }
            }
            Order::Leave => {
                self.followers.remove(key);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn party() -> State {
        let mut facts = Interaction::default();
        facts.stats[15] = Some(1);
        facts.stats[17] = Some(2);
        facts.stats[19] = Some(0);
        facts.reaction = Some(50);
        let mut packet = [0; 17];
        packet[5..9].copy_from_slice(&[20, 100, 100, 2]);
        facts.candidate = Some(Candidate {
            name: 7,
            level: 1,
            alignment: 0,
            flags: 0,
            packet: Some(packet),
            other_leader: false,
        });
        State {
            interaction: Some(facts),
            ..Default::default()
        }
    }

    #[test]
    fn waiting_retains_the_leader_and_rejoining_rechecks_available_capacity() {
        let mut p = party();
        p.order("first", Order::Join(false), false).unwrap();
        let before = p.clone();
        assert!(p.order("second", Order::Join(false), false).is_err());
        assert_eq!(before, p);
        p.order("first", Order::Wait, false).unwrap();
        p.order("second", Order::Join(false), false).unwrap();
        assert!(p.order("first", Order::Resume(false), false).is_err());
        assert!(p.followers["first"].waiting);
        p.order("second", Order::Leave, true).unwrap();
        p.order("first", Order::Resume(false), false).unwrap();
        assert!(p.following("first"));
        assert_eq!(p.follower_count(), 1);
        p.order("scripted", Order::Join(true), true).unwrap();
        assert_eq!(p.follower_count(), 1);
    }

    #[test]
    fn original_ai_packet_controls_reaction_alignment_and_level_eligibility() {
        let mut p = party();
        for (reaction, alignment, level) in [(20, 0, 1), (21, -101, 1), (21, 101, 1), (21, 0, 4)] {
            let facts = p.interaction.as_mut().unwrap();
            facts.reaction = Some(reaction);
            let npc = facts.candidate.as_mut().unwrap();
            npc.alignment = alignment;
            npc.level = level;
            assert!(p.order("candidate", Order::Join(false), false).is_err());
            assert!(p.followers.is_empty());
        }
        p.interaction.as_mut().unwrap().skills[11].training = 3;
        p.order("candidate", Order::Join(false), false).unwrap();
        assert!(p.following("candidate"));
    }
}
