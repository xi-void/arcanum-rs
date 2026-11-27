//! Original movie IDs and Bink packets feed the existing surface and audio
//! output. Only compressed data from archive mounts requires a memory copy.
use super::*;
use arcanum_rs::{
    content::{load_mes, read_bounded},
    vfs::Vfs,
};
use infinitier_bik_decoder::{AudioDecoder, VideoDecoder, VideoFrame, parse_header};
use rodio::{
    Sink,
    buffer::SamplesBuffer,
    queue::{self, SourcesQueueInput},
};
use std::{
    collections::VecDeque,
    io::{Cursor, Read, Seek, SeekFrom},
    sync::mpsc::{self, Receiver, TryRecvError},
};

trait ClipReader: Read + Seek + Send {}
impl<T: Read + Seek + Send> ClipReader for T {}

struct Packet {
    pixels: Vec<u8>,
    samples: Vec<i16>,
}

fn rgba(image: &VideoFrame, w: usize, h: usize) -> Vec<u8> {
    let mut result = vec![0; w * h * 4];
    for (y, line) in result.chunks_exact_mut(4 * w).enumerate() {
        for (x, pixel) in line.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let l = i32::from(image.y.data[y * image.y.stride + x]) * 256;
            let u = i32::from(image.u.data[(y / 2) * image.u.stride + x / 2]) - 128;
            let v = i32::from(image.v.data[(y / 2) * image.v.stride + x / 2]) - 128;
            let component = |c: i32| ((l + c + 128) >> 8).clamp(0, 255) as u8;
            pixel.copy_from_slice(&[
                component(359 * v),
                component(-88 * u - 183 * v),
                component(454 * u),
                255,
            ]);
        }
    }
    result
}

struct Stream {
    input: Box<dyn ClipReader>,
    header: infinitier_bik_decoder::BikHeader,
    video: VideoDecoder,
    audio: Option<AudioDecoder>,
    track: usize,
}
impl Stream {
    fn resource(vfs: &mut Vfs, path: &str, track: usize) -> Result<(String, Self), String> {
        let resolution = vfs.resolve(path).map_err(|e| e.to_string())?;
        let winner = resolution
            .winner()
            .ok_or_else(|| format!("Missing movie {path}"))?;
        let source = format!("{} ({})", resolution.path, winner.mount.path.display());
        let input: Box<dyn ClipReader> = match vfs.open_loose(path).map_err(|e| e.to_string())? {
            Some(file) => Box::new(std::io::BufReader::new(file)),
            None => Box::new(Cursor::new(
                read_bounded(vfs, path, 256 * 1024 * 1024)
                    .map_err(|e| e.to_string())?
                    .1,
            )),
        };
        Ok((source, Self::open(input, track)?))
    }

    fn open(mut input: Box<dyn ClipReader>, track: usize) -> Result<Self, String> {
        let length = input.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
        input.rewind().map_err(|e| e.to_string())?;
        let mut fixed = [0; 44];
        input.read_exact(&mut fixed).map_err(|e| e.to_string())?;
        let tracks = u32::from_le_bytes(fixed[40..44].try_into().unwrap());
        if 44 + u64::from(tracks) * 12 > length || tracks > 256 {
            return Err("Invalid Bink track table".into());
        }
        let header = parse_header(&mut input).map_err(|e| e.to_string())?;
        if u64::from(header.width) * u64::from(header.height) > MAX_SURFACE_PIXELS as u64
            || header.max_frame_size > 128 * 1024 * 1024
            || header.file_size > length
            || header
                .frames
                .iter()
                .any(|f| u64::from(f.pos) + u64::from(f.size) > length)
        {
            return Err("Invalid Bink frame dimensions or index".into());
        }
        if track >= header.audio_tracks.len() && !header.audio_tracks.is_empty() {
            return Err(format!("Missing Bink track {track}"));
        }
        let video = VideoDecoder::new(&header).map_err(|e| e.to_string())?;
        let audio = header
            .audio_tracks
            .get(track)
            .map(AudioDecoder::new)
            .transpose()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            input,
            header,
            video,
            audio,
            track,
        })
    }
    fn decode(mut self, mut emit: impl FnMut(Packet) -> bool) -> Result<(), String> {
        let mut buffer = Vec::new();
        for frame in &self.header.frames {
            self.input
                .seek(SeekFrom::Start(u64::from(frame.pos)))
                .map_err(|e| e.to_string())?;
            buffer.resize(frame.size as usize, 0);
            self.input
                .read_exact(&mut buffer)
                .map_err(|e| e.to_string())?;
            let mut payload = &buffer[..];
            let mut samples = Vec::new();
            for i in 0..self.header.audio_tracks.len() {
                let (length, rest) = payload
                    .split_at_checked(4)
                    .ok_or("Incomplete Bink audio header")?;
                let (audio, rest) = rest
                    .split_at_checked(u32::from_le_bytes(length.try_into().unwrap()) as usize)
                    .ok_or("Incomplete Bink audio packet")?;
                if i == self.track && !audio.is_empty() {
                    samples = self
                        .audio
                        .as_mut()
                        .unwrap()
                        .decode_packet(audio)
                        .map_err(|e| e.to_string())?;
                }
                payload = rest;
            }
            let image = self
                .video
                .decode_frame(payload)
                .map_err(|e| e.to_string())?;
            let pixels = rgba(
                image,
                self.header.width as usize,
                self.header.height as usize,
            );
            if !emit(Packet { pixels, samples }) {
                break;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Destination {
    Menu,
    Game,
    Credits,
}

#[derive(Clone, Copy, serde::Serialize)]
#[serde(untagged)]
pub(super) enum Clip {
    Number(i32),
    Path(&'static str),
}
impl std::fmt::Display for Clip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(id) => write!(f, "{id}"),
            Self::Path(path) => f.write_str(path),
        }
    }
}

pub(super) struct Playback {
    source: String,
    id: Clip,
    pending: VecDeque<Clip>,
    after: Destination,
    packets: Receiver<Result<Packet, String>>,
    buffer: VecDeque<Vec<u8>>,
    asset: Arc<LoadedFrame>,
    period: Duration,
    count: u32,
    presented: u32,
    ended: bool,
    clock: Option<Instant>,
    audio_progress: Option<(Duration, Instant)>,
    audio: Option<(Sink, Arc<SourcesQueueInput>)>,
    rate: u32,
    channels: u16,
    samples: u64,
}

impl Playback {
    fn open(vfs: &mut Vfs, id: Clip, audio: Option<&Audio>) -> Result<Self, String> {
        let path = match id {
            Clip::Number(number) => {
                let table = load_mes(vfs, "movies/movies.mes").map_err(|e| e.to_string())?;
                let name = table
                    .mes
                    .get(number)
                    .ok_or_else(|| format!("Missing movie {id} in {}", table.source))?;
                format!("movies/{}", name.trim())
            }
            Clip::Path(path) => path.to_owned(),
        };
        let (source, stream) = Stream::resource(vfs, &path, 0)?;
        let (w, h) = (stream.header.width, stream.header.height);
        let period = Duration::from_secs_f64(1. / stream.header.fps());
        let count = stream.header.frame_count;
        let (rate, channels) = stream
            .header
            .audio_tracks
            .first()
            .map_or((0, 0), |t| (u32::from(t.sample_rate), t.flags.channels()));
        let audio = audio
            .filter(|_| rate > 0)
            .and_then(Audio::movie_sink)
            .map(|sink| {
                let (queue, samples) = queue::queue(true);
                sink.append(samples);
                (sink, queue)
            });
        let asset = hud::generated_frame(
            &path,
            SpriteFrame {
                width: w,
                height: h,
                hotspot: (0, 0),
                offset: (0, 0),
                pixels: Vec::new(),
            },
        );
        let (send, packets) = mpsc::sync_channel(2);
        std::thread::Builder::new()
            .name("arcanum-bink".into())
            .spawn(move || {
                if let Err(e) = stream.decode(|packet| send.send(Ok(packet)).is_ok()) {
                    let _ = send.send(Err(e));
                }
            })
            .map_err(|e| e.to_string())?;
        eprintln!(
            "Cinematic {id} {source}, {w}x{h}, {count} frames, {rate} Hz, {channels} channels"
        );
        Ok(Self {
            source,
            id,
            pending: Default::default(),
            after: Destination::Menu,
            packets,
            buffer: Default::default(),
            asset,
            period,
            count,
            presented: 0,
            ended: false,
            clock: None,
            audio_progress: None,
            audio,
            rate,
            channels,
            samples: 0,
        })
    }

    fn slide(vfs: &mut Vfs, id: i32, audio: Option<&Audio>) -> Result<Self, String> {
        let table = load_mes(vfs, "rules/slide.mes").map_err(|e| e.to_string())?;
        let mut paths = table
            .mes
            .get(id)
            .ok_or_else(|| format!("Missing slide {id}"))?
            .split_whitespace();
        let path = format!("slide/{}", paths.next().ok_or("Missing slide bitmap")?);
        let (source, data) =
            read_bounded(vfs, &path, 32 * 1024 * 1024).map_err(|e| e.to_string())?;
        let bmp = arcanum_rs::content::map_atlas::decode_bmp(&data)
            .map_err(|e| format!("{source}: {e}"))?;
        let audio = audio.and_then(|a| {
            let path = format!("sound/speech/slide/{}", paths.next()?);
            let (source, bytes) = read_bounded(vfs, &path, 64 * 1024 * 1024)
                .map_err(|e| eprintln!("Slide audio: {e}"))
                .ok()?;
            let voice = rodio::Decoder::try_from(Cursor::new(bytes))
                .map_err(|e| eprintln!("Slide audio {source}: {e}"))
                .ok()?;
            let sink = a.movie_sink()?;
            let (queue, output) = queue::queue(false);
            queue.append(voice);
            sink.append(output);
            Some((sink, queue))
        });
        let asset = hud::generated_frame(
            &path,
            SpriteFrame {
                width: bmp.size.0,
                height: bmp.size.1,
                hotspot: (0, 0),
                offset: (0, 0),
                pixels: Vec::new(),
            },
        );
        let (send, packets) = mpsc::sync_channel(1);
        send.send(Ok(Packet {
            pixels: bmp.pixels.into_flattened(),
            samples: Vec::new(),
        }))
        .unwrap();
        Ok(Self {
            source: source.to_string(),
            id: Clip::Number(id),
            pending: Default::default(),
            after: Destination::Credits,
            packets,
            buffer: Default::default(),
            asset,
            period: Duration::from_secs(3),
            count: 1,
            presented: 0,
            ended: false,
            clock: None,
            audio_progress: None,
            audio,
            rate: 0,
            channels: 0,
            samples: 0,
        })
    }
    fn elapsed(&self) -> Duration {
        self.clock.map_or(Duration::ZERO, |start| {
            self.audio
                .as_ref()
                .filter(|(sink, _)| !sink.empty())
                .map_or_else(|| start.elapsed(), |(sink, _)| sink.get_pos())
        })
    }
    fn update(&mut self, renderer: &mut gpu::WindowRenderer) -> Result<bool, String> {
        while self.buffer.len() < 3 && !self.ended {
            match self.packets.try_recv() {
                Ok(packet) => {
                    let packet = packet?;
                    self.samples += packet.samples.len() as u64;
                    if let Some((_, queue)) = &self.audio
                        && !packet.samples.is_empty()
                    {
                        queue.append(SamplesBuffer::new(
                            self.channels,
                            self.rate,
                            packet
                                .samples
                                .into_iter()
                                .map(|s| f32::from(s) / 32768.)
                                .collect::<Vec<_>>(),
                        ));
                    }
                    self.buffer.push_back(packet.pixels);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.ended = true;
                    if self.presented as usize + self.buffer.len() != self.count as usize {
                        return Err("Movie decoder stopped before the final frame".into());
                    }
                }
            }
        }
        if self.clock.is_some()
            && let Some((sink, _)) = &self.audio
            && !sink.empty()
        {
            let position = sink.get_pos();
            let (previous, changed) = self
                .audio_progress
                .get_or_insert((position, Instant::now()));
            if position != *previous {
                *previous = position;
                *changed = Instant::now();
            } else if changed.elapsed() >= Duration::from_secs(2) {
                eprintln!(
                    "Cinematic {}: audio device stopped advancing at {position:?}",
                    self.id
                );
                self.audio = None;
                self.clock = Instant::now().checked_sub(position);
            }
        }
        let elapsed = self.elapsed();
        let mut current = None;
        while elapsed >= self.period * self.presented
            && let Some(next) = self.buffer.pop_front()
        {
            current = Some(next);
            self.presented += 1;
        }
        if let Some(frame) = current {
            renderer.stream_rgba(&self.asset, &frame)?;
            if self.clock.is_none() {
                self.clock = Some(Instant::now());
                if let Some((sink, _)) = &self.audio {
                    sink.play();
                }
            }
        }
        Ok(self.ended
            && self.presented == self.count
            && elapsed >= self.period * self.count
            && (self.after != Destination::Credits
                || self.audio.as_ref().is_none_or(|(s, _)| s.empty())))
    }
    pub(super) fn next_tick(&self) -> Duration {
        if self.after == Destination::Credits && self.clock.is_some() {
            return Duration::from_millis(125);
        }
        if self.clock.is_none() {
            return Duration::from_millis(5);
        }
        (self.period * self.presented)
            .saturating_sub(self.elapsed())
            .clamp(
                Duration::from_millis(4),
                self.period.max(Duration::from_millis(4)),
            )
    }
    fn draw(&self, size: (u32, u32)) -> Option<OverlayDraw<'_>> {
        if self.presented == 0 {
            return None;
        }
        let frame = &self.asset.decoded.frame;
        let scale = (size.0 as f32 / frame.width as f32).min(size.1 as f32 / frame.height as f32);
        let (w, h) = (frame.width as f32 * scale, frame.height as f32 * scale);
        let rect = [(size.0 as f32 - w) * 0.5, (size.1 as f32 - h) * 0.5, w, h];
        Some(OverlayDraw {
            asset: &self.asset,
            rect,
            clip: rect,
        })
    }
    pub(super) fn diagnostic(&self) -> serde_json::Value {
        serde_json::json!({"id":self.id,"resource":self.source,"frame":self.presented,"frame_count":self.count,"elapsed_ms":self.elapsed().as_millis(),"audio_output":self.audio.is_some(),"pcm_samples":self.samples,"sample_rate":self.rate,"channels":self.channels,"pending":self.pending})
    }
}

impl Viewer<'_> {
    pub(super) fn play_movies(
        &mut self,
        ids: VecDeque<i32>,
        start_game: bool,
        events: &ActiveEventLoop,
    ) {
        self.play_sequence(
            ids.into_iter().map(Clip::Number).collect(),
            if start_game {
                Destination::Game
            } else {
                Destination::Menu
            },
            events,
        )
    }

    pub(super) fn play_sequence(
        &mut self,
        mut ids: VecDeque<Clip>,
        after: Destination,
        events: &ActiveEventLoop,
    ) {
        self.movie = None;
        if let ViewContent::Stream {
            inventory_reader: Some(reader),
            ..
        } = &mut self.content
        {
            while let Some(id) = ids.pop_front() {
                let opened = match (after, id) {
                    (Destination::Credits, Clip::Number(n)) => {
                        Playback::slide(reader.resources(), n, self.audio.as_ref())
                    }
                    _ => Playback::open(reader.resources(), id, self.audio.as_ref()),
                };
                match opened {
                    Ok(mut movie) => {
                        movie.pending = ids;
                        movie.after = after;
                        self.movie = Some(movie);
                        if let Some(view) = &self.view {
                            view.window.set_cursor_visible(false);
                            view.window.request_redraw();
                        }
                        self.update_control_flow(events);
                        return;
                    }
                    Err(e) => eprintln!("Skipping cinematic {id}: {e}"),
                }
            }
        }
        if let Some(view) = &self.view {
            view.window.set_cursor_visible(true);
            view.window.request_redraw();
        }
        if after == Destination::Game {
            self.menu_action(MenuCommand::EnterGame, events);
        }
        self.update_control_flow(events);
    }

    pub(super) fn movie_event(&mut self, event: WindowEvent, events: &ActiveEventLoop) {
        let view = self.view.as_mut().unwrap();
        let movie = self.movie.as_mut().unwrap();
        let reason = match event {
            WindowEvent::CloseRequested => {
                events.exit();
                return;
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state.is_pressed()
                    && !event.repeat
                    && event.logical_key == Key::Named(NamedKey::Escape) =>
            {
                if movie.after == Destination::Credits {
                    movie.pending.clear();
                }
                "skipped"
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                ..
            } if movie.after == Destination::Credits => "advanced",
            WindowEvent::KeyboardInput { event, .. }
                if movie.after == Destination::Credits
                    && event.state.is_pressed()
                    && !event.repeat =>
            {
                "advanced"
            }
            WindowEvent::Resized(_) => {
                view.window.request_redraw();
                return;
            }
            WindowEvent::RedrawRequested => {
                let size = view.window.inner_size();
                match movie.update(&mut view.renderer) {
                    Ok(false) => {
                        if size.width > 0 && size.height > 0 {
                            let size = (size.width, size.height);
                            if let Err(e) = view.renderer.redraw(
                                (size, 1),
                                [0; 3],
                                std::iter::empty(),
                                movie.draw(size),
                                &view.window,
                                false,
                            ) {
                                self.fail(events, e);
                                return;
                            }
                            view.submitted_frames += 1;
                        }
                        self.update_control_flow(events);
                        return;
                    }
                    Ok(true) => "completed",
                    Err(e) => {
                        eprintln!("Cinematic playback: {e}");
                        "failed"
                    }
                }
            }
            _ => return,
        };
        eprintln!(
            "Cinematic {} {reason} at frame {}/{}",
            movie.id, movie.presented, movie.count
        );
        let ids = std::mem::take(&mut movie.pending);
        let after = movie.after;
        self.play_sequence(ids, after, events);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires the installed original Bink movies"]
    fn installed_bink_movies_decode_to_the_end_with_audio() {
        let root = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let mut vfs = Vfs::from_game(&root, arcanum_rs::vfs::Policy::Ce).unwrap();
        let table = load_mes(&mut vfs, "movies/movies.mes").unwrap();
        for (id, name) in table.mes.entries().filter(|(id, _)| *id > 0) {
            let (_, stream) =
                Stream::resource(&mut vfs, &format!("movies/{}", name.trim()), 0).unwrap();
            let count = stream.header.frame_count;
            let length = (stream.header.width * stream.header.height * 4) as usize;
            let mut frames = 0;
            let mut samples = 0;
            let mut audible = false;
            stream
                .decode(|packet| {
                    assert_eq!(packet.pixels.len(), length);
                    frames += 1;
                    samples += packet.samples.len();
                    audible |= packet.samples.iter().any(|s| s.unsigned_abs() > 256);
                    true
                })
                .unwrap();
            assert_eq!(frames, count, "movie {id}");
            assert!(audible, "movie {id}");
            eprintln!("Movie {id}: {frames} frames, {samples} decoded PCM samples");
        }
    }
}
