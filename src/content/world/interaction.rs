//! Prepared interactive objects and bounded session-local door state.

use super::super::script::{Actor, Attachment, Event, Invocation, Outcome, Script, State};
use super::*;
use crate::compositor::scaled_sprite_rect;

type Tile = (u32, u32);
type Id = (u64, usize);
pub(super) type Handler = Option<Result<Box<(Attachment, Script)>, String>>;

pub(super) fn handler(
    vfs: &mut Vfs,
    owner: &str,
    event: Event,
    id: i32,
    initial: super::super::script::Local,
) -> Handler {
    (id > 0).then(|| {
        let script = Script::load(vfs, id)?;
        Ok(Box::new((
            Attachment {
                owner: owner.to_owned(),
                event,
                script: id,
                initial,
            },
            script,
        )))
    })
}

pub(super) fn check_handler(
    handler: &Handler,
    hour: u8,
    scripts: &State,
    at: Tile,
    from: Option<Tile>,
) -> Result<(), String> {
    let Some(handler) = handler else {
        return Ok(());
    };
    let (attachment, script) = handler.as_deref().map_err(Clone::clone)?;
    let input = invocation(attachment, hour, at, from);
    let result = script.inspect(&input, scripts).map_err(|fault| {
        format!(
            "{:?} {} from {}: {fault}",
            attachment.event, script.source.virtual_path, script.source
        )
    })?;
    if result.outcome != (Outcome::Return { run_default: true }) {
        return Err(format!(
            "{:?} {} disallows opening at statement {}",
            attachment.event, script.source.virtual_path, result.statement
        ));
    }
    Ok(())
}

fn invocation(attachment: &Attachment, hour: u8, tile: Tile, from: Option<Tile>) -> Invocation<'_> {
    Invocation {
        attachment,
        triggerer: Actor::Player,
        position: 0,
        hour,
        follows_player: None,
        npc_flags: None,
        locations: [from, Some(tile)],
        met: None,
    }
}

pub(super) enum Action {
    Travel(Destination),
    Door(Box<Door>),
    Window {
        placement: ((i32, i32), u8),
        id: Id,
        flags: u32,
        difficulty: i32,
        notify: i32,
    },
    Container(crate::sector::objects::OriginalId),
    Pickup(crate::sector::objects::OriginalId),
    Summon(crate::sector::objects::OriginalId),
    Npc(crate::sector::objects::OriginalId),
    Unsupported(String),
}

impl Action {
    pub(super) fn portal_placement(&self) -> Option<((i32, i32), u8)> {
        match self {
            Self::Door(door) => Some((door.offset, door.rotation)),
            Self::Window { placement, .. } => Some(*placement),
            _ => None,
        }
    }

    pub(super) fn destination(&self) -> Option<Destination> {
        match self {
            Self::Travel(dst) => Some(*dst),
            Self::Door(door) => door.destination,
            _ => None,
        }
    }
}

pub(super) struct Target {
    pub tile: Tile,
    pub action: Action,
}

pub(super) struct Door {
    pub id: Id,
    tile: Tile,
    pub originally_open: bool,
    pub direction: u8,
    pub(super) flags: u32,
    sound_effect: Option<i32>,
    unlock_script: i32,
    unlock: Handler,
    pub(super) notify: i32,
    pub(super) difficulty: i32,
    pub(super) key_id: i32,
    rotation: u8,
    offset: (i32, i32),
    use_script: i32,
    on_use: Handler,
    destination: Option<Destination>,
    frames: [(Arc<LoadedFrame>, (i32, i32), usize); 4],
    period: Duration,
}

#[derive(Clone, Copy)]
struct Change {
    open: bool,
    started: Option<Duration>,
}

/// Keyed by original sector and record index; never evicted with raster data.
#[derive(Clone, Default)]
pub struct DoorStates {
    poses: BTreeMap<Id, Change>,
    developer_unlocked: BTreeSet<Id>,
    pub(super) summons: BTreeSet<Tile>,
    pub(super) vacated: BTreeMap<Tile, u32>,
    pub(super) spell_holds: BTreeSet<abilities::LockId>,
    pub(super) locks: BTreeMap<abilities::LockId, abilities::LockChange>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct SavedDoors {
    changed: Vec<(Id, bool)>,
    unlocked: Vec<Id>,
    #[serde(default)]
    locks: Vec<(abilities::LockId, abilities::LockChange)>,
}

impl SavedDoors {
    pub fn validate(&self, vfs: &mut Vfs, map: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut sectors: BTreeMap<u64, BTreeSet<usize>> = BTreeMap::new();
        let mut containers = None;
        for (id, _) in &self.locks {
            if let abilities::LockId::Container(id) = id {
                let index =
                    containers.get_or_insert_with(|| mobile::population::Index::load(vfs, map));
                if index.category(*id) != Some(2) {
                    return Err("Saved lock container is unavailable".into());
                }
            }
        }
        let portals = self.locks.iter().filter_map(|(id, _)| match id {
            abilities::LockId::Door(id) => Some(id),
            _ => None,
        });
        for &(sector, record) in self
            .changed
            .iter()
            .map(|(id, _)| id)
            .chain(&self.unlocked)
            .chain(portals)
        {
            if let std::collections::btree_map::Entry::Vacant(entry) = sectors.entry(sector) {
                let coordinates =
                    Coordinates::from_id(sector).ok_or("Invalid saved door sector")?;
                let (_, bytes) = super::super::sector::read_base_sector(vfs, map, coordinates)?;
                let objects = crate::sector::objects::decode(&bytes)?;
                entry.insert(objects.portals.into_iter().map(|door| door.index).collect());
            }
            if !sectors[&sector].contains(&record) {
                return Err(format!("Saved door {map}/{sector}:{record} is unavailable").into());
            }
        }
        Ok(())
    }
}

impl DoorStates {
    pub fn for_save(&self) -> SavedDoors {
        SavedDoors {
            changed: self
                .poses
                .iter()
                .map(|(&id, state)| (id, state.open))
                .collect(),
            locks: self
                .locks
                .iter()
                .map(|(id, lock)| (*id, lock.clone()))
                .collect(),
            unlocked: self.developer_unlocked.iter().copied().collect(),
        }
    }

    pub fn from_save(saved: &SavedDoors) -> Result<Self, String> {
        let changed: BTreeMap<_, _> = saved
            .changed
            .iter()
            .map(|&(id, open)| {
                (
                    id,
                    Change {
                        open,
                        started: None,
                    },
                )
            })
            .collect();
        let unlocked: BTreeSet<_> = saved.unlocked.iter().copied().collect();
        if changed.len() != saved.changed.len() || unlocked.len() != saved.unlocked.len() {
            return Err("Duplicate door identities in save".into());
        }
        let locks: BTreeMap<_, _> = saved.locks.iter().cloned().collect();
        if locks.len() != saved.locks.len() {
            return Err("Duplicate saved lock".into());
        }
        Ok(Self {
            summons: BTreeSet::new(),
            vacated: BTreeMap::new(),
            spell_holds: BTreeSet::new(),
            poses: changed,
            developer_unlocked: unlocked,
            locks,
        })
    }
}

impl DoorStates {
    pub(super) fn flags(&self, id: abilities::LockId, original: u32) -> u32 {
        self.locks
            .get(&id)
            .map_or(original, |change| change.flags(original))
            | if self.spell_holds.contains(&id) { 4 } else { 0 }
    }

    pub(super) fn expire_locks(&mut self, now: u64) -> bool {
        let mut closed = false;
        for (id, change) in &mut self.locks {
            if change.expire(now) {
                if let abilities::LockId::Door(id) = id {
                    self.poses.insert(
                        *id,
                        Change {
                            open: false,
                            started: None,
                        },
                    );
                }
                closed = true;
            }
        }
        closed
    }

    pub(super) fn is_open(&self, door: &Door) -> bool {
        self.poses
            .get(&door.id)
            .map_or(door.originally_open, |change| change.open)
    }

    pub(super) fn diagnostic(&self, door: &Door, cache: &SectorCache) -> serde_json::Value {
        let open = self.is_open(door);
        let id = abilities::LockId::Door(door.id);
        let flags = self.flags(id, door.flags);
        serde_json::json!({"difficulty":door.difficulty, "effective_flags":flags,"lock_change":self.locks.get(&id), "kind":"door","open":open,"flags":door.flags,
            "locked":!self.developer_unlocked.contains(&door.id)&&locked(flags,cache.hour),
            "developer_unlock":self.developer_unlocked.contains(&door.id),"unlock_script":door.unlock_script,
            "key_id":door.key_id,"matching_key":cache.inventory.matching_key(door.key_id).map(|id| id.key()),
            "unlock_check":check_handler(&door.unlock, cache.hour, &cache.scripts, door.tile, cache.player.as_ref().map(|p|p.tile())).err(),
            "destination":door.destination,"use_script":door.use_script,"use_source":door.on_use.as_ref().and_then(|h|h.as_ref().ok()).map(|h|super::super::diagnostics::source(&h.1.source)), "use_issue":door.on_use.as_ref().and_then(|h|h.as_ref().err())})
    }

    pub(super) fn unlock(&mut self, door: &Door) -> Result<bool, String> {
        if door.unlock_script > 0
            || self.flags(abilities::LockId::Door(door.id), door.flags) & 6 != 0
        {
            return Err("Door has a scripted, jammed or magical restriction".into());
        }
        Ok(self.developer_unlocked.insert(door.id))
    }
}

impl DoorStates {
    pub(super) fn cell(
        &self,
        sector: &CachedSector,
        tile: Tile,
        base: collision::Cell,
    ) -> collision::Cell {
        let base = base.vacate(self.vacated.get(&tile).copied().unwrap_or(0));
        let mut cell = if self.summons.contains(&tile) {
            base.occupied()
        } else {
            base
        };
        for key in sector.doors_at.get(&tile).into_iter().flatten() {
            let Action::Door(door) = &sector.interactions[key].action else {
                continue;
            };
            if let Some(change) = self.poses.get(&door.id) {
                cell = cell.portal_state(door.direction, door.originally_open, change.open);
            }
        }
        cell
    }

    fn change(
        &mut self,
        door: &Door,
        tick: Duration,
        hour: u8,
        access: bool,
        from: Tile,
        context: (&mut State, &mut super::inventory::Session),
    ) -> Result<bool, String> {
        let (scripts, inventory) = context;
        if self
            .poses
            .get(&door.id)
            .is_some_and(|change| door.delay(*change, tick).is_some())
        {
            return Err("door is moving".into());
        }
        let open = !self.is_open(door);
        if open {
            door.may_open(
                self,
                hour,
                self.developer_unlocked.contains(&door.id) || access,
                scripts,
                from,
            )?;
        }
        if let Some(handler) = &door.on_use {
            let (attachment, script) = handler.as_deref().map_err(Clone::clone)?;
            let mut staged = scripts.clone();
            let result = script
                .execute(
                    &invocation(attachment, hour, door.tile, Some(from)),
                    &mut staged,
                )
                .map_err(|error| {
                    format!(
                        "Use {} from {}: {error}",
                        script.source.virtual_path, script.source
                    )
                })?;
            if result.outcome != (Outcome::Return { run_default: true }) {
                return Err(format!(
                    "Use {} disallows the default action at statement {}",
                    script.source.virtual_path, result.statement
                ));
            }
            inventory.apply_quest_rewards(&staged.quests, &scripts.quests)?;
            *scripts = staged;
        }
        self.poses.insert(
            door.id,
            Change {
                open,
                started: Some(tick),
            },
        );
        Ok(open)
    }
}

pub(super) fn check_lock(
    object: &str,
    flags: u32,
    hour: u8,
    authorized: bool,
) -> Result<(), String> {
    if flags & 2 != 0 {
        return Err(format!("{object} is jammed"));
    }
    if flags & 4 != 0 {
        return Err(format!("{object} is magically held"));
    }
    if !authorized && locked(flags, hour) {
        return Err(format!("{object} is locked"));
    }
    Ok(())
}

pub(super) fn locked(flags: u32, hour: u8) -> bool {
    // CE object_locked_get. ALWAYS_LOCKED gates the indoor egress exception
    // in ai_attempt_open_portal, rather than replacing OPF_LOCKED.
    if flags & (8 | 0x80) != 0 {
        return false;
    } // NEVER_LOCKED
    let day = (7..=21).contains(&hour);
    flags & 1 != 0 && (flags & 0x20 == 0 || day) && (flags & 0x40 == 0 || !day)
}

impl Door {
    pub(super) fn direct_transition(&self) -> bool {
        self.destination.is_some() && self.unlock_script == 0 && self.flags & 7 == 0
    }

    pub(super) fn within_reach(&self, player: Tile) -> bool {
        if self.direct_transition() {
            self.tile.0.abs_diff(player.0) + self.tile.1.abs_diff(player.1) <= 1
        } else {
            self.tile == player || neighbor(self.tile, self.direction) == Some(player)
        }
    }

    fn may_open(
        &self,
        states: &DoorStates,
        hour: u8,
        unlocked: bool,
        scripts: &State,
        from: Tile,
    ) -> Result<(), String> {
        check_handler(&self.unlock, hour, scripts, self.tile, Some(from))?;
        check_lock(
            "door",
            states.flags(abilities::LockId::Door(self.id), self.flags),
            hour,
            unlocked,
        )?;
        if let Some(Err(issue)) = &self.on_use {
            return Err(issue.clone());
        }
        Ok(())
    }

    fn index(&self, change: Change, tick: Duration) -> usize {
        let Some(started) = change.started else {
            return if change.open { 3 } else { 0 };
        };
        let step =
            (tick.saturating_sub(started).as_millis() / self.period.as_millis()).min(2) as usize;
        if change.open { step + 1 } else { 2 - step }
    }

    fn delay(&self, change: Change, tick: Duration) -> Option<Duration> {
        let n = tick.saturating_sub(change.started?).as_millis();
        let p = self.period.as_millis();
        (n < 2 * p).then(|| Duration::from_millis((p - n % p) as u64))
    }

    fn pose(&self, states: &DoorStates, tick: Duration) -> usize {
        states
            .poses
            .get(&self.id)
            .map_or(usize::from(self.originally_open) * 3, |change| {
                self.index(*change, tick)
            })
    }

    pub(super) fn art_frame(&self, states: &DoorStates, tick: Duration) -> usize {
        self.frames[self.pose(states, tick)].2
    }

    pub(super) fn draw<'a>(
        &'a self,
        states: &DoorStates,
        tick: Duration,
    ) -> (&'a Arc<LoadedFrame>, SpriteDraw<'a>) {
        let (asset, anchor, _) = &self.frames[self.pose(states, tick)];
        (
            asset,
            SpriteDraw {
                frame: &asset.decoded.frame,
                anchor: *anchor,
            },
        )
    }
}

pub(super) fn prepare(
    vfs: &mut Vfs,
    frames: &mut FrameCache,
    room: &super::super::scenery::Room,
    map: &str,
    sector: u64,
    reference: Tile,
) -> BTreeMap<WorldOrder, Target> {
    let mut targets = BTreeMap::new();
    for portal in &room.portals {
        let tile = portal.placement.tile;
        if portal.art.kind == super::super::portal::Kind::Window {
            targets.insert(
                (true, portal.order()),
                Target {
                    tile,
                    action: Action::Window {
                        placement: (portal.placement.pixel_offset, portal.art.rotation),
                        id: (sector, portal.record.index),
                        flags: portal.appearance.portal_flags,
                        difficulty: portal.difficulty,
                        notify: portal.notify,
                    },
                },
            );
            continue;
        }
        let owner = format!("{map}/{sector}:{}", portal.record.index);
        let use_handler = handler(vfs, &owner, Event::Use, portal.use_script, portal.use_local);
        let destination = use_handler
            .as_ref()
            .and_then(|handler| handler.as_ref().ok())
            .and_then(|handler| super::super::jumps::decode_use(handler.1.code()).ok());
        let direct_transition = destination.is_some()
            && portal.unlock_script == 0
            && portal.appearance.portal_flags & 7 == 0;
        let action = {
            let build = || -> Result<_, Box<dyn std::error::Error>> {
                let art = portal.art;
                let first = usize::from(art.first_opening_frame());
                let mut sequence = Vec::with_capacity(4);
                let indices = if direct_transition {
                    [usize::from(art.frame); 4]
                } else {
                    [0, first, first + 1, first + 2]
                };
                for index in indices {
                    let asset = frames.load(
                        vfs,
                        &portal.artwork.source.virtual_path,
                        index,
                        art.palette,
                        art.mirror_x,
                    )?;
                    let frame = &asset.decoded.frame;
                    if frame.width > 512 || frame.height > 512 {
                        return Err("animated door exceeds the supported dimensions".into());
                    }
                    let anchor = portal
                        .placement
                        .draw_with_pivot(frame, reference, art.pivot(frame))
                        .ok_or("door pivot overflow")?
                        .anchor;
                    sequence.push((asset, anchor, index));
                }
                let fps = sequence[0].0.decoded.frames_per_second;
                let period = Duration::from_millis((1000 / fps.max(1)).clamp(16, 800) as u64);
                Ok(Door {
                    id: (sector, portal.record.index),
                    tile,
                    direction: art.rotation | 1,
                    originally_open: art.frame != 0,
                    flags: portal.appearance.portal_flags,
                    sound_effect: portal.sound_effect,
                    unlock_script: portal.unlock_script,
                    notify: portal.notify,
                    difficulty: portal.difficulty,
                    key_id: portal.key_id,
                    rotation: art.rotation,
                    offset: portal.placement.pixel_offset,
                    unlock: handler(
                        vfs,
                        &owner,
                        Event::Unlock,
                        portal.unlock_script,
                        portal.unlock_local,
                    ),
                    use_script: portal.use_script,
                    on_use: if destination.is_none() {
                        use_handler
                    } else {
                        None
                    },
                    destination,
                    frames: sequence.try_into().expect("four door poses"),
                    period,
                })
            };
            match build() {
                Ok(door) => Action::Door(Box::new(door)),
                Err(error) => Action::Unsupported(error.to_string()),
            }
        };
        targets.insert((true, portal.order()), Target { tile, action });
    }
    for object in &room.scenery {
        if object.use_script <= 0 {
            continue;
        }
        targets.insert(
            (true, object.order()),
            Target {
                tile: object.tile,
                action: use_action(vfs, object.use_script),
            },
        );
    }
    targets
}

fn use_action(vfs: &mut Vfs, script: i32) -> Action {
    match super::super::jumps::use_destination(vfs, script) {
        Ok(destination) => Action::Travel(destination),
        Err(error) => Action::Unsupported(format!("Use script {script}: {error}")),
    }
}

impl SectorCache {
    /// The planning edge may open a single portal. Movement still uses
    /// the real collision grid and never executes that edge while it is closed.
    fn planned_edge(
        &self,
        tile: Tile,
        direction: u8,
    ) -> Option<(Tile, Option<(WorldOrder, &Door)>)> {
        let cell = |tile| Self::player_cell(&self.entries, &self.door_states, tile);
        if let Some(next) = collision::step(tile, direction, cell) {
            return Some((next, None));
        }
        if direction & 1 == 0 {
            return None;
        }
        let next = neighbor(tile, direction)?;
        for (door_tile, edge) in [(tile, direction), (next, (direction + 4) % 8)] {
            let (sector, _) = self.entries.get(&tile_sector_id(door_tile))?;
            for &key in sector.doors_at.get(&door_tile).into_iter().flatten() {
                let Action::Door(door) = &sector.interactions[&key].action else {
                    continue;
                };
                if door.direction == edge
                    && !self.door_states.is_open(door)
                    && collision::step(tile, direction, |at| {
                        let current = cell(at)?;
                        Some(if at == door_tile {
                            current.portal_state(door.direction, false, true)
                        } else {
                            current
                        })
                    })
                    .is_some()
                {
                    return Some((next, Some((key, door))));
                }
            }
        }
        None
    }

    fn key_or_egress(&self, door: &Door, from: Tile) -> bool {
        if self.inventory.matching_key(door.key_id).is_some() {
            return true;
        }
        // CE ai_is_indoor_to_outdoor_transition tests the leaf's floor and its rotation neighbor.
        // The two interaction tiles are the leaf itself (location_rot returns 5
        // for coincident tiles) and the neighbor facing back across its edge.
        if door.flags & 0x10 != 0 || from != door.tile || matches!(door.rotation, 0..=2) {
            return false;
        }
        self.outdoor_at(door.tile) == Some(false)
            && neighbor(door.tile, door.rotation).and_then(|tile| self.outdoor_at(tile))
                == Some(true)
    }

    pub(super) fn plan_route(
        &mut self,
        goal: Tile,
        run: bool,
    ) -> Result<(usize, Option<WorldOrder>), String> {
        if self.player_spell_flags() & 0x01000000 != 0 {
            return Err("Movement prevented by Entangle".into());
        }
        let origin = self
            .player
            .as_ref()
            .ok_or("no active player")?
            .route_origin();
        if self.collision_disabled {
            if !Self::available_tile(&self.entries, goal) {
                return Err("destination sector is unavailable".into());
            }
            let route = super::super::player::path::find_with(origin, goal, |tile, direction| {
                Self::unrestricted_step(&self.entries, tile, direction).map(|next| (next, 0))
            })
            .ok_or("destination is beyond the loaded route")?;
            let count = route.len();
            self.player
                .as_mut()
                .expect("active player")
                .follow_route(route, run);
            return Ok((count, None));
        }
        let find = |allow_locked: bool| {
            super::super::player::path::find_with(origin, goal, |tile, direction| {
                let (next, door) = self.planned_edge(tile, direction)?;
                if !allow_locked && let Some((_, door)) = door {
                    door.may_open(
                        &self.door_states,
                        self.hour,
                        self.door_states.developer_unlocked.contains(&door.id)
                            || self.key_or_egress(door, tile),
                        &self.scripts,
                        tile,
                    )
                    .ok()?;
                }
                Some((next, if door.is_some() { 20 } else { 0 }))
            })
        };
        if let Some(mut route) = find(false) {
            let count = route.len();
            let mut tile = origin;
            let mut pending = None;
            for (index, &direction) in route.iter().enumerate() {
                let (next, door) = self.planned_edge(tile, direction).expect("planned edge");
                if let Some((key, _)) = door {
                    pending = Some(key);
                    route.truncate(index);
                    break;
                }
                tile = next;
            }
            self.player
                .as_mut()
                .expect("active player")
                .follow_route(route, run);
            return Ok((count, pending));
        }
        let mut tile = origin;
        for direction in find(true).into_iter().flatten() {
            let (next, door) = self.planned_edge(tile, direction).expect("planned edge");
            if let Some((_, door)) = door {
                door.may_open(
                    &self.door_states,
                    self.hour,
                    self.door_states.developer_unlocked.contains(&door.id)
                        || self.key_or_egress(door, tile),
                    &self.scripts,
                    tile,
                )?;
            }
            tile = next;
        }
        Err("cannot reach the destination".into())
    }

    pub(super) fn door_opening_delay(&self, key: WorldOrder, tick: Duration) -> Option<Duration> {
        let Action::Door(door) = &self.target(key)?.action else {
            return None;
        };
        let change = self.door_states.poses.get(&door.id)?;
        (change.started? + 2 * door.period).checked_sub(tick)
    }
    pub(super) fn target(&self, key: WorldOrder) -> Option<&Target> {
        if let Some(target) = self.dropped_targets.get(&key) {
            return Some(target);
        }
        let tile = (key.1.2, key.1.1);
        self.entries
            .get(&tile_sector_id(tile))?
            .0
            .interactions
            .get(&key)
    }

    pub(super) fn player_cell(
        entries: &BTreeMap<u64, (CachedSector, Option<u64>)>,
        states: &DoorStates,
        tile: Tile,
    ) -> Option<collision::Cell> {
        let (sector, _) = entries.get(&tile_sector_id(tile))?;
        Some(states.cell(sector, tile, sector.collision.as_ref()?.cell(tile)))
    }

    pub(super) fn interaction_frame<'a>(
        &'a self,
        raster: &Raster,
        elapsed: Duration,
    ) -> Option<(&'a Arc<LoadedFrame>, SpriteDraw<'a>)> {
        if let Some(Action::Npc(id) | Action::Summon(id)) =
            self.target(raster.key).map(|t| &t.action)
            && let Some((frame, offset)) = self.inventory.combat.sprite(Some(*id), elapsed)
        {
            let mut draw = raster.frame_draw(frame);
            draw.anchor.0 += offset.0;
            draw.anchor.1 += offset.1;
            return Some((frame, draw));
        }
        if let Some((_, frame)) = self
            .speaking_pose
            .as_ref()
            .filter(|(key, _)| *key == raster.key)
        {
            return Some((frame, raster.frame_draw(frame)));
        }
        raster.key.1.6?;
        match &self.target(raster.key)?.action {
            Action::Door(door) => Some(door.draw(&self.door_states, elapsed)),
            _ => None,
        }
    }

    pub(super) fn pick(
        &self,
        point: (i64, i64),
        reference: Tile,
        elapsed: Duration,
    ) -> Option<WorldOrder> {
        self.pick_raster(point, reference, elapsed, false)
            .filter(|&key| {
                self.target(key)
                    .is_some_and(|t| !matches!(t.action, Action::Window { .. }))
            })
    }

    pub(super) fn pick_raster(
        &self,
        point: (i64, i64),
        reference: Tile,
        elapsed: Duration,
        inspect: bool,
    ) -> Option<WorldOrder> {
        if let Some(tile) =
            crate::isometric::world_tile_at_pixel((point.0, point.1.checked_add(120)?), reference)
            && let Ok(index) = self
                .ordered
                .binary_search_by_key(&roof_key(roof::cell_center(tile)), |raster| raster.key)
        {
            let roof = &self.ordered[index];
            if self.raster_revealed(roof) && hits(roof.frame_draw(&roof.frame), point) {
                return None;
            }
        }
        self.ordered
            .iter()
            .rev()
            .filter(|raster| {
                raster.key.0
                    // CE sub_43D9F0 skips walls with OWAF_TRANS_LEFT or
                    // OWAF_TRANS_RIGHT during object hit testing.
                    && (inspect || (!raster.hidden && !raster.click_through && !self.faded_walls.contains_key(&raster.key)))
                    && !matches!(raster.object_light, ObjectLight::Roof)
                    && self.raster_revealed(raster)
            })
            .find_map(|raster| {
                let sprite = self.interaction_frame(raster, elapsed).map_or_else(
                    || raster.frame_draw(raster.asset_at(elapsed, self.fidget)),
                    |(_, sprite)| sprite,
                );
                hits_scaled(sprite, point, raster.scale).then_some(raster.key)
            })
    }

    pub(super) fn approach(&mut self, key: WorldOrder, run: bool) -> Option<usize> {
        let target = self.target(key)?;
        let tile = target.tile;
        let directions: &[u8] = match &target.action {
            Action::Door(door) if !door.direct_transition() => {
                std::slice::from_ref(&door.direction)
            }
            Action::Container(_) | Action::Npc(_) | Action::Summon(_) => &[0, 1, 2, 3, 4, 5, 6, 7],
            _ => &[1, 3, 5, 7],
        };
        let entries = &self.entries;
        let states = &self.door_states;
        let cell = |tile| Self::player_cell(entries, states, tile);
        let reachable = |from| match &target.action {
            Action::Container(_) | Action::Npc(_) | Action::Summon(_) => {
                collision::can_reach_container(from, tile, cell)
            }
            Action::Pickup(_) => Self::can_reach_item(from, tile, cell),
            _ => true,
        };
        let from = self.player.as_ref()?.route_origin();
        if matches!(target.action, Action::Npc(_))
            && self.npc(key).is_some_and(|(r, _)| {
                r.prototype.as_ref().is_some_and(|p| {
                    self.inventory
                        .npc_vitals(r.id, &p.defaults, &r.instance())
                        .1
                        .current_health
                        > 0
                })
            })
            && npc::in_range(from, tile)
        {
            return Some(0);
        }
        let candidates: Vec<_> = std::iter::once(tile)
            .chain(
                directions
                    .iter()
                    .filter_map(|&direction| neighbor(tile, direction)),
            )
            .filter(|&from| reachable(from))
            .collect();
        let player = self.player.as_mut()?;
        player.destination_near(&candidates, run, cell)
    }

    pub(super) fn use_target(
        &mut self,
        key: WorldOrder,
        tick: Duration,
        sound: &mut Option<i32>,
    ) -> Option<Result<Option<Destination>, String>> {
        let (sector, _) = self.entries.get(&tile_sector_id((key.1.2, key.1.1)))?;
        let target = sector.interactions.get(&key)?;
        let player = self.player.as_ref()?;
        if player.moving() {
            return None;
        }
        let tile = player.tile();
        let facing = player.facing();
        let near = match &target.action {
            Action::Door(door) => door.within_reach(tile),
            _ => tile.0.abs_diff(target.tile.0) + tile.1.abs_diff(target.tile.1) <= 1,
        };
        if !near {
            return Some(Err("cannot reach the object".into()));
        }
        match &target.action {
            Action::Door(door) => {
                let access = self.key_or_egress(door, tile);
                if let Some(destination) = door.destination {
                    return Some(
                        door.may_open(
                            &self.door_states,
                            self.hour,
                            access || self.door_states.developer_unlocked.contains(&door.id),
                            &self.scripts,
                            tile,
                        )
                        .map(|()| Some(destination)),
                    );
                }
                let result = self.door_states.change(
                    door,
                    tick,
                    self.hour,
                    access,
                    tile,
                    (&mut self.scripts, &mut self.inventory),
                );
                let offset = match &result {
                    Ok(true) => Some(0),
                    Ok(false) => Some(1),
                    Err(_)
                        if door
                            .may_open(
                                &self.door_states,
                                self.hour,
                                access || self.door_states.developer_unlocked.contains(&door.id),
                                &self.scripts,
                                tile,
                            )
                            .is_err() =>
                    {
                        Some(2)
                    }
                    Err(_) => None,
                };
                *sound = door
                    .sound_effect
                    .filter(|&id| id > 0)
                    .zip(offset)
                    .and_then(|(id, offset)| id.checked_add(offset));
                Some(result.map(|_| None))
            }
            Action::Travel(dst) => {
                eprintln!(
                    "Using original transition at {:?}, facing {facing}",
                    target.tile
                );
                Some(Ok(Some(*dst)))
            }
            Action::Unsupported(reason) => Some(Err(reason.clone())),
            Action::Window { .. }
            | Action::Container(_)
            | Action::Pickup(_)
            | Action::Summon(_)
            | Action::Npc(_) => None,
        }
    }

    pub(super) fn door_delay(&self, viewport: &Viewport, tick: Duration) -> Option<Duration> {
        if self.door_states.poses.is_empty() {
            return None;
        }
        self.active
            .iter()
            .flat_map(|id| self.entries[id].0.interactions.values())
            .filter_map(|target| {
                let Action::Door(door) = &target.action else {
                    return None;
                };
                let change = *self.door_states.poses.get(&door.id)?;
                let (_, sprite) = door.draw(&self.door_states, tick);
                viewport
                    .intersects(RasterBounds::new(sprite))
                    .then_some(())?;
                door.delay(change, tick)
            })
            .min()
    }
}

pub(super) fn hits(sprite: SpriteDraw<'_>, point: (i64, i64)) -> bool {
    hits_scaled(sprite, point, 100)
}

pub(super) fn hits_scaled(sprite: SpriteDraw<'_>, point: (i64, i64), scale: u32) -> bool {
    let ((left, top), (width, height)) = scaled_sprite_rect(sprite, scale);
    let (x, y) = (point.0 - left, point.1 - top);
    if !(0..width).contains(&x) || !(0..height).contains(&y) {
        return false;
    }
    let source = |offset| (offset * 100 / i64::from(scale)) as usize;
    sprite.frame.pixels[source(y) * sprite.frame.width as usize + source(x)][3] != 0
}

pub(super) fn neighbor((x, y): Tile, direction: u8) -> Option<Tile> {
    let (dx, dy) = collision::DIRECTIONS[usize::from(direction)];
    Some((x.checked_add_signed(dx)?, y.checked_add_signed(dy)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_sprite_bounds_and_hit_use_the_same_hotspot_and_texels() {
        let mut frame = crate::art::SpriteFrame {
            width: 4,
            height: 4,
            hotspot: (2, -2),
            offset: (0, 0),
            pixels: vec![[0; 4]; 16],
        };
        frame.pixels[2] = [255; 4];
        let sprite = SpriteDraw {
            frame: &frame,
            anchor: (0, 0),
        };
        let bounds = RasterBounds::scaled(sprite, 50);
        assert_eq!((bounds.min, bounds.max), ((-1, 1), (1, 3)));
        assert!(hits_scaled(sprite, (0, 1), 50));
        assert!(!hits_scaled(sprite, (-1, 1), 50)); // Transparent texel inside the quad.
        assert!(!hits_scaled(sprite, (1, 1), 50)); // Clipped right edge.
    }

    #[test]
    #[ignore = "requires installed original data"]
    fn original_bates_door_collision_and_map_transitions() {
        let game = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let mut vfs = Vfs::from_game(&game, crate::vfs::Policy::Ce).unwrap();
        let catalog = super::super::super::maps::Catalog::load(&mut vfs).unwrap();
        let bates = catalog.find("5011").unwrap();
        let files = bates.files(&mut vfs).unwrap();
        let center = files.center(None).unwrap();
        let mut camera = files.region.camera(center).unwrap();
        camera.follow((0, 0));
        let mut world = WorldStream::new(vfs, bates.directory.clone(), files.region, center)
            .unwrap()
            .with_starting_lighting()
            .unwrap()
            .with_player()
            .unwrap();
        world.prepare(&camera, (1280, 720)).unwrap();
        let bates = &world.cache.entries[&67108865].0;
        let (tile, door) = bates
            .interactions
            .values()
            .find_map(|target| {
                let Action::Door(door) = &target.action else {
                    return None;
                };
                (door.id.1 == 244).then_some((target.tile, door))
            })
            .expect("Bates authored billiard room door");
        let edge_cell = |states: &DoorStates| {
            SectorCache::player_cell(&world.cache.entries, states, tile).unwrap()
        };
        let mut states = DoorStates::default();
        let travel = |states: &DoorStates| {
            let from = neighbor(tile, door.direction).unwrap();
            collision::step(from, (door.direction + 4) % 8, |tile| {
                SectorCache::player_cell(&world.cache.entries, states, tile)
            })
        };
        let closed = edge_cell(&states);
        assert_eq!(travel(&states), None);
        assert!(
            states
                .change(
                    door,
                    Duration::ZERO,
                    15,
                    false,
                    tile,
                    (&mut State::default(), &mut Default::default())
                )
                .unwrap()
        );
        assert!(door.draw(&states, Duration::ZERO).0.decoded.frame_count > 1);
        assert!(!Arc::ptr_eq(
            door.draw(&states, Duration::ZERO).0,
            &door.frames[0].0
        ));
        let open = edge_cell(&states);
        assert_ne!(closed, open);
        assert_eq!(travel(&states), Some(tile));
        assert!(
            !states
                .change(
                    door,
                    Duration::from_secs(2),
                    15,
                    false,
                    tile,
                    (&mut State::default(), &mut Default::default())
                )
                .unwrap()
        );
        assert_eq!(closed, edge_cell(&states));
        assert_eq!(travel(&states), None);

        // Unlock changes permission, while the original door stays solid until opened.
        let mut ordinary = Door {
            sound_effect: None,
            id: door.id,
            tile: door.tile,
            originally_open: false,
            direction: door.direction,
            flags: 1,
            unlock_script: 0,
            unlock: None,
            use_script: 0,
            difficulty: 0,
            notify: 0,
            key_id: 0,
            rotation: door.rotation,
            offset: door.offset,
            on_use: None,
            destination: None,
            frames: door.frames.clone(),
            period: door.period,
        };
        let mut states = DoorStates::default();
        assert_eq!(
            states.change(
                &ordinary,
                Duration::ZERO,
                15,
                false,
                tile,
                (&mut State::default(), &mut Default::default())
            ),
            Err("door is locked".into())
        );
        assert!(states.unlock(&ordinary).unwrap());
        assert_eq!(travel(&states), None);
        assert!(
            states
                .change(
                    &ordinary,
                    Duration::ZERO,
                    15,
                    false,
                    tile,
                    (&mut State::default(), &mut Default::default())
                )
                .unwrap()
        );
        assert_eq!(travel(&states), Some(tile));
        ordinary.unlock_script = 1151;
        assert!(DoorStates::default().unlock(&ordinary).is_err());

        let dst = world.cache.jumps[&(109, 92)];
        assert_eq!(
            dst,
            Destination {
                map: Some(5000),
                tile: (61976, 65664)
            }
        );
        assert!(bates.interactions.values().any(|target| matches!(
            target.action.destination(),
            Some(Destination {
                map: Some(5012),
                tile: (99, 91)
            })
        )));
        let mut vfs = Vfs::from_game(&game, crate::vfs::Policy::Ce).unwrap();
        let exterior = catalog.find("5000").unwrap();
        let files = exterior.files(&mut vfs).unwrap();
        let center = files.center(Some(dst.tile)).unwrap();
        let region = files.bounds();
        let mut camera = region.camera(center).unwrap();
        camera.follow((0, 0));
        let mut outside = WorldStream::new(vfs, exterior.directory.clone(), region, center)
            .unwrap()
            .with_starting_lighting()
            .unwrap()
            .with_player()
            .unwrap();
        outside.prepare(&camera, (1280, 720)).unwrap();
        outside.validate_arrival(dst.tile).unwrap();
        assert!(outside.stats().light_samples > 0);
        assert!(
            outside.cache.entries[&tile_sector_id(dst.tile)]
                .0
                .interactions
                .values()
                .any(|target| matches!(
                    target.action.destination(),
                    Some(Destination {
                        map: Some(5011),
                        tile: (104, 92)
                    })
                ))
        );
    }

    #[test]
    fn lock_flags_have_original_time_gate() {
        assert!(locked(1, 12));
        assert!(locked(0x11, 12));
        assert!(!locked(0x10, 12));
        assert!(locked(0x21, 12));
        assert!(!locked(0x21, 22));
        assert!(locked(0x41, 22));
        assert!(!locked(8 | 1, 22));
    }
}
