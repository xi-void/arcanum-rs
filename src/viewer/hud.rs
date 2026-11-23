//! Declarative HUD layout, assets and hit testing, in physical window pixels.
use arcanum_rs::{
    art::{DecodedFrame, SpriteFrame},
    compositor::SpriteDraw,
    content::{
        Source,
        art::{LoadedFrame, load_frame},
    },
    vfs::{MountId, Policy, Storage, Vfs},
};
use serde::Deserialize;
use std::{collections::BTreeMap, error::Error, fs::File, io::Read, path::Path, sync::Arc};

type Color = [u8; 4];

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Action {
    Pause,
    ToggleRun,
    ResetZoom,
}

#[derive(Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
enum Anchor {
    #[default]
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Theme {
    name: String,
    author: Option<String>,
    #[serde(default = "default_scale")]
    scale: u16,
    #[serde(default)]
    fonts: BTreeMap<String, Font>,
    #[serde(default)]
    hud: Vec<Element>,
}

fn default_scale() -> u16 {
    100
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Font {
    source: String,
    size: u32,
    color: Color,
    #[serde(default)]
    bold: bool,
    #[serde(default)]
    line_gap: u32,
    fallback: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Element {
    id: String,
    parent: Option<String>,
    #[serde(default)]
    anchor: Anchor,
    #[serde(default)]
    offset: [i32; 2],
    size: [u32; 2],
    image: Option<String>,
    source_rect: Option<[u32; 4]>,
    #[serde(default)]
    frame: usize,
    fill: Option<Color>,
    hover_fill: Option<Color>,
    text: Option<String>,
    font: Option<String>,
    #[serde(default)]
    align: Align,
    #[serde(default)]
    wrap: bool,
    #[serde(default)]
    capture: bool,
    action: Option<Action>,
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    hidden: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

impl Rect {
    fn contains(self, (x, y): (u32, u32)) -> bool {
        let (x, y) = (
            i64::from(x) - i64::from(self.x),
            i64::from(y) - i64::from(self.y),
        );
        x >= 0 && y >= 0 && x < i64::from(self.w) && y < i64::from(self.h)
    }
    fn intersect(self, other: Self) -> Option<Self> {
        let (left, top) = (self.x.max(other.x), self.y.max(other.y));
        let right = (self.x + self.w as i32).min(other.x + other.w as i32);
        let bottom = (self.y + self.h as i32).min(other.y + other.h as i32);
        (left < right && top < bottom).then(|| Self {
            x: left,
            y: top,
            w: (right - left) as u32,
            h: (bottom - top) as u32,
        })
    }
}

struct Component {
    config: Element,
    parent: Option<usize>,
    base: SpriteFrame,
    text: Option<Text>,
}

struct Text {
    glyphs: Vec<Option<(Arc<SpriteFrame>, char)>>,
    font: Font,
    native_height: u32,
}

struct Placement {
    rect: Rect,
    art: Arc<LoadedFrame>,
    hover_art: Option<Arc<LoadedFrame>>,
}

pub(crate) struct Hud {
    components: Vec<Component>,
    placed: Vec<Option<Placement>>,
    size: (u32, u32),
    scale: u16,
    hover: Option<usize>,
    focus: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Hit {
    World,
    Panel,
    Button(Action),
}

impl Hud {
    pub fn load(path: &Path, game: &mut Vfs, scale: Option<u16>) -> Result<Self, Box<dyn Error>> {
        let mut source = String::new();
        File::open(path)?
            .take(128 * 1024 + 1)
            .read_to_string(&mut source)?;
        let theme = parse(&source)?;
        let mut files = Vfs::new(Policy::Ordered);
        files.mount_directory(
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )?;
        let scale = scale.unwrap_or(theme.scale);
        if !(50..=300).contains(&scale) {
            return Err("UI scale must be 50–300 percent".into());
        }
        let mut components = Vec::new();
        let mut indices = BTreeMap::new();
        let mut glyphs = BTreeMap::new();
        for config in theme.hud {
            let index = components.len();
            let parent = config
                .parent
                .as_deref()
                .map(|id| {
                    indices
                        .get(id)
                        .copied()
                        .ok_or_else(|| format!("{}: unknown or forward parent {id}", config.id))
                })
                .transpose()?;
            if indices.insert(config.id.clone(), index).is_some() {
                return Err(format!("duplicate UI component {}", config.id).into());
            }
            let mut base = sprite(config.size, config.fill.unwrap_or([0; 4]));
            let mut prepared_text = None;
            if !config.hidden {
                if let Some(asset) = &config.image {
                    let art = resource(game, &mut files, asset, config.frame)
                        .map_err(|error| format!("{}: {error}", config.id))?;
                    scale_into(&art.decoded.frame, &mut base, config.source_rect)
                        .map_err(|error| format!("{}: {error}", config.id))?;
                }
                if let Some(text) = &config.text {
                    let id = config
                        .font
                        .as_deref()
                        .ok_or_else(|| format!("{}: text requires a font", config.id))?;
                    let font = theme
                        .fonts
                        .get(id)
                        .ok_or_else(|| format!("{}: unknown font {id}", config.id))?;
                    let mut glyph = |ch| {
                        let key = (id.to_string(), ch);
                        if let std::collections::btree_map::Entry::Vacant(entry) =
                            glyphs.entry(key.clone())
                        {
                            let mask = font_glyph(game, &mut files, font, ch)
                                .or_else(|error| {
                                    let Some(fallback) = &font.fallback else {
                                        return Err(error);
                                    };
                                    font_glyph(game, &mut files, &theme.fonts[fallback], ch)
                                })
                                .map_err(|error| format!("{}: {error}", config.id))?;
                            entry.insert(Arc::new(mask));
                        }
                        Ok::<_, Box<dyn Error>>(Arc::clone(&glyphs[&key]))
                    };
                    let glyphs: Vec<_> = text
                        .chars()
                        .map(|ch| {
                            if ch == '\n' {
                                Ok(None)
                            } else {
                                glyph(ch).map(|frame| Some((frame, ch)))
                            }
                        })
                        .collect::<Result<_, _>>()?;
                    let native_height = glyph(' ').map_or_else(
                        |_| {
                            glyphs
                                .iter()
                                .flatten()
                                .next()
                                .map_or(font.size, |(g, _)| g.height)
                        },
                        |space| space.height,
                    );
                    prepared_text = Some(Text {
                        glyphs,
                        font: font.clone(),
                        native_height,
                    });
                }
            }
            components.push(Component {
                config,
                parent,
                base,
                text: prepared_text,
            });
        }
        if let Some(author) = &theme.author {
            eprintln!("HUD author: {author}");
        }
        eprintln!("HUD {} from {} at {}%", theme.name, path.display(), scale);
        Ok(Self {
            components,
            placed: Vec::new(),
            size: (0, 0),
            scale,
            hover: None,
            focus: None,
        })
    }

    pub fn layout(&mut self, size: (u32, u32)) {
        if size == self.size {
            return;
        }
        self.size = size;
        self.placed.clear();
        let viewport = Rect {
            x: 0,
            y: 0,
            w: size.0,
            h: size.1,
        };
        let widest = self
            .components
            .iter()
            .filter(|c| c.parent.is_none() && !c.config.hidden)
            .map(|c| c.config.size[0])
            .max()
            .unwrap_or(1);
        // Keep wide stock panels usable on narrow windows. Both hit testing and
        // raster placement use this exact effective scale.
        let scale = u32::from(self.scale)
            .min(size.0.saturating_mul(100) / widest)
            .max(1);
        let mut bounds = Vec::with_capacity(self.components.len());
        for component in &self.components {
            let parent = component
                .parent
                .map_or(Some((viewport, viewport)), |id| bounds[id]);
            let placement = parent.and_then(|(parent, clip)| {
                if component.config.hidden {
                    return None;
                }
                let rect = layout_rect(&component.config, parent, scale);
                Some((rect, rect.intersect(clip)?))
            });
            bounds.push(placement);
            self.placed.push(placement.map(|(rect, clip)| {
                let art = raster(component, rect, clip, scale, None);
                let hover_art = component
                    .config
                    .hover_fill
                    .filter(|_| !component.config.disabled && component.config.action.is_some())
                    .map(|color| raster(component, rect, clip, scale, Some(color)));
                Placement {
                    rect: clip,
                    art,
                    hover_art,
                }
            }));
        }
    }

    pub fn adjust_scale(&mut self, step: i16) {
        self.scale = (self.scale as i16 + step).clamp(50, 300) as u16;
        let size = std::mem::take(&mut self.size);
        self.layout(size);
    }

    fn select(&self, point: (u32, u32)) -> Option<usize> {
        self.placed
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, placement)| {
                let config = &self.components[index].config;
                (placement.as_ref()?.rect.contains(point)
                    && (config.capture || config.action.is_some()))
                .then_some(index)
            })
    }
    pub(super) fn hit(&self, point: (u32, u32)) -> Hit {
        self.select(point).map_or(Hit::World, |index| {
            let config = &self.components[index].config;
            config
                .action
                .filter(|_| !config.disabled)
                .map_or(Hit::Panel, Hit::Button)
        })
    }
    pub fn hover(&mut self, point: Option<(u32, u32)>) -> bool {
        let hover = point.and_then(|point| self.select(point)).filter(|&i| {
            self.placed[i]
                .as_ref()
                .is_some_and(|p| p.hover_art.is_some())
        });
        std::mem::replace(&mut self.hover, hover) != hover
    }
    pub fn advance_focus(&mut self, backwards: bool) {
        let len = self.components.len();
        if len == 0 {
            return;
        }
        let start = self.focus.unwrap_or(if backwards { 0 } else { len - 1 });
        self.focus = (1..=len)
            .map(|step| {
                if backwards {
                    (start + len - step) % len
                } else {
                    (start + step) % len
                }
            })
            .find(|&index| self.available_action(index).is_some());
    }

    pub(super) fn focused_action(&self) -> Option<Action> {
        self.available_action(self.focus?)
    }

    fn available_action(&self, index: usize) -> Option<Action> {
        self.placed.get(index)?.as_ref()?;
        let config = &self.components[index].config;
        config.action.filter(|_| !config.disabled)
    }

    pub fn draws(
        &self,
        running: bool,
    ) -> impl Iterator<Item = (&Arc<LoadedFrame>, SpriteDraw<'_>)> {
        self.placed
            .iter()
            .enumerate()
            .filter_map(move |(index, placement)| {
                let p = placement.as_ref()?;
                let asset = if self.hover == Some(index)
                    || self.focus == Some(index)
                    || (running && self.components[index].config.action == Some(Action::ToggleRun))
                {
                    p.hover_art.as_ref().unwrap_or(&p.art)
                } else {
                    &p.art
                };
                Some((
                    asset,
                    SpriteDraw {
                        frame: &asset.decoded.frame,
                        anchor: (p.rect.x, p.rect.y),
                    },
                ))
            })
    }
}

fn parse(source: &str) -> Result<Theme, Box<dyn Error>> {
    if source.len() > 128 * 1024 {
        return Err("theme exceeds 128 KiB".into());
    }
    let theme: Theme = toml::from_str(source)?;
    if theme.name.trim().is_empty()
        || theme.hud.len() > 64
        || theme.fonts.len() > 16
        || !(50..=300).contains(&theme.scale)
    {
        return Err("invalid theme name, scale or component count".into());
    }
    let mut pixels = 0;
    for e in &theme.hud {
        if e.id.is_empty()
            || e.size.iter().any(|&v| !(1..=1024).contains(&v))
            || e.offset.iter().any(|v| v.unsigned_abs() > 4096)
            || (e.source_rect.is_some() && e.image.is_none())
            || e.text.as_ref().is_some_and(|s| s.len() > 256)
        {
            return Err(format!("invalid size, offset or text in component {}", e.id).into());
        }
        pixels += e.size[0] * e.size[1];
    }
    if pixels > 512 * 1024
        || theme.fonts.values().any(|font| {
            !(5..=48).contains(&font.size)
                || font.line_gap > 32
                || font
                    .fallback
                    .as_ref()
                    .is_some_and(|f| !theme.fonts.contains_key(f))
        })
    {
        return Err("theme exceeds image bounds or has an invalid font profile".into());
    }
    Ok(theme)
}

fn resource(
    game: &mut Vfs,
    files: &mut Vfs,
    asset: &str,
    frame: usize,
) -> Result<LoadedFrame, Box<dyn Error>> {
    let (scope, path) = asset
        .split_once(':')
        .ok_or("asset must start with vfs: or file:")?;
    let vfs = match scope {
        "vfs" => game,
        "file" => files,
        _ => return Err("unknown asset scope".into()),
    };
    Ok(load_frame(vfs, path, frame)?)
}

fn layout_rect(e: &Element, parent: Rect, s: u32) -> Rect {
    let w = (e.size[0] * s / 100).max(1);
    let h = (e.size[1] * s / 100).max(1);
    let horizontal = match e.anchor {
        Anchor::TopLeft | Anchor::Left | Anchor::BottomLeft => 0,
        Anchor::Top | Anchor::Center | Anchor::Bottom => 1,
        _ => 2,
    };
    let vertical = match e.anchor {
        Anchor::TopLeft | Anchor::Top | Anchor::TopRight => 0,
        Anchor::Left | Anchor::Center | Anchor::Right => 1,
        _ => 2,
    };
    Rect {
        x: parent.x + (parent.w as i32 - w as i32) * horizontal / 2 + e.offset[0] * s as i32 / 100,
        y: parent.y + (parent.h as i32 - h as i32) * vertical / 2 + e.offset[1] * s as i32 / 100,
        w,
        h,
    }
}
fn sprite([width, height]: [u32; 2], color: Color) -> SpriteFrame {
    SpriteFrame {
        width,
        height,
        hotspot: (0, 0),
        offset: (0, 0),
        pixels: vec![color; (width * height) as usize],
    }
}
fn scale_into(
    src: &SpriteFrame,
    dst: &mut SpriteFrame,
    crop: Option<[u32; 4]>,
) -> Result<(), &'static str> {
    let [left, top, width, height] = crop.unwrap_or([0, 0, src.width, src.height]);
    if width == 0
        || height == 0
        || left.saturating_add(width) > src.width
        || top.saturating_add(height) > src.height
    {
        return Err("image source_rect exceeds ART frame bounds");
    }
    for y in 0..dst.height {
        for x in 0..dst.width {
            let sx = left + x * width / dst.width;
            let sy = top + y * height / dst.height;
            let px = src.pixels[(sy * src.width + sx) as usize];
            if px[3] != 0 {
                dst.pixels[(y * dst.width + x) as usize] = px;
            }
        }
    }
    Ok(())
}

fn raster(
    c: &Component,
    full: Rect,
    clip: Rect,
    scale: u32,
    hover: Option<Color>,
) -> Arc<LoadedFrame> {
    let mut frame = sprite([clip.w, clip.h], [0; 4]);
    let src = &c.base;
    for y in 0..clip.h {
        for x in 0..clip.w {
            let sx = ((clip.x - full.x) as u32 + x) * src.width / full.w;
            let sy = ((clip.y - full.y) as u32 + y) * src.height / full.h;
            let mut color = src.pixels[(sy * src.width + sx) as usize];
            if let Some(highlight) = hover
                && (color == c.config.fill.unwrap_or([0; 4])
                    || sx == 0
                    || sy == 0
                    || sx + 1 == src.width
                    || sy + 1 == src.height)
            {
                color = highlight;
            }
            frame.pixels[(y * clip.w + x) as usize] = color;
        }
    }
    if let Some(text) = &c.text {
        draw_text(&mut frame, text, &c.config, full, clip, scale);
    }
    Arc::new(LoadedFrame {
        source: Source {
            virtual_path: format!("ui/{}", c.config.id),
            mount_id: MountId::GENERATED,
            mount_path: Default::default(),
            loose_path: None,
            storage: Storage::Loose,
            mod_id: None,
        },
        decoded: DecodedFrame {
            frame,
            frame_count: 1,
            rotation_count: 1,
            stored_rotation: 0,
            stored_palette: 0,
            palette_key: [0; 3],
            frames_per_second: 0,
            action_frame: 0,
            ignored_trailing_bytes: 0,
        },
        input_bytes: 0,
    })
}

fn font_glyph(
    game: &mut Vfs,
    files: &mut Vfs,
    font: &Font,
    ch: char,
) -> Result<SpriteFrame, Box<dyn Error>> {
    if font.source == "bitmap" {
        let ch = u8::try_from(ch.to_ascii_uppercase() as u32)
            .map_err(|_| "bitmap font supports ASCII")?;
        let rows = super::menu::glyph_rows(ch);
        if ch != b' ' && rows == [0; 7] {
            return Err(format!("bitmap font has no glyph for {}", char::from(ch)).into());
        }
        let mut frame = sprite([6, 7], [0; 4]);
        for (y, row) in rows.into_iter().enumerate() {
            for x in 0..5 {
                if row & (16 >> x) != 0 {
                    frame.pixels[y * 6 + x] = [255; 4];
                }
            }
        }
        frame.hotspot.0 = 7;
        Ok(frame)
    } else {
        let frame = (ch as usize)
            .checked_sub(31)
            .ok_or("invalid font character")?;
        let mut glyph = resource(game, files, &font.source, frame)?.decoded.frame;
        if glyph.width > 64 || glyph.height > 64 || !(0..=128).contains(&glyph.hotspot.0) {
            return Err("ART font glyph dimensions or advance exceed bounds".into());
        }
        for [red, _, _, alpha] in &mut glyph.pixels {
            if *alpha != 0 {
                // TIG's source-alpha font blit takes coverage from palette red.
                *alpha = *red;
            }
        }
        Ok(glyph)
    }
}

fn draw_text(
    frame: &mut SpriteFrame,
    text: &Text,
    element: &Element,
    full: Rect,
    clip: Rect,
    scale: u32,
) {
    let Text {
        glyphs,
        font,
        native_height,
    } = text;
    let to_pixels = |units: u32| (units * scale + native_height * 50) / (native_height * 100);
    let scale_glyph = |units| to_pixels(units * font.size);
    let line_step = (font.size + font.line_gap) * native_height;
    let (mut start, mut line) = (0, 0);
    let visible_x = (clip.x - full.x) as u32;
    let visible_y = (clip.y - full.y) as u32;
    while start < glyphs.len() {
        let y = to_pixels(line * line_step);
        if y >= visible_y + frame.height {
            break;
        }
        let (mut end, mut advance, mut width, mut space) = (start, 0, 0, None);
        while let Some(Some((glyph, ch))) = glyphs.get(end) {
            let next = width.max(advance + glyph.width);
            if element.wrap && scale_glyph(next) > full.w && end > start {
                if let Some((word_end, line_width)) = space {
                    end = word_end;
                    width = line_width;
                }
                break;
            }
            if *ch == ' ' {
                space = Some((end, width));
            }
            advance += glyph.hotspot.0 as u32;
            width = next;
            end += 1;
        }
        let space_left = full.w.saturating_sub(scale_glyph(width));
        let origin = match element.align {
            Align::Left => 0,
            Align::Center => space_left / 2,
            Align::Right => space_left,
        };
        let mut pen = 0;
        for (glyph, _) in glyphs[start..end].iter().flatten() {
            let x = origin + scale_glyph(pen);
            let w = (origin + scale_glyph(pen + glyph.width) - x).max(1);
            let h = scale_glyph(glyph.height).max(1);
            let bold = if font.bold { (scale / 100).max(1) } else { 0 };
            for py in y.max(visible_y)..(y + h).min(visible_y + frame.height) {
                let sy = (2 * (py - y) + 1) * glyph.height / (2 * h);
                for px in x.max(visible_x)..(x + w + bold).min(visible_x + frame.width) {
                    let u = px - x;
                    let coverage = |ix: u32| {
                        let sx = (2 * ix + 1) * glyph.width / (2 * w);
                        glyph.pixels[(sy * glyph.width + sx) as usize][3]
                    };
                    let alpha = if u < w { coverage(u) } else { 0 };
                    let alpha = if bold > 0 && u >= bold && u - bold < w {
                        alpha.max(coverage(u - bold))
                    } else {
                        alpha
                    };
                    let at = ((py - visible_y) * frame.width + px - visible_x) as usize;
                    blend_glyph(&mut frame.pixels[at], font.color, alpha);
                }
            }
            pen += glyph.hotspot.0 as u32;
        }
        start = end;
        if glyphs
            .get(start)
            .is_some_and(|g| g.as_ref().is_none_or(|(_, ch)| *ch == ' '))
        {
            start += 1;
        }
        line += 1;
    }
}

fn blend_glyph(dst: &mut Color, color: Color, coverage: u8) {
    let alpha = (u32::from(color[3]) * u32::from(coverage) + 127) / 255;
    if alpha == 0 {
        return;
    }
    let background = u32::from(dst[3]) * (255 - alpha);
    let total = alpha * 255 + background;
    for i in 0..3 {
        let sum = u32::from(color[i]) * alpha * 255 + u32::from(dst[i]) * background;
        dst[i] = ((sum + total / 2) / total) as u8;
    }
    dst[3] = ((total + 127) / 255) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    struct Package(PathBuf);

    impl Package {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "arcanum-hud-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn theme(&self, source: &str) -> Result<Hud, Box<dyn Error>> {
            let path = self.0.join("theme.toml");
            fs::write(&path, source)?;
            Hud::load(&path, &mut Vfs::new(Policy::Ordered), None)
        }
    }

    impl Drop for Package {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const EXAMPLE: &str = include_str!("../../ui/field-notes/theme.toml");

    #[test]
    fn component_can_use_original_art_subrect_without_stretching_other_pixels() {
        let source = SpriteFrame {
            pixels: vec![[1, 2, 3, 255], [2, 3, 4, 255], [3, 4, 5, 255]],
            ..sprite([3, 1], [0; 4])
        };
        let mut frame = sprite([2, 1], [0; 4]);
        scale_into(&source, &mut frame, Some([1, 0, 2, 1])).unwrap();
        assert_eq!(frame.pixels, source.pixels[1..]);
        assert!(scale_into(&source, &mut frame, Some([2, 0, 2, 1])).is_err());
    }

    #[test]
    fn text_preserves_coverage_and_cumulative_advance_at_fractional_scales() {
        let mut glyph = sprite([2, 2], [255, 255, 255, 64]);
        glyph.hotspot.0 = 3;
        let glyph = Arc::new(glyph);
        let text = Text {
            glyphs: vec![Some((Arc::clone(&glyph), 'A')), Some((glyph, 'A'))],
            font: Font {
                source: "bitmap".into(),
                size: 2,
                color: [255; 4],
                bold: false,
                line_gap: 0,
                fallback: None,
            },
            native_height: 2,
        };
        let theme = parse(EXAMPLE).unwrap();
        for (scale, second_x) in [(100, 3), (125, 4), (150, 5), (200, 6)] {
            let full = Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 5,
            };
            let mut frame = sprite([full.w, full.h], [0, 0, 0, 255]);
            draw_text(&mut frame, &text, &theme.hud[0], full, full, scale);
            assert_eq!(frame.pixels[0], [64, 64, 64, 255]);
            assert_eq!(frame.pixels[second_x - 1], [0, 0, 0, 255]);
            assert_eq!(frame.pixels[second_x], [64, 64, 64, 255]);
        }
    }

    #[test]
    #[ignore = "requires installed Arcanum data"]
    fn classic_uses_native_button_bounds_and_installed_font_metrics() {
        let root = std::env::var_os("ARCANUM_GAME_DIR").expect("set ARCANUM_GAME_DIR");
        let mut game = Vfs::from_game(root, Policy::Ce).unwrap();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/classic/theme.toml");
        let mut files = Vfs::new(Policy::Ordered);
        let font = Font {
            source: "vfs:art/interface/morph15font.art".into(),
            size: 19,
            color: [255; 4],
            bold: false,
            line_gap: 0,
            fallback: None,
        };
        let glyph = font_glyph(&mut game, &mut files, &font, 'A').unwrap();
        assert_eq!(
            (glyph.width, glyph.height, glyph.hotspot),
            (11, 19, (11, 16))
        );
        assert!(
            glyph
                .pixels
                .iter()
                .any(|color| color[3] > 0 && color[3] < 255)
        );

        for scale in [100, 125, 150, 200] {
            let mut hud = Hud::load(&path, &mut game, Some(scale)).unwrap();
            hud.layout((1920, 1080));
            assert_eq!(hud.draws(false).count(), 26);
            let left = (1920 - 8 * u32::from(scale)) / 2;
            let button = hud.placed[2].as_ref().unwrap().rect;
            assert_eq!(button.x as u32, left + 4 * u32::from(scale) / 100);
            assert_eq!(button.y as u32, 2 * u32::from(scale) / 100);
            assert_eq!(button.w, 34 * u32::from(scale) / 100);
            assert_eq!(hud.hit((button.x as u32, button.y as u32)), Hit::Panel);
            assert_eq!(hud.hit((left - 1, 5)), Hit::World);
            hud.advance_focus(false);
            assert_eq!(hud.focused_action(), None);
        }
    }

    #[test]
    fn packaged_theme_replaces_layout_font_and_hit_regions() {
        let package = Package::new();
        let mut first = package.theme(EXAMPLE).unwrap();
        first.layout((1920, 1080));
        assert_eq!(first.hit((40, 65)), Hit::Button(Action::Pause));
        assert_eq!(first.hit((21, 22)), Hit::Panel);
        first.advance_focus(false);
        assert_eq!(first.focused_action(), Some(Action::Pause));
        first.advance_focus(true);
        assert_eq!(first.focused_action(), Some(Action::ResetZoom));
        let original_text = first.placed[1]
            .as_ref()
            .unwrap()
            .art
            .decoded
            .frame
            .pixels
            .clone();
        first.adjust_scale(25);
        assert_eq!(first.placed[0].as_ref().unwrap().rect.x, 21);

        let replaced = EXAMPLE
            .replace("top-left", "top-right")
            .replace("offset = [12, 12]", "offset = [-12, 12]")
            .replace("size = 10", "size = 12");
        let mut replacement = package.theme(&replaced).unwrap();
        replacement.layout((800, 600));
        assert_ne!(
            replacement.placed[1]
                .as_ref()
                .unwrap()
                .art
                .decoded
                .frame
                .pixels,
            original_text
        );
        assert_eq!(replacement.placed[0].as_ref().unwrap().rect.x, 578);
        assert_eq!(replacement.hit((40, 65)), Hit::World);
        assert_eq!(replacement.hit((640, 65)), Hit::Button(Action::Pause));
    }

    #[test]
    fn theme_reports_bad_references_and_clips_children_and_input() {
        let package = Package::new();
        assert!(parse("name = 'bad'\nsacle = 100").is_err());
        assert!(
            package
                .theme(&EXAMPLE.replace("parent = \"toolbar\"", "parent = \"missing\""))
                .is_err()
        );
        assert!(
            package
                .theme(&EXAMPLE.replace("source = \"bitmap\"", "source = \"file:missing.art\""))
                .is_err()
        );
        let mut clipped = package
            .theme(&EXAMPLE.replace("size = [136, 97]", "size = [40, 30]"))
            .unwrap();
        clipped.layout((800, 600));
        assert_eq!(clipped.hit((40, 61)), Hit::Button(Action::Pause));
        assert_eq!(clipped.hit((40, 65)), Hit::World);
        assert_eq!(clipped.hit((79, 61)), Hit::World);
        let mut hidden = package
            .theme(&EXAMPLE.replace("capture = true", "capture = true\nhidden = true"))
            .unwrap();
        hidden.layout((800, 600));
        assert_eq!(hidden.draws(false).count(), 0);
        assert_eq!(hidden.hit((40, 61)), Hit::World);
        hidden.advance_focus(false);
        assert_eq!(hidden.focused_action(), None);
    }

    #[test]
    fn package_art_is_explicit_and_installed_art_uses_vfs_precedence() {
        fn pixel_art(red: u8) -> Vec<u8> {
            let mut bytes = vec![0; 132 + 1024 + 28];
            let frame = 132 + 1024;
            for (offset, value) in [
                (0, 5_i32),
                (8, 8),
                (12, 10),
                (28, 1),
                (32, 1),
                (frame, 1),
                (frame + 4, 1),
                (frame + 8, 1),
            ] {
                bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
            bytes[138] = red;
            bytes.push(1);
            bytes
        }
        let source = Package::new();
        let overlay = Package::new();
        let package = Package::new();
        for (dir, red) in [(&source, 10), (&overlay, 20), (&package, 30)] {
            fs::write(dir.0.join("image.art"), pixel_art(red)).unwrap();
        }
        let mut game = Vfs::new(Policy::Ordered);
        game.mount_directory(&source.0).unwrap();
        game.mount_directory(&overlay.0).unwrap();
        let mut files = Vfs::new(Policy::Ordered);
        files.mount_directory(&package.0).unwrap();
        for (path, red) in [("vfs:image.art", 20), ("file:image.art", 30)] {
            let asset = resource(&mut game, &mut files, path, 0).unwrap();
            assert_eq!(asset.decoded.frame.pixels, [[red, 0, 0, 255]]);
        }
        assert!(resource(&mut game, &mut files, "file:../image.art", 0).is_err());
        let font = Font {
            source: "file:image.art".into(),
            size: 5,
            color: [255; 4],
            bold: false,
            line_gap: 0,
            fallback: None,
        };
        assert!(font_glyph(&mut game, &mut files, &font, '\u{1f}').is_ok());
        assert!(font_glyph(&mut game, &mut files, &font, 'A').is_err());
    }
}
