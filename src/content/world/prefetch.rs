//! One sector loader with a replaceable, prioritized request list.
use super::*;
use std::{
    collections::VecDeque,
    io,
    sync::{Condvar, Mutex},
    thread::{self, JoinHandle},
};

const LOOKAHEAD_PIXELS: i32 = 320;
const SPECULATIVE_LIMIT: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Priority {
    Required,
    Prefetch,
}

fn predict(
    region: SectorRegion,
    reference: (u32, u32),
    position: (i32, i32),
    viewport: (u32, u32),
    direction: (i32, i32),
    required: &[u64],
) -> Vec<u64> {
    if direction == (0, 0) {
        return Vec::new();
    }
    let distance = i64::from(direction.0)
        .abs()
        .max(i64::from(direction.1).abs());
    let axis = |delta| (i64::from(delta) * i64::from(LOOKAHEAD_PIXELS) / distance) as i32;
    let ahead = (axis(direction.0), axis(direction.1));
    let mut next = Vec::new();
    for factor in [2, 1] {
        let future = (
            position.0.saturating_add(ahead.0 / factor),
            position.1.saturating_add(ahead.1 / factor),
        );
        if let Ok(ids) = region.needed(reference, future, viewport) {
            for &id in ids.as_slice() {
                if next.len() == SPECULATIVE_LIMIT {
                    return next;
                }
                if !required.contains(&id) && !next.contains(&id) {
                    next.push(id);
                }
            }
        }
    }
    next
}

#[derive(Clone, Copy)]
struct Request {
    id: u64,
    priority: Priority,
    enqueued: Instant,
}

struct Completion {
    sector: CachedSector,
    request: Request,
    elapsed: Duration,
    queued: Duration,
    stages: LoadStages,
    resources: (u64, FrameCacheStats),
}

#[derive(Default)]
struct Mailbox {
    queue: VecDeque<Request>,
    loading: Option<(u64, Priority)>,
    ready: Option<Completion>,
    stop: bool,
}

#[derive(Default)]
struct Exchange {
    mailbox: Mutex<Mailbox>,
    changed: Condvar,
}

struct Worker {
    exchange: Arc<Exchange>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    fn start(
        mut load: impl FnMut(u64) -> (CachedSector, LoadStages, (u64, FrameCacheStats)) + Send + 'static,
        wake: impl Fn() + Send + 'static,
    ) -> io::Result<Self> {
        let exchange = Arc::new(Exchange::default());
        let worker = Arc::clone(&exchange);
        let thread = thread::Builder::new()
            .name("sector-loader".into())
            .spawn(move || {
                loop {
                    let request = {
                        let mut mail = worker.mailbox.lock().unwrap();
                        while !mail.stop && (mail.queue.is_empty() || mail.ready.is_some()) {
                            mail = worker.changed.wait(mail).unwrap();
                        }
                        if mail.stop {
                            return;
                        }
                        let request = mail.queue.pop_front().unwrap();
                        mail.loading = Some((request.id, request.priority));
                        request
                    };
                    let start = Instant::now();
                    let (sector, stages, resources) = load(request.id);
                    let completion = Completion {
                        sector,
                        request,
                        elapsed: start.elapsed(),
                        queued: start - request.enqueued,
                        stages,
                        resources,
                    };
                    {
                        let mut mail = worker.mailbox.lock().unwrap();
                        if mail.stop {
                            return;
                        }
                        mail.loading = None;
                        mail.ready = Some(completion);
                    }
                    wake();
                }
            })?;
        Ok(Self {
            exchange,
            thread: Some(thread),
        })
    }

    fn plan(&self, jobs: &[(u64, Priority)], trace: bool) {
        let mut mail = self.exchange.mailbox.lock().unwrap();
        let time = Instant::now();
        let mut next = VecDeque::new();
        for &(id, priority) in jobs {
            if mail.loading.is_some_and(|loading| loading.0 == id)
                || mail
                    .ready
                    .as_ref()
                    .is_some_and(|done| done.request.id == id)
                || next.iter().any(|request: &Request| request.id == id)
            {
                continue;
            }
            let enqueued = mail
                .queue
                .iter()
                .find(|request| request.id == id)
                .map_or(time, |request| request.enqueued);
            next.push_back(Request {
                id,
                priority,
                enqueued,
            });
        }
        let changed = trace
            && !next
                .iter()
                .map(|r| (r.id, r.priority))
                .eq(mail.queue.iter().map(|r| (r.id, r.priority)));
        let trace_queue =
            changed.then(|| next.iter().map(|r| (r.id, r.priority)).collect::<Vec<_>>());
        let loading = mail.loading;
        mail.queue = next;
        drop(mail);
        self.exchange.changed.notify_one();
        if let Some(queue) = trace_queue {
            eprintln!("Sector requests {queue:?}; loading {loading:?}");
        }
    }

    fn take(&self) -> Option<Completion> {
        let ready = self.exchange.mailbox.lock().unwrap().ready.take();
        if ready.is_some() {
            self.exchange.changed.notify_one();
        }
        ready
    }

    fn pending(&self) -> usize {
        let mail = self.exchange.mailbox.lock().unwrap();
        mail.queue.len() + usize::from(mail.loading.is_some() || mail.ready.is_some())
    }

    fn request_stop(&self) {
        self.exchange.mailbox.lock().unwrap().stop = true;
        self.exchange.changed.notify_one();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.request_stop();
        // The caller drops the worker after leaving the presentation loop.
        // The only work left is at most the single in-flight import.
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PrefetchStats {
    pub cache: StreamStats,
    pub prefetch_hits: u64,
    pub prefetch_misses: u64,
    pub required_misses: u64,
    pub obsolete: u64,
    pub pending: usize,
    /// Time a requested view was held waiting for its complete scene. No event-thread wait.
    pub view_hold: Duration,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ViewPreparation {
    pub ready: bool,
    pub ordering: Duration,
    pub shading: Duration,
    pub scheduling: Duration,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ViewPlan {
    position: (i32, i32),
    viewport: (u32, u32),
    direction: (i32, i32),
}

/// Owns only prepared, immutable raster resources on the presentation thread.
pub struct InteractiveWorld {
    region: SectorRegion,
    reference: (u32, u32),
    cache: SectorCache,
    worker: Worker,
    resources: (u64, FrameCacheStats),
    required: Vec<u64>,
    speculative: Vec<u64>,
    ready_plan: Option<ViewPlan>,
    prefetch_hits: u64,
    prefetch_misses: u64,
    required_misses: u64,
    obsolete: u64,
    hold_started: Option<Instant>,
    view_hold: Duration,
    trace: bool,
    player_started: bool,
    transition: Option<(Destination, u8)>,
    pending_use: Option<WorldOrder>,
    notice: Option<String>,
}

impl InteractiveWorld {
    pub(super) fn new(
        world: WorldStream,
        wake: impl Fn() + Send + 'static,
        trace: bool,
    ) -> io::Result<Self> {
        let WorldStream {
            region,
            mut loader,
            cache,
        } = world;
        let reference = loader.reference;
        let resources = loader.stats();
        let worker = Worker::start(
            move |id| {
                let (sector, stages) = loader.load(id);
                (sector, stages, loader.stats())
            },
            wake,
        )?;
        Ok(Self {
            region,
            reference,
            cache,
            worker,
            resources,
            required: Vec::new(),
            speculative: Vec::new(),
            ready_plan: None,
            prefetch_hits: 0,
            prefetch_misses: 0,
            required_misses: 0,
            obsolete: 0,
            hold_started: None,
            view_hold: Duration::ZERO,
            trace,
            player_started: false,
            transition: None,
            pending_use: None,
            notice: None,
        })
    }

    /// Consume one complete import. Only an import needed by the requested view wakes rendering.
    pub fn receive(&mut self) -> bool {
        let Some(done) = self.worker.take() else {
            return false;
        };
        // A completion can change available entries and speculative eviction.
        self.ready_plan = None;
        let id = done.request.id;
        let required = self.required.contains(&id);
        let wanted = required || self.speculative.contains(&id);
        let start = Instant::now();
        let evictions = self.cache.evictions;
        let accepted = wanted && self.cache.evict(MAX_SECTORS, &self.required, true);
        let (reads, art) = done.resources;
        if accepted {
            self.cache
                .entries
                .insert(id, (done.sector, required.then_some(self.cache.step)));
        } else {
            self.obsolete += 1;
        }
        let insertion = start.elapsed();
        if self.trace && self.cache.evictions != evictions {
            eprintln!(
                "Sector cache evictions {}; {} resident",
                self.cache.evictions,
                self.cache.entries.len()
            );
        }
        let report = self
            .cache
            .entries
            .get(&id)
            .map(|(sector, _)| &sector.report);
        if self.trace || report.is_some_and(|r| r.object_list_failed) {
            eprintln!(
                "Sector {id} {:?} complete: accepted {accepted}, queue {:?}, worker {:?}, stages {:?}, VFS +{}, ART reads +{}, decodes +{}, ART resolve {:?}, ART read {:?}, ART decode {:?}, insertion {insertion:?}, {} surfaces, {} diagnostics {}",
                done.request.priority,
                done.queued,
                done.elapsed,
                done.stages,
                reads - self.resources.0,
                art.reads - self.resources.1.reads,
                art.decodes - self.resources.1.decodes,
                art.resolution_time - self.resources.1.resolution_time,
                art.read_time - self.resources.1.read_time,
                art.decode_time - self.resources.1.decode_time,
                report.map_or(0, |r| r.surfaces),
                report.map_or(0, |r| r.diagnostics.len()),
                report
                    .and_then(|r| r.diagnostics.first())
                    .map_or("", String::as_str)
            );
        }
        self.resources = done.resources;
        self.cache.loads += 1;
        accepted && required
    }

    pub fn prepare(
        &mut self,
        camera: &Camera,
        viewport: (u32, u32),
        direction: (i32, i32),
    ) -> Result<ViewPreparation, String> {
        let begin = Instant::now();
        if self
            .worker
            .thread
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
        {
            return Err("sector preparation worker stopped".into());
        }
        let position = camera.position();
        let plan = ViewPlan {
            position,
            viewport,
            direction,
        };
        if self.ready_plan == Some(plan) {
            return Ok(ViewPreparation {
                ready: true,
                scheduling: begin.elapsed(),
                ..ViewPreparation::default()
            });
        }
        let needed = self.region.needed(self.reference, position, viewport)?;
        let next = needed.as_slice();
        for id in next.iter().filter(|id| !self.required.contains(id)) {
            if let Some((_, touched)) = self.cache.entries.get(id) {
                self.cache.hits += 1;
                self.prefetch_hits += u64::from(touched.is_none());
                if self.trace && touched.is_none() {
                    eprintln!("Sector {id} prefetch hit at camera {position:?}");
                }
            } else {
                self.required_misses += 1;
                self.prefetch_misses += u64::from(self.speculative.contains(id));
            }
        }
        self.required.clear();
        self.required.extend_from_slice(next);
        let ready = next.iter().all(|id| self.cache.entries.contains_key(id));
        let active_elsewhere = self
            .cache
            .active
            .iter()
            .filter(|id| !next.contains(id))
            .count();
        if next.len() + active_elsewhere > MAX_SECTORS {
            // The previous buffer remains on screen, but its scene is no longer
            // needed when the two footprints cannot both fit.
            self.cache.activate(&[]);
        }
        let start = Instant::now();
        let shading = if ready && self.cache.active != next {
            self.cache.activate(next)
        } else {
            Duration::ZERO
        };
        let ordering = start.elapsed().saturating_sub(shading);
        if ready {
            if let Some(start) = self.hold_started.take() {
                let hold = start.elapsed();
                self.view_hold += hold;
                if self.trace {
                    eprintln!(
                        "World view became ready after {hold:?}; event-thread loading wait 0ns"
                    );
                }
            }
        } else if self.hold_started.is_none() {
            self.hold_started = Some(Instant::now());
        }
        let free = MAX_SECTORS
            - next.len()
            - self
                .cache
                .active
                .iter()
                .filter(|id| !next.contains(id))
                .count();
        self.speculative = predict(
            self.region,
            self.reference,
            position,
            viewport,
            direction,
            next,
        );
        self.speculative
            .retain(|id| !self.cache.active.contains(id));
        self.speculative.truncate(free);
        let mut jobs = Vec::with_capacity(MAX_SECTORS);
        jobs.extend(
            next.iter()
                .filter(|id| !self.cache.entries.contains_key(id))
                .map(|&id| (id, Priority::Required)),
        );
        jobs.extend(
            self.speculative
                .iter()
                .filter(|id| !self.cache.entries.contains_key(id))
                .map(|&id| (id, Priority::Prefetch)),
        );
        self.worker.plan(&jobs, self.trace);
        self.ready_plan = ready.then_some(plan);
        Ok(ViewPreparation {
            ready,
            ordering,
            shading,
            scheduling: begin.elapsed().saturating_sub(ordering + shading),
        })
    }

    pub fn stats(&self) -> PrefetchStats {
        PrefetchStats {
            cache: self.cache.stats(self.resources),
            prefetch_hits: self.prefetch_hits,
            prefetch_misses: self.prefetch_misses,
            required_misses: self.required_misses,
            obsolete: self.obsolete,
            pending: self.worker.pending(),
            view_hold: self.view_hold,
        }
    }

    pub fn visible(
        &self,
        camera: &Camera,
        viewport: (u32, u32),
    ) -> impl Iterator<Item = SpriteDraw<'_>> {
        self.cache.visible(camera, viewport)
    }

    pub fn dynamic_light(&self, slot: usize) -> Option<DynamicLightState> {
        self.cache.dynamic.get(slot).map(|light| light.state)
    }

    pub fn set_dynamic_light(
        &mut self,
        slot: usize,
        state: DynamicLightState,
    ) -> Result<LightUpdate, &'static str> {
        if self.region.camera(state.tile).is_none() {
            return Err("light position is outside the world region");
        }
        self.cache.set_dynamic(slot, state)
    }

    pub fn playing(&self) -> bool {
        self.cache.player.is_some()
    }

    /// Compose the camera and player raster at the same simulation timestamp.
    pub fn follow_player(&self, elapsed: Duration, camera: &mut Camera) {
        if let Some(player) = &self.cache.player {
            camera.follow(player.sample(elapsed).anchor);
        }
    }

    pub fn player_moving(&self) -> bool {
        self.pending_use.is_some() || self.cache.player.as_ref().is_some_and(Player::moving)
    }

    pub fn start_player(&mut self) -> Result<(), String> {
        if self.player_started {
            return Ok(());
        }
        if let Some(player) = &self.cache.player {
            self.cache
                .validate_arrival(player.tile())
                .map_err(|error| {
                    format!("{error}; choose another starting tile with --center X Y")
                })?;
        }
        self.player_started = true;
        Ok(())
    }

    pub fn set_player_destination(
        &mut self,
        screen: (u32, u32),
        size: (u32, u32),
        presented: &Camera,
        run: bool,
        elapsed: Duration,
    ) {
        if !self.player_started || screen.0 >= size.0 || screen.1 >= size.1 {
            return;
        }
        let (x, y) = presented.position();
        let point = (
            i64::from(x) + i64::from(screen.0) - i64::from(size.0 / 2),
            i64::from(y) + i64::from(screen.1) - i64::from(size.1 / 2),
        );
        self.cancel_player_route();
        if let Some(key) = self.cache.pick(point, elapsed) {
            let steps = self.cache.approach(key, run);
            self.pending_use = steps.map(|_| key);
            if steps.is_none() {
                self.notice = Some("cannot reach the object".into());
            }
            if self.trace {
                eprintln!("Use target {key:?}: {steps:?} steps");
            }
            return;
        }
        let Some(player) = &mut self.cache.player else {
            return;
        };
        let tile = crate::isometric::world_tile_at_pixel(point, self.reference);
        let entries = &self.cache.entries;
        let states = &self.cache.door_states;
        let steps = tile.and_then(|tile| {
            player.destination(tile, run, |tile| {
                SectorCache::player_cell(entries, states, tile)
            })
        });
        if self.trace {
            eprintln!("Player target {tile:?}: {steps:?} steps, running {run}");
        }
    }

    pub fn cancel_player_route(&mut self) {
        self.pending_use = None;
        if let Some(player) = &mut self.cache.player {
            player.cancel_route();
        }
    }

    pub fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }

    pub fn take_door_states(&mut self) -> DoorStates {
        std::mem::take(&mut self.cache.door_states)
    }

    pub fn restore_door_states(&mut self, states: DoorStates) {
        self.cache.door_states = states;
    }

    pub fn take_transition(&mut self) -> Option<(Destination, u8)> {
        self.transition.take()
    }

    pub fn move_player(
        &mut self,
        input: (i32, i32),
        run: bool,
        now: Duration,
        camera: &mut Camera,
    ) -> bool {
        if !self.player_started {
            return false;
        }
        let before = self.cache.player.as_ref().map(|p| {
            let pose = p.sample(now);
            (pose.anchor, Arc::as_ptr(pose.asset), p.tile(), p.facing())
        });
        let Some(anchor) = self.cache.update_player(input, run, now) else {
            return false;
        };
        let pose = self.cache.player.as_ref().unwrap().sample(now);
        let previous_camera = camera.position();
        camera.follow(anchor);
        let changed = previous_camera != anchor
            || before.is_some_and(|(previous, asset, tile, _)| {
                previous != anchor || asset != Arc::as_ptr(pose.asset) || tile != pose.tile
            });
        let player = self.cache.player.as_ref().unwrap();
        if let Some((_, _, tile, facing)) = before
            && tile != player.tile()
        {
            self.transition = self
                .cache
                .jumps
                .get(&player.tile())
                .copied()
                .map(|dst| (dst, facing));
            if self.trace {
                eprintln!(
                    "Player entered tile {:?}, camera {anchor:?}, ART {}",
                    player.tile(),
                    pose.asset.source.virtual_path
                );
            }
        }
        if self.transition.is_none()
            && !player.moving()
            && let Some(key) = self.pending_use.take()
        {
            let facing = player.facing();
            match self.cache.use_target(key, now) {
                Some(Ok(Some(dst))) => self.transition = Some((dst, facing)),
                Some(Err(error)) => {
                    self.notice = Some(error);
                }
                None => {
                    self.notice = Some("object is unavailable".into());
                }
                Some(Ok(None)) => {}
            }
            return true;
        }
        changed
    }

    /// Start at the current epoch and report whether the viewport needs redraw.
    pub fn start_walk(&mut self, camera: &Camera, size: (u32, u32), elapsed: Duration) -> bool {
        if !self.cache.start_walk(elapsed) {
            return false;
        }
        let viewport = Viewport::new(camera.position(), size);
        let source = self
            .cache
            .walk_subject
            .as_ref()
            .expect("starting walk subject")
            .0;
        self.cache.ordered[source]
            .visible_at(&viewport, elapsed, self.cache.fidget)
            .is_some()
            || self.cache.walk_delay(&viewport, elapsed).is_some()
    }

    pub fn reset_walk(&mut self) -> bool {
        self.cache.reset_walk()
    }

    pub fn update_walk(&mut self, elapsed: Duration) -> Option<WalkStatus> {
        self.cache.update_walk(elapsed)
    }

    pub fn start_fidget(&mut self, elapsed: Duration) -> bool {
        self.cache.start_fidget(elapsed)
    }

    pub fn turn_fidget(&mut self, step: i8) -> Option<u8> {
        self.cache.turn_fidget(step)
    }

    pub fn toggle_shadows(&mut self) -> Option<bool> {
        self.cache.toggle_shadows()
    }

    pub fn toggle_probe_shadow(&mut self, slot: usize) -> Option<(bool, bool)> {
        self.cache.toggle_probe_shadow(slot)
    }

    pub fn animation_delay(
        &self,
        camera: &Camera,
        size: (u32, u32),
        elapsed: Duration,
    ) -> Option<Duration> {
        self.cache.animation_delay(camera, size, elapsed)
    }

    pub fn visible_assets_at(
        &self,
        camera: &Camera,
        size: (u32, u32),
        elapsed: Duration,
    ) -> impl Iterator<Item = super::WorldDraw<'_>> {
        self.cache.visible_assets(camera, size, elapsed)
    }

    pub fn visible_modulated_at(
        &self,
        camera: &Camera,
        size: (u32, u32),
        elapsed: Duration,
    ) -> impl Iterator<Item = (SpriteDraw<'_>, crate::compositor::Modulation<'_>)> {
        self.cache.visible_modulated(camera, size, elapsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    const TIMEOUT: Duration = Duration::from_secs(3);
    const VIEWPORT: (u32, u32) = (160, 100);

    fn strip() -> SectorRegion {
        SectorRegion {
            first: Coordinates { x: 0, y: 0 },
            last: Coordinates { x: 3, y: 0 },
        }
    }

    #[test]
    fn prediction_favors_the_direction_of_travel() {
        let region = strip();
        let ref_tile = (34, 32);
        let now = region.needed(ref_tile, (0, 0), VIEWPORT).unwrap();
        assert_eq!(now.as_slice(), [0]);
        assert_eq!(
            predict(
                region,
                ref_tile,
                (0, 0),
                VIEWPORT,
                (-64, 32),
                now.as_slice()
            ),
            [1]
        );
        assert!(
            predict(
                region,
                ref_tile,
                (0, 0),
                VIEWPORT,
                (64, -32),
                now.as_slice()
            )
            .is_empty()
        );
        assert!(predict(region, ref_tile, (0, 0), VIEWPORT, (0, 0), now.as_slice()).is_empty());
    }

    #[test]
    fn worker_deduplicates_reprioritizes_and_replaces_obsolete_work() {
        let (started, starts) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let (wake, completed) = mpsc::channel();
        let worker = Worker::start(
            move |id| {
                started.send(id).unwrap();
                gate.recv_timeout(TIMEOUT).unwrap();
                (
                    CachedSector::missing(id, "delayed synthetic failure".into()),
                    LoadStages::default(),
                    (0, FrameCacheStats::default()),
                )
            },
            move || wake.send(()).unwrap(),
        )
        .unwrap();

        worker.plan(&[(0, Priority::Prefetch), (3, Priority::Prefetch)], false);
        assert_eq!(starts.recv_timeout(TIMEOUT).unwrap(), 0);
        worker.plan(
            &[
                (1, Priority::Required),
                (1, Priority::Prefetch),
                (0, Priority::Prefetch),
                (2, Priority::Prefetch),
            ],
            false,
        );
        assert_eq!(worker.pending(), 3);
        assert!(worker.take().is_none());
        worker.plan(&[(1, Priority::Required)], false);
        release.send(()).unwrap();
        completed.recv_timeout(TIMEOUT).unwrap();
        let first = worker.take().unwrap();
        assert_eq!(first.request.id, 0);
        assert!(first.sector.report.source.is_none());
        assert!(first.sector.draws.is_empty());
        assert_eq!(starts.recv_timeout(TIMEOUT).unwrap(), 1);
        release.send(()).unwrap();
        completed.recv_timeout(TIMEOUT).unwrap();
        assert_eq!(worker.take().unwrap().request.priority, Priority::Required);
        assert_eq!(worker.pending(), 0);
        drop(worker);
        assert!(starts.recv_timeout(TIMEOUT).is_err());
    }

    #[test]
    fn obsolete_completion_does_not_enter_the_cache() {
        let (wake, done) = mpsc::channel();
        let region = strip();
        let reference = (32, 32);
        let camera = region.camera(reference).unwrap();
        let world = WorldStream::new(
            Vfs::new(crate::vfs::Policy::Ordered),
            "missing".into(),
            region,
            reference,
        )
        .unwrap();
        let mut world = world
            .into_interactive(
                move || {
                    let _ = wake.send(());
                },
                false,
            )
            .unwrap();
        assert!(!world.prepare(&camera, VIEWPORT, (0, 0)).unwrap().ready);
        // Simulate a target replacement while the worker is busy.
        world.required.clear();
        done.recv_timeout(TIMEOUT).unwrap();
        assert!(!world.receive());
        assert_eq!(world.stats().obsolete, 1);
        assert!(world.cache.entries.is_empty());
        assert!(!world.prepare(&camera, VIEWPORT, (0, 0)).unwrap().ready);
        done.recv_timeout(TIMEOUT).unwrap();
        assert!(world.receive());
        assert!(world.prepare(&camera, VIEWPORT, (0, 0)).unwrap().ready);
        assert_eq!(world.stats().cache.cached_sectors, 1);
        assert!(world.cache.entries[&0].0.report.diagnostics[0].contains("maps/missing/0.sec"));
    }

    #[test]
    fn ready_plan_is_reused_until_view_changes_or_worker_completes() {
        let (wake, done) = mpsc::channel();
        let region = strip();
        let reference = (34, 32);
        let camera = region.camera(reference).unwrap();
        let mut world = WorldStream::new(
            Vfs::new(crate::vfs::Policy::Ordered),
            "missing".into(),
            region,
            reference,
        )
        .unwrap()
        .into_interactive(
            move || {
                let _ = wake.send(());
            },
            false,
        )
        .unwrap();

        assert!(!world.prepare(&camera, VIEWPORT, (0, 0)).unwrap().ready);
        assert!(world.ready_plan.is_none());
        done.recv_timeout(TIMEOUT).unwrap();
        assert!(world.receive());
        assert!(world.prepare(&camera, VIEWPORT, (0, 0)).unwrap().ready);
        assert!(world.prepare(&camera, VIEWPORT, (0, 0)).unwrap().ready);
        assert_eq!(world.stats().cache.sector_reads, 1);
        assert!(world.speculative.is_empty());

        let direction = (-64, 32);
        assert!(world.prepare(&camera, VIEWPORT, direction).unwrap().ready);
        assert_eq!(world.speculative, [1]);
        assert_eq!(world.ready_plan.unwrap().direction, direction);
        done.recv_timeout(TIMEOUT).unwrap();
        assert!(!world.receive()); // Speculative completion does not redraw.
        assert!(world.ready_plan.is_none());
        let resized = (161, 101);
        assert!(world.prepare(&camera, resized, direction).unwrap().ready);
        assert_eq!(world.ready_plan.unwrap().viewport, resized);
        assert_eq!(world.stats().cache.sector_reads, 2);
        assert_eq!(world.cache.active, [0]);
    }

    #[test]
    fn worker_shutdown_finishes_one_import_and_discards_queued_work() {
        let (started, starts) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let worker = Worker::start(
            move |id| {
                started.send(id).unwrap();
                gate.recv_timeout(TIMEOUT).unwrap();
                (
                    CachedSector::missing(id, "fixture".into()),
                    LoadStages::default(),
                    (0, FrameCacheStats::default()),
                )
            },
            || panic!("shutdown published a completion"),
        )
        .unwrap();
        worker.plan(&[(0, Priority::Required), (1, Priority::Prefetch)], false);
        assert_eq!(starts.recv_timeout(TIMEOUT).unwrap(), 0);
        worker.request_stop();
        release.send(()).unwrap();
        drop(worker);
        assert!(matches!(
            starts.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn speculative_sector_is_evicted_before_visited_sector_and_never_evicts_active() {
        let mut cache = SectorCache::default();
        for (id, stamp) in [(0, Some(1)), (1, Some(2)), (2, None)] {
            cache
                .entries
                .insert(id, (CachedSector::missing(id, "fixture".into()), stamp));
        }
        cache.activate(&[0]);
        assert!(cache.evict(3, &[], true));
        assert_eq!(cache.entries.keys().copied().collect::<Vec<_>>(), [0, 1]);
        assert!(!cache.evict(2, &[1], true));
        assert_eq!(cache.active, [0]);
    }
}
