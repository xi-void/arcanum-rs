//! Native output and original sound scheme playback. VFS access and decoder
//! preparation run on the audio worker, independently of presentation.
use crate::app_config::Audio as Volumes;
use arcanum_rs::{
    content::{self, load_mes, read_bounded},
    mes::Mes,
    random::Random,
    vfs::Vfs,
};
use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink, Source, source::ChannelVolume};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::Cursor,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const SCHEME_TICK: Duration = Duration::from_millis(250);
const FADE: Duration = Duration::from_millis(25);
const MAX_CLIP_BYTES: usize = 64 * 1024 * 1024;
const MAX_EFFECTS: usize = 32;
const MAX_CACHED_EFFECTS: usize = 48;
const MAX_CACHED_EFFECT_DURATION: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum Channel {
    Effects,
    Voice,
    Music,
}

impl Channel {
    fn gain(self, levels: Volumes) -> f32 {
        let value = match self {
            Self::Effects => f32::from(levels.effects) * 0.8,
            Self::Voice => f32::from(levels.voice),
            Self::Music => f32::from(levels.music) * 0.8,
        };
        value / 10.
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Random,
    Loop,
    Anchor,
    Over,
}

#[derive(Debug)]
struct Entry {
    path: String,
    mode: Mode,
    time: [u8; 2],
    volume: [u8; 2],
    balance: [u8; 2],
    frequency: u16,
}

impl Entry {
    fn parse(raw: &str, sounds: &BTreeMap<i32, String>) -> Option<Self> {
        let raw = raw.to_ascii_lowercase().replace('\\', "/");
        let mut words = raw.split_ascii_whitespace();
        let name = words.next()?;
        let path = if let Some(id) = name.strip_prefix('#') {
            sounds.get(&id.trim_end_matches('#').parse().ok()?)?.clone()
        } else {
            format!("sound/{name}")
        };
        let mut entry = Self {
            path,
            mode: Mode::Random,
            time: [0, 23],
            volume: [100; 2],
            balance: [50; 2],
            frequency: 5,
        };
        let mut scatter = None;
        for word in words {
            match word {
                "/loop" => entry.mode = Mode::Loop,
                "/over" => entry.mode = Mode::Over,
                "/anchor" if entry.mode != Mode::Over => entry.mode = Mode::Anchor,
                word => {
                    let Some((key, value)) = word.split_once(':') else {
                        continue;
                    };
                    if !matches!(key, "/time" | "/vol" | "/bal" | "/freq" | "/scatter") {
                        continue;
                    }
                    let (first, second) = value.split_once([',', '-']).unwrap_or((value, value));
                    let first: u16 = first.parse().ok()?;
                    let second: u16 = second.parse().ok()?;
                    match key {
                        "/time" => entry.time = [first.min(24) as u8, second.min(24) as u8],
                        "/vol" => entry.volume = [first.min(127) as u8, second.min(127) as u8],
                        "/bal" => entry.balance = [first.min(100) as u8, second.min(100) as u8],
                        "/freq" => entry.frequency = first.min(1000),
                        "/scatter" => scatter = Some(first.min(100) as u8),
                        _ => {}
                    }
                }
            }
        }
        if let Some(spread) = scatter.filter(|_| entry.mode == Mode::Random) {
            entry.volume = [100 - spread, 100];
            entry.balance = [50 - spread / 2, 50 + spread / 2];
        }
        Some(entry)
    }

    fn active(&self, hour: u8) -> bool {
        let [start, end] = self.time;
        if start <= end {
            (start..=end).contains(&hour)
        } else {
            // Late-night entries use 19-0, with 0-5 often listed separately.
            hour >= start || hour <= end
        }
    }

    fn channel(&self, ambient: bool) -> Channel {
        if ambient || self.mode == Mode::Random {
            Channel::Effects
        } else {
            Channel::Music
        }
    }
}

struct Tables {
    sounds: BTreeMap<i32, String>,
    schemes: BTreeMap<i32, Vec<Entry>>,
    terrain: [Vec<usize>; 4],
}

impl Tables {
    fn load(vfs: &mut Vfs) -> Result<Self, content::Error> {
        let mut sounds = BTreeMap::new();
        for (_, name) in load_mes(vfs, "sound/snd_00index.mes")?
            .mes
            .entries()
            .chain(std::iter::once((0, "snd_user.mes")))
        {
            let path = format!("sound/{name}");
            match load_mes(vfs, &path) {
                Ok(table) => {
                    for (id, name) in table.mes.entries() {
                        sounds.entry(id).or_insert_with(|| {
                            format!("sound/{}", name.trim())
                                .replace('\\', "/")
                                .to_ascii_lowercase()
                        });
                    }
                }
                Err(content::Error::Resource {
                    error: arcanum_rs::vfs::Error::NotFound(_),
                    ..
                }) if name == "snd_user.mes" => {}
                Err(error) => eprintln!("Audio table {path}: {error}"),
            }
        }
        // Empty stock SFX entries reserve an ID without naming a sample. Keep
        // first-table precedence before discarding those directory-only paths.
        sounds.retain(|_, path| path != "sound/");
        let index = load_mes(vfs, "sound/schemeindex.mes")?.mes;
        let list = load_mes(vfs, "sound/schemelist.mes")?.mes;
        let schemes = index
            .entries()
            .filter_map(|(id, raw)| {
                let base: i32 = raw.split_once('#')?.1.trim().parse().ok()?;
                let end = base.checked_add(100)?;
                let entries = (base..end)
                    .filter_map(|row| {
                        let raw = list.get(row)?;
                        if raw.trim().is_empty() {
                            return None;
                        }
                        let entry = Entry::parse(raw, &sounds);
                        if entry.is_none() {
                            eprintln!(
                                "Audio scheme {id}, SchemeList row {row}: invalid sound entry"
                            );
                        }
                        entry
                    })
                    .collect();
                Some((id, entries))
            })
            .collect();
        let terrain = tile_sounds(&load_mes(vfs, "art/tile/tilename.mes")?.mes);
        Ok(Self {
            sounds,
            schemes,
            terrain,
        })
    }

    fn footstep(&self, surface: u8, armor: u8) -> i32 {
        // CE sfx_critter_sound and a_name_tile_sound.
        if matches!(armor, 4 | 6) {
            return 2900;
        }
        let group = usize::from(surface / 64);
        let index = usize::from(surface % 64);
        let surface = self.terrain[group].get(index).copied().unwrap_or(0);
        [2904, 2912, 2916, 2920, 2928, 2932][surface]
            + if armor == 3 && matches!(surface, 0 | 3 | 5) {
                4
            } else {
                0
            }
    }
}

fn tile_sounds(names: &Mes) -> [Vec<usize>; 4] {
    let mut sounds: [Vec<usize>; 4] = Default::default();
    for (id, value) in names.entries() {
        if (0..400).contains(&id) {
            let sound = value
                .split_ascii_whitespace()
                .nth(1)
                .and_then(|sound| sound.parse::<usize>().ok())
                .filter(|&sound| sound < 6)
                .unwrap_or(0);
            sounds[id as usize / 100].push(sound);
        }
    }
    sounds
}

fn map_scheme(vfs: &mut Vfs, map: &str) -> [i32; 2] {
    let path = format!("maps/{map}/mapinfo.txt");
    let bytes = match read_bounded(vfs, &path, 65536) {
        Ok((_, bytes)) => bytes,
        Err(content::Error::Resource {
            error: arcanum_rs::vfs::Error::NotFound(_),
            ..
        }) => return [0; 2],
        Err(error) => {
            eprintln!("Audio {path}: {error}");
            return [0; 2];
        }
    };
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| line.split_once(':'))
        .filter(|(key, _)| key.trim().eq_ignore_ascii_case("SoundScheme"))
        .filter_map(|(_, value)| {
            let (music, ambient) = value.split_once(',')?;
            Some([music.trim().parse().ok()?, ambient.trim().parse().ok()?])
        })
        .next_back()
        .unwrap_or_default()
}

type PathKey = (String, Channel);
type Encoded = Cursor<Vec<u8>>;
type CachedEffect = rodio::source::Buffered<Decoder<Encoded>>;

struct Clip {
    sink: Sink,
    key: PathKey,
    gain: f32,
    world: bool,
    looping: bool,
    transition: bool,
}

impl Clip {
    fn apply_volume(&self, levels: Volumes) {
        let gain = self.gain * self.key.1.gain(levels);
        self.sink.set_volume(gain);
        if self.looping && gain == 0. {
            self.sink.pause();
        } else {
            self.sink.play();
        }
    }
}

#[derive(Default, Serialize)]
struct Status {
    device: Option<String>,
    error: Option<String>,
    map: String,
    schemes: [i32; 2],
    hour: u8,
    paused: bool,
    loops: Vec<PathKey>,
    active_voice: Option<String>,
    recent: VecDeque<Played>,
    resource_reads: u64,
    effect_cache_hits: u64,
}

#[derive(Serialize)]
struct Played {
    path: String,
    channel: Channel,
    sound: Option<i32>,
    mount: String,
}

enum Command {
    MovieSink(mpsc::Sender<Sink>),
    Variant(i32, i32),
    Location {
        map: String,
        sector: Option<[i32; 2]>,
        hour: u8,
        reset: bool,
    },
    Volumes(Volumes),
    Voice(Option<String>),
    Pause(bool),
    MainMenu,
    Effect {
        id: i32,
        world: bool,
    },
    Footstep {
        surface: u8,
        armor: u8,
    },
}

pub(crate) struct Audio {
    queue: Option<SyncSender<Command>>,
    worker: Option<JoinHandle<()>>,
    status: Arc<Mutex<Status>>,
    stop: Arc<AtomicBool>,
    location: Option<(String, Option<[i32; 2]>, u8)>,
}

impl Audio {
    pub(crate) fn start(vfs: Vfs, levels: Volumes) -> std::io::Result<Self> {
        let (send, receive) = mpsc::sync_channel(64);
        let status = Arc::new(Mutex::new(Status::default()));
        let shared = Arc::clone(&status);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("arcanum-audio".into())
            .spawn(move || {
                if let Err(error) = Mixer::run(vfs, receive, levels, &shared, &stopped) {
                    eprintln!("Native audio unavailable: {error}");
                    shared.lock().unwrap().error = Some(error.to_string());
                }
            })?;
        Ok(Self {
            queue: Some(send),
            worker: Some(worker),
            status,
            stop,
            location: None,
        })
    }

    fn send(&self, command: Command) {
        // State changes take precedence over transient effects.
        let _ = self.queue.as_ref().unwrap().send(command);
    }

    pub(super) fn location(&mut self, map: &str, sector: Option<[i32; 2]>, hour: u8, reset: bool) {
        if reset
            || self
                .location
                .as_ref()
                .is_none_or(|(previous, s, h)| previous != map || *s != sector || *h != hour)
        {
            self.location = Some((map.to_owned(), sector, hour));
            self.send(Command::Location {
                map: map.to_owned(),
                sector,
                hour,
                reset,
            });
        }
    }

    pub(super) fn voice(&self, path: Option<&str>) {
        self.send(Command::Voice(path.map(str::to_owned)));
    }

    pub(super) fn volumes(&self, levels: Volumes) {
        self.send(Command::Volumes(levels));
    }

    pub(super) fn movie_sink(&self) -> Option<Sink> {
        let (send, receive) = mpsc::channel();
        self.send(Command::MovieSink(send));
        receive.recv().ok()
    }

    pub(super) fn main_menu(&mut self) {
        self.location = None;
        self.send(Command::MainMenu);
    }

    pub(super) fn pause(&self, paused: bool) {
        self.send(Command::Pause(paused));
    }

    pub(super) fn effect(&self, id: i32, world: bool) {
        let _ = self
            .queue
            .as_ref()
            .unwrap()
            .try_send(Command::Effect { id, world });
    }

    pub(super) fn variant(&self, preferred: i32, fallback: i32) {
        let _ = self
            .queue
            .as_ref()
            .unwrap()
            .try_send(Command::Variant(preferred, fallback));
    }

    pub(super) fn footstep(&self, surface: u8, armor: u8) {
        let _ = self
            .queue
            .as_ref()
            .unwrap()
            .try_send(Command::Footstep { surface, armor });
    }

    pub(super) fn diagnostic(&self, levels: Volumes) -> serde_json::Value {
        serde_json::json!({
            "volumes": levels,
            "gain": { "effects": Channel::Effects.gain(levels), "voice": Channel::Voice.gain(levels), "music": Channel::Music.gain(levels) },
            "output": &*self.status.lock().unwrap(),
        })
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.queue.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct Mixer<'a> {
    stream: OutputStream,
    status: &'a Mutex<Status>,
    vfs: Vfs,
    tables: Tables,
    levels: Volumes,
    map: String,
    schemes: [i32; 2],
    previous_schemes: [i32; 2],
    hour: u8,
    paused: bool,
    clips: Vec<Clip>,
    failed: BTreeSet<String>,
    cached: VecDeque<(String, String, CachedEffect)>,
    random: Random,
}

impl Mixer<'_> {
    fn run(
        mut vfs: Vfs,
        queue: Receiver<Command>,
        levels: Volumes,
        status: &Mutex<Status>,
        stop: &AtomicBool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let tables = Tables::load(&mut vfs)?;
        let mut stream = OutputStreamBuilder::open_default_stream()?;
        stream.log_on_drop(false);
        status.lock().unwrap().device = Some(format!("{:?}", stream.config()));
        let mut mixer = Mixer {
            stream,
            status,
            vfs,
            tables,
            levels,
            map: String::new(),
            schemes: [0; 2],
            previous_schemes: [0; 2],
            hour: 0,
            paused: false,
            clips: Vec::new(),
            cached: VecDeque::new(),
            failed: BTreeSet::new(),
            random: Random::fresh(),
        };
        let mut next_tick = Instant::now() + SCHEME_TICK;
        while !stop.load(Ordering::Relaxed) {
            let now = Instant::now();
            if now >= next_tick {
                mixer.tick();
                next_tick = now + SCHEME_TICK;
            }
            match queue.recv_timeout(next_tick.saturating_duration_since(now)) {
                Ok(command) => mixer.command(command),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        Ok(())
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::MovieSink(reply) => {
                let sink = Sink::connect_new(self.stream.mixer());
                sink.set_volume(Channel::Voice.gain(self.levels));
                sink.pause();
                let _ = reply.send(sink);
            }
            Command::Location {
                map,
                sector,
                hour,
                reset,
            } => {
                let entered = reset || self.map != map;
                let next = sector.unwrap_or_else(|| {
                    if entered {
                        map_scheme(&mut self.vfs, &map)
                    } else {
                        self.schemes
                    }
                });
                self.hour = hour;
                if entered {
                    self.clips
                        .retain(|clip| !clip.world || clip.looping && !reset);
                    self.map = map;
                }
                self.set_schemes(next, reset);
            }
            Command::Voice(path) => {
                self.clips.retain(|clip| clip.key.1 != Channel::Voice);
                if let Some(path) = path {
                    self.play(Request {
                        path,
                        channel: Channel::Voice,
                        volume: 100,
                        balance: 50,
                        mode: Mode::Random,
                        world: true,
                        id: None,
                    });
                }
            }
            Command::Volumes(levels) => {
                self.levels = levels;
                for clip in &self.clips {
                    clip.apply_volume(levels);
                }
            }
            Command::MainMenu => {
                self.paused = true;
                self.map.clear();
                self.clips.retain(|clip| !clip.world);
                self.set_schemes([0; 2], true);
            }
            Command::Pause(paused) => {
                self.paused = paused;
                if paused {
                    // Keep background loops playing through the menu. Discard
                    // existing world one-shots and suspend random ambient cues.
                    self.clips
                        .retain(|clip| !clip.world || clip.looping || clip.transition);
                }
            }
            Command::Variant(id, fallback) => {
                let found = self
                    .tables
                    .sounds
                    .get(&id)
                    .is_some_and(|p| self.vfs.resolve(p).is_ok_and(|r| r.winner().is_some()));
                self.effect(if found { id } else { fallback }, true);
            }
            Command::Effect { id, world } => self.effect(id, world),
            Command::Footstep { surface, armor } => {
                let id = self.tables.footstep(surface, armor) + self.roll(4) as i32;
                self.effect(id, true);
            }
        }
        self.publish();
    }

    fn set_schemes(&mut self, mut next: [i32; 2], reset: bool) {
        if next[0] == next[1] {
            next[1] = 0;
        }
        let previous = self.schemes;
        if reset || previous != next {
            self.clips.retain(|clip| !clip.transition);
            self.previous_schemes = if reset { [0; 2] } else { previous };
            self.schemes = next;
            let mut starts = Vec::new();
            for (slot, id) in next.into_iter().enumerate() {
                if reset || !previous.contains(&id) {
                    if id != 0 && !self.tables.schemes.contains_key(&id) {
                        eprintln!("Audio {}: unknown SchemeIndex ID {id}", self.map);
                    }
                    for entry in self
                        .tables
                        .schemes
                        .get(&id)
                        .into_iter()
                        .flatten()
                        .filter(|entry| matches!(entry.mode, Mode::Anchor | Mode::Over))
                    {
                        starts.push((
                            entry.path.clone(),
                            entry.channel(slot == 1),
                            entry.volume[0],
                            entry.mode,
                        ));
                    }
                }
            }
            self.reconcile_loops();
            for (path, channel, volume, mode) in starts {
                self.play(Request {
                    path,
                    channel,
                    volume,
                    balance: 50,
                    mode,
                    world: true,
                    id: None,
                });
            }
        } else {
            self.reconcile_loops();
        }
    }

    fn reconcile_loops(&mut self) {
        let mut desired = BTreeMap::new();
        for (slot, id) in self.schemes.iter().enumerate() {
            for entry in self
                .tables
                .schemes
                .get(id)
                .into_iter()
                .flatten()
                .filter(|entry| entry.mode == Mode::Loop && entry.active(self.hour))
            {
                desired
                    .entry((entry.path.clone(), entry.channel(slot == 1)))
                    .or_insert(entry.volume[0]);
            }
        }
        self.clips.retain_mut(|clip| {
            if !clip.looping {
                return true;
            }
            if let Some(volume) = desired.remove(&clip.key) {
                clip.gain = f32::from(volume) / 100.;
                clip.apply_volume(self.levels);
                true
            } else {
                false
            }
        });
        for ((path, channel), volume) in desired {
            self.play(Request {
                path,
                channel,
                volume,
                balance: 50,
                mode: Mode::Loop,
                world: true,
                id: None,
            });
        }
    }

    fn tick(&mut self) {
        let mut restore = false;
        self.clips.retain(|clip| {
            let ended = clip.sink.empty();
            restore |= clip.transition && ended;
            !ended
        });
        if restore {
            self.set_schemes(self.previous_schemes, false);
        }
        if !self.paused && self.levels.effects > 0 {
            let mut seed = self.random;
            let mut random = |limit| seed.below(limit);
            let mut cues = Vec::new();
            for id in self.schemes {
                for entry in self.tables.schemes.get(&id).into_iter().flatten() {
                    if entry.mode == Mode::Random
                        && entry.active(self.hour)
                        && random(1000) < u64::from(entry.frequency)
                    {
                        let mut between = |[min, max]: [u8; 2]| {
                            min + random(u64::from(max.saturating_sub(min)).max(1)) as u8
                        };
                        cues.push(Request {
                            path: entry.path.clone(),
                            channel: Channel::Effects,
                            volume: between(entry.volume),
                            balance: between(entry.balance),
                            mode: Mode::Random,
                            world: true,
                            id: None,
                        });
                    }
                }
            }
            self.random = seed;
            for cue in cues {
                self.play(cue);
            }
        }
        self.publish();
    }

    fn roll(&mut self, limit: u64) -> u64 {
        self.random.below(limit)
    }

    fn effect(&mut self, id: i32, world: bool) {
        if self.levels.effects == 0 || world && self.paused {
            return;
        }
        if let Some(path) = self.tables.sounds.get(&id).cloned() {
            self.play(Request {
                path,
                channel: Channel::Effects,
                volume: 100,
                balance: 50,
                mode: Mode::Random,
                world,
                id: Some(id),
            });
        }
    }

    fn play(&mut self, request: Request) {
        let path = &request.path;
        if self.failed.contains(path)
            || request.channel == Channel::Effects
                && request.mode == Mode::Random
                && self
                    .clips
                    .iter()
                    .filter(|clip| !clip.looping && !clip.sink.empty())
                    .count()
                    >= MAX_EFFECTS
        {
            return;
        }
        if let Err(error) = self.start_clip(request.clone()) {
            self.failed.insert(path.clone());
            let message = format!("{path}: {error}");
            eprintln!("Audio: {message}");
            self.status.lock().unwrap().error = Some(message);
            if request.mode == Mode::Over {
                self.set_schemes(self.previous_schemes, false);
            }
        }
    }

    fn start_clip(&mut self, request: Request) -> Result<(), Box<dyn std::error::Error>> {
        let Request {
            path,
            channel,
            volume,
            balance,
            mode,
            world,
            id,
        } = request;
        let sink = Sink::connect_new(self.stream.mixer());
        let gain = f32::from(volume) / 100.;
        let output_gain = gain * channel.gain(self.levels);
        sink.set_volume(output_gain);
        if mode == Mode::Loop && output_gain == 0. {
            sink.pause();
        }
        let pan = (f32::from(balance) - 50.) / 50.;
        let stereo = || vec![1. - pan.max(0.), 1. + pan.min(0.)];
        let mount;
        if channel == Channel::Effects
            && mode == Mode::Random
            && let Some(index) = self.cached.iter().position(|(key, _, _)| *key == path)
        {
            let cached = self.cached.remove(index).unwrap();
            mount = cached.1.clone();
            if balance == 50 {
                sink.append(cached.2.clone());
            } else {
                sink.append(ChannelVolume::new(cached.2.clone(), stereo()));
            }
            self.cached.push_back(cached);
            self.status.lock().unwrap().effect_cache_hits += 1;
        } else {
            let (source, bytes) = read_bounded(&mut self.vfs, &path, MAX_CLIP_BYTES)?;
            mount = source.to_string();
            let len = bytes.len() as u64;
            let decoder = Decoder::builder()
                .with_data(Cursor::new(bytes))
                .with_byte_len(len);
            if mode == Mode::Loop {
                sink.append(decoder.build_looped()?.fade_in(FADE));
            } else {
                let decoder = decoder.build()?;
                if channel == Channel::Effects
                    && mode == Mode::Random
                    && decoder
                        .total_duration()
                        .is_some_and(|time| time <= MAX_CACHED_EFFECT_DURATION)
                {
                    let effect = decoder.buffered();
                    if balance == 50 {
                        sink.append(effect.clone());
                    } else {
                        sink.append(ChannelVolume::new(effect.clone(), stereo()));
                    }
                    self.cached.push_back((path.clone(), mount.clone(), effect));
                    if self.cached.len() > MAX_CACHED_EFFECTS {
                        self.cached.pop_front();
                    }
                } else if mode == Mode::Random {
                    if balance == 50 {
                        sink.append(decoder);
                    } else {
                        sink.append(ChannelVolume::new(decoder, stereo()));
                    }
                } else {
                    sink.append(decoder.fade_in(FADE));
                }
            }
        }
        self.clips
            .retain(|clip| clip.transition || !clip.sink.empty());
        self.clips.push(Clip {
            sink,
            gain,
            key: (path.clone(), channel),
            world,
            looping: mode == Mode::Loop,
            transition: mode == Mode::Over,
        });
        let mut status = self.status.lock().unwrap();
        if status.recent.len() == 32 {
            status.recent.pop_front();
        }
        status.recent.push_back(Played {
            path,
            channel,
            sound: id,
            mount,
        });
        Ok(())
    }

    fn publish(&self) {
        let mut status = self.status.lock().unwrap();
        if status.map != self.map {
            status.map.clone_from(&self.map);
        }
        status.schemes = self.schemes;
        status.hour = self.hour;
        status.paused = self.paused;
        status.resource_reads = self.vfs.read_attempts();
        status.loops.clear();
        status.active_voice = self
            .clips
            .iter()
            .find(|clip| clip.key.1 == Channel::Voice && !clip.sink.empty())
            .map(|clip| clip.key.0.clone());
        status.loops.extend(
            self.clips
                .iter()
                .filter(|clip| clip.looping && !clip.sink.empty())
                .map(|clip| clip.key.clone()),
        );
    }
}

#[derive(Clone)]
struct Request {
    path: String,
    channel: Channel,
    volume: u8,
    balance: u8,
    mode: Mode,
    world: bool,
    id: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_scheme_windows_and_scatter() {
        let sounds = BTreeMap::from([(30, "sound/bell.wav".into())]);
        let night = Entry::parse("VILLAGE_NIGHT.WAV /LOOP /TIME:19-0", &sounds).unwrap();
        assert_eq!(night.mode, Mode::Loop);
        assert_eq!(night.path, "sound/village_night.wav");
        assert!(night.active(23) && night.active(0) && !night.active(12));
        let cue = Entry::parse("#30# /scatter:80 /freq:30 /time:6,8", &sounds).unwrap();
        assert_eq!(cue.path, "sound/bell.wav");
        assert_eq!(
            (cue.volume, cue.balance, cue.frequency),
            ([20, 100], [10, 90], 30)
        );
        assert!(cue.active(6) && cue.active(8) && !cue.active(9));
    }

    #[test]
    fn terrain_sound_codes_follow_art_groups_and_authored_material() {
        let tiles = Mes::parse(
            b"{0}{drt/n 0}
{100}{wk1 3}
{200}{snw/i 2}
{300}{plk 5}",
        )
        .unwrap();
        let tables = Tables {
            terrain: tile_sounds(&tiles),
            sounds: Default::default(),
            schemes: Default::default(),
        };
        assert_eq!(tables.footstep(64, 0), 2920);
        assert_eq!(tables.footstep(192, 3), 2936);
        assert_eq!(tables.footstep(128, 4), 2900);
    }

    #[test]
    fn each_mixer_channel_has_independent_gain_and_mute() {
        let levels = Volumes {
            effects: 0,
            voice: 5,
            music: 10,
        };
        assert_eq!(
            [Channel::Effects, Channel::Voice, Channel::Music].map(|bus| bus.gain(levels)),
            [0., 0.5, 0.8]
        );
    }
}
