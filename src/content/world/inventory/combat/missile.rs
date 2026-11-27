use super::resolution::Weapon;
use super::*;

/// Ammunition and AP belong to release. Carry the weapon roll parameters through a save
/// so that changing equipment between release and impact cannot change the shot.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Shot {
    pub(super) from: Actor,
    pub(super) to: Actor,
    origin: Tile,
    destination: Tile,
    weapon: Weapon,
    ammo: u32,
    remaining: u64,
    duration: u64,
    #[serde(skip)]
    visible: bool,
}
impl Shot {
    pub(super) fn valid(&self) -> bool {
        self.from != self.to
            && self.remaining <= self.duration
            && self.duration > 0
            && self.weapon.valid()
            && self.weapon.missile.is_some()
    }
}
impl InventoryReader {
    pub(super) fn release(
        &mut self,
        w: &mut InteractiveWorld,
        attacker: &Target,
        target: &Target,
        weapon: Weapon,
        ammo: u32,
        now: Duration,
    ) {
        let id = |t: &Target| {
            if let Subject::Mobile(id) = t.subject {
                Some(id)
            } else {
                None
            }
        };
        let duration = u64::from(distance(attacker.tile, target.tile)).max(1) * 35;
        let mut shot = Shot {
            from: id(attacker),
            to: id(target),
            origin: attacker.tile,
            destination: target.tile,
            weapon,
            ammo,
            remaining: duration,
            duration,
            visible: false,
        };
        self.show_shot(w, &mut shot, now);
        let combat = &mut w.cache.inventory.combat;
        combat.log(json!({"event":"projectile_released","attacker":attacker.subject,"target":target.subject,"ammo":ammo,"duration_ms":duration,"art":shot.weapon.missile}));
        combat.shots.push(shot);
    }
    fn show_shot(&mut self, w: &mut InteractiveWorld, shot: &mut Shot, now: Duration) {
        let result = (|| {
            let art = crate::content::scenery::SceneryArt::decode(shot.weapon.missile.unwrap())?;
            let names = load_mes(&mut self.vfs, "art/scenery/scenery.mes")?;
            let path = art.resource_path(&names.mes)?;
            let dir = npc::facing(shot.origin, shot.destination).unwrap_or(0);
            let load = |vfs: &mut Vfs, cache: &mut FrameCache, i| {
                cache.load_rotated(vfs, &path, i, art.palette, false, dir)
            };
            let first = load(&mut self.vfs, &mut self.frames, 0)?;
            let count = first.decoded.frame_count;
            let fps = first.decoded.frames_per_second;
            let mut frames = vec![first];
            for i in 1..count {
                frames.push(load(&mut self.vfs, &mut self.frames, i)?)
            }
            let clip = FrameLoop::timed(frames, fps.max(1) as u32)?;
            let origin = project_world_tile(shot.origin, w.reference)
                .ok_or("Projectile origin outside map")?;
            let dest = project_world_tile(shot.destination, w.reference)
                .ok_or("Projectile destination outside map")?;
            // Resume a saved flight at the same fraction along its path.
            let f = 1. - shot.remaining as f64 / shot.duration as f64;
            let interpolate = |a: i32, b: i32| a + ((b - a) as f64 * f) as i32;
            let at = (interpolate(origin.0, dest.0), interpolate(origin.1, dest.1));
            w.cache.weapon_missile(
                shot.origin,
                [at, dest],
                dir,
                now,
                Duration::from_millis(shot.remaining.max(1)),
                clip,
            );
            Ok::<(), Box<dyn std::error::Error>>(())
        })();
        shot.visible = true;
        if let Err(e) = result {
            w.cache
                .inventory
                .combat
                .log(json!({"event":"projectile_art_unavailable","reason":e.to_string()}))
        }
    }
    pub(super) fn advance_shots(
        &mut self,
        w: &mut InteractiveWorld,
        dt: u64,
        now: Duration,
    ) -> bool {
        if w.cache.inventory.combat.shots.is_empty() {
            return false;
        }
        for mut shot in std::mem::take(&mut w.cache.inventory.combat.shots) {
            shot.remaining = shot.remaining.saturating_sub(dt);
            if shot.remaining != 0 {
                if !shot.visible {
                    self.show_shot(w, &mut shot, now)
                }
                w.cache.inventory.combat.shots.push(shot);
                continue;
            }
            let result = self.fighter(w, shot.from).and_then(|mut a| {
                let mut t = self.fighter(w, shot.to)?;
                if t.hp >= t.maximum {
                    return Err("Projectile target already dead".into());
                }
                if !reachable(w, shot.origin, t.tile, shot.weapon.range) {
                    return Err("Projectile lost line of sight or range".into());
                }
                let inv = &mut w.cache.inventory;
                let event = inv.strike(&mut a, &mut t, &shot.weapon, shot.ammo);
                self.attack_feedback(w, &a, &t, &shot.weapon, event, now);
                Ok(())
            });
            if let Err(e) = result {
                w.cache
                    .inventory
                    .combat
                    .log(json!({"event":"projectile_missed","reason":e}))
            }
        }
        true
    }
}
