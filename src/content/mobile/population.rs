//! Original object identities, map membership and effective mobile properties.
use super::*;
use crate::sector::objects::{
    AppearanceFields, FieldValue, Instance, OriginalId, PrototypeDefaults,
};
use serde::Serialize;
use std::collections::{BTreeSet, btree_map::Entry};

pub(super) const INVENTORY: u32 = 0x1000;
pub(super) const INACTIVE: u32 = 0x0003;
pub(super) const DONT_DRAW: u32 = 0x0100;
pub(super) const INVISIBLE: u32 = 0x0200;
const NO_BLOCK: u32 = 0x0400;

pub(crate) fn paths(vfs: &Vfs, map: &str) -> Vec<String> {
    let prefix = format!("maps/{map}/");
    vfs.paths_with_prefix(&prefix)
        .into_iter()
        .filter(|path| path.ends_with(".mob") && !path[prefix.len()..].contains('/'))
        .map(str::to_owned)
        .collect()
}

#[derive(Clone)]
struct Locator {
    path: String,
    category: u32,
    tile: Option<(u32, u32)>,
    parent: Option<OriginalId>,
    held: bool,
    inventory: Vec<OriginalId>,
}

#[derive(Default)]
pub(crate) struct Index {
    objects: BTreeMap<OriginalId, Locator>,
    sectors: BTreeMap<u64, Vec<OriginalId>>,
    inverse: BTreeMap<OriginalId, Option<OriginalId>>,
    duplicates: BTreeSet<OriginalId>,
    invalid_chains: BTreeSet<OriginalId>,
    pub unreadable: usize,
    pub rejected: usize,
}

pub(crate) fn parent(object: &Instance<'_>) -> Option<OriginalId> {
    matches!(object.category, 5..=14)
        .then(|| object.field(65).and_then(FieldValue::reference))
        .flatten()
}

pub(crate) fn inventory(object: &Instance<'_>) -> Vec<OriginalId> {
    let key = match object.category {
        2 => 68,
        15 | 16 => 84,
        _ => return Vec::new(),
    };
    if let Some(FieldValue::Array { values, .. }) = object.field(key) {
        values
            .as_chunks::<24>()
            .0
            .iter()
            .copied()
            .map(OriginalId::from_wire)
            .filter(|id| id.kind != 0)
            .collect()
    } else {
        Vec::new()
    }
}

pub(crate) fn stored_flags(object: &Instance<'_>) -> Option<u32> {
    object.field(18).and_then(FieldValue::scalar)
}

impl Index {
    pub fn load(vfs: &mut Vfs, map: &str) -> Self {
        let mut index = Self::default();
        for path in paths(vfs, map) {
            match read_bounded(vfs, &path, MAX_PROTOTYPE_BYTES) {
                Ok((_, bytes)) => match objects::decode_instance(&bytes) {
                    Ok(object) => index.add(path, &object),
                    Err(_) => index.rejected += 1,
                },
                Err(_) => index.unreadable += 1,
            }
        }
        index.finish();
        index
    }

    pub fn add(&mut self, path: String, object: &Instance<'_>) {
        if object.id.kind != 2 || self.duplicates.contains(&object.id) {
            return;
        }
        match self.objects.entry(object.id) {
            Entry::Occupied(entry) => {
                entry.remove();
                self.duplicates.insert(object.id);
            }
            Entry::Vacant(entry) => {
                entry.insert(Locator {
                    path,
                    category: object.category,
                    tile: object.tile().filter(|tile| *tile != (0, 0)),
                    parent: parent(object),
                    held: stored_flags(object).is_some_and(|flags| flags & INVENTORY != 0),
                    inventory: inventory(object),
                });
            }
        }
    }

    pub fn finish(&mut self) {
        for (&owner, entry) in &self.objects {
            for &item in &entry.inventory {
                self.inverse
                    .entry(item)
                    .and_modify(|prior| {
                        if *prior != Some(owner) {
                            *prior = None
                        }
                    })
                    .or_insert(Some(owner));
            }
        }
        for (&id, entry) in &self.objects {
            let mut visited = BTreeSet::new();
            let mut cursor = id;
            let mut tile = entry.tile;
            // Associate contained objects with their root's sector for inspection.
            // Placement remains separately classified as contained.
            while let Some(owner) = self.owner(cursor) {
                if visited.len() == 64 || !visited.insert(cursor) || self.conflicting_owner(cursor)
                {
                    self.invalid_chains.insert(id);
                    break;
                }
                let Some(parent) = self.objects.get(&owner) else {
                    break;
                };
                tile = parent.tile.or(tile);
                cursor = owner;
            }
            if let Some(tile) = tile {
                self.sectors
                    .entry(super::super::world::tile_sector_id(tile))
                    .or_default()
                    .push(id);
            }
        }
    }

    pub fn contains(&self, id: OriginalId) -> bool {
        self.objects.contains_key(&id)
    }

    pub fn path(&self, id: OriginalId) -> Option<&str> {
        self.objects.get(&id).map(|entry| entry.path.as_str())
    }

    pub fn category(&self, id: OriginalId) -> Option<u32> {
        self.objects.get(&id).map(|entry| entry.category)
    }

    pub fn members(&self, owner: OriginalId) -> BTreeSet<OriginalId> {
        let mut members: BTreeSet<_> = self.objects[&owner].inventory.iter().copied().collect();
        members.extend(
            self.objects
                .iter()
                .filter_map(|(&id, entry)| (entry.parent == Some(owner)).then_some(id)),
        );
        members
    }

    pub fn owner(&self, id: OriginalId) -> Option<OriginalId> {
        self.objects
            .get(&id)?
            .parent
            .or_else(|| self.inverse.get(&id).copied().flatten())
    }

    pub fn conflicting_owner(&self, id: OriginalId) -> bool {
        self.invalid_chains.contains(&id)
            || self.inverse.get(&id).is_some_and(|inverse| match inverse {
                Some(inverse) => self
                    .objects
                    .get(&id)
                    .and_then(|entry| entry.parent)
                    .is_some_and(|direct| direct != *inverse),
                None => true,
            })
    }

    pub fn is_contained(&self, object: &Instance<'_>, flags: Option<u32>) -> bool {
        flags.is_some_and(|flags| flags & INVENTORY != 0)
            || self.objects.get(&object.id).is_some_and(|entry| entry.held)
            || self.inverse.contains_key(&object.id)
            || parent(object).is_some()
    }

    pub fn summary(&self) -> (usize, usize, usize, usize) {
        let mut placed = 0;
        let mut contained = 0;
        let mut resolved_owners = 0;
        for (&id, entry) in &self.objects {
            if entry.held || entry.parent.is_some() || self.inverse.contains_key(&id) {
                contained += 1;
                if !self.conflicting_owner(id) && self.owner(id).is_some_and(|id| self.contains(id))
                {
                    resolved_owners += 1;
                }
            } else if entry.tile.is_some() {
                placed += 1;
            }
        }
        (placed, contained, resolved_owners, self.duplicates.len())
    }

    pub fn entries(&self, sector: u64) -> impl Iterator<Item = (OriginalId, &str)> {
        self.sectors
            .get(&sector)
            .into_iter()
            .flatten()
            .map(|id| (*id, self.objects[id].path.as_str()))
    }
}

#[derive(Serialize, Clone, Copy)]
#[serde(tag = "origin", content = "value", rename_all = "snake_case")]
pub(crate) enum Property<T> {
    Instance(T),
    Prototype(T),
    Unresolved,
}

impl<T: Copy> Property<T> {
    pub fn resolve(instance: Option<T>, prototype: Option<T>) -> Self {
        match (instance, prototype) {
            (Some(value), _) => Self::Instance(value),
            (None, Some(value)) => Self::Prototype(value),
            (None, None) => Self::Unresolved,
        }
    }

    pub fn value(self) -> Option<T> {
        match self {
            Self::Instance(value) | Self::Prototype(value) => Some(value),
            Self::Unresolved => None,
        }
    }
}

pub(crate) struct Prototype {
    pub source: super::super::Source,
    pub defaults: PrototypeDefaults,
}

#[derive(Default)]
pub(crate) struct Prototypes(BTreeMap<(u32, u32), Result<Arc<Prototype>, Arc<str>>>);

impl Prototypes {
    pub fn get(
        &mut self,
        vfs: &mut Vfs,
        object: &Instance<'_>,
    ) -> Result<Arc<Prototype>, Arc<str>> {
        // Cache both successful and failed reads, within a map/loader lifetime.
        const CACHE_ENTRIES: usize = 4096;
        let key = (object.category, object.prototype.0);
        if let Some(entry) = self.0.get(&key) {
            return entry.clone();
        }
        let result = read_static_prototype(vfs, object.prototype)
            .and_then(|(source, bytes)| {
                let defaults =
                    objects::prototype_defaults(&bytes, object.prototype, object.category)?;
                Ok(Arc::new(Prototype { source, defaults }))
            })
            .map_err(|err| Arc::from(err.to_string()));
        if self.0.len() < CACHE_ENTRIES {
            self.0.insert(key, result.clone());
        }
        result
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Effective {
    pub current_art: Property<u32>,
    pub flags: Property<u32>,
    pub appearance: AppearanceFields,
    pub dexterity: Option<i32>,
}

impl Effective {
    pub fn new(object: &Instance<'_>, prototype: Option<&Prototype>) -> Self {
        let explicit = object.appearance();
        let defaults = prototype
            .map(|proto| proto.defaults.appearance)
            .unwrap_or_default();
        let inherited_dexterity = prototype.and_then(|proto| proto.defaults.dexterity);
        let current_art = Property::resolve(
            object.field(0).and_then(FieldValue::scalar),
            prototype.and_then(|proto| proto.defaults.current_art),
        );
        let flags = Property::resolve(explicit.flags, defaults.flags);
        let explicit_dexterity = matches!(object.category, 15 | 16)
            .then(|| object.field(66))
            .flatten();
        let dexterity = match explicit_dexterity {
            Some(field) => field
                .array_entry(1)
                .map(|bytes| i32::from_le_bytes(bytes.try_into().unwrap())),
            None => inherited_dexterity,
        };
        let appearance = AppearanceFields {
            offset_x: explicit.offset_x.or(defaults.offset_x),
            offset_y: explicit.offset_y.or(defaults.offset_y),
            shadow_art: explicit.shadow_art.or(defaults.shadow_art),
            scale: explicit.scale.or(defaults.scale),
            flags: flags.value(),
            blit_flags: explicit.blit_flags.or(defaults.blit_flags),
            blit_color: explicit.blit_color.or(defaults.blit_color),
            alpha: explicit.alpha.or(defaults.alpha),
            light_flags: explicit.light_flags.or(defaults.light_flags),
            light_art: explicit.light_art.or(defaults.light_art),
            light_color: explicit.light_color.or(defaults.light_color),
            wall_flags: explicit.wall_flags.or(defaults.wall_flags),
            scenery_flags: explicit.scenery_flags.or(defaults.scenery_flags),
            lock_notify: explicit.lock_notify.or(defaults.lock_notify),
            lock_difficulty: explicit.lock_difficulty.or(defaults.lock_difficulty),
            lock_key_id: explicit.lock_key_id.or(defaults.lock_key_id),
            bed_occupied: explicit.bed_occupied.or(defaults.bed_occupied),
            use_local: explicit.use_local.or(defaults.use_local),
            portal_flags: explicit.portal_flags.or(defaults.portal_flags),
            critter_flags: explicit.critter_flags.or(defaults.critter_flags),
            critter_flags2: explicit.critter_flags2.or(defaults.critter_flags2),
            spell_flags: explicit.spell_flags.or(defaults.spell_flags),
            material: explicit.material.or(defaults.material),
            sound_effect: explicit.sound_effect.or(defaults.sound_effect),
            use_script: explicit.use_script.or(defaults.use_script),
            unlock_script: explicit.unlock_script.or(defaults.unlock_script),
            unlock_local: explicit.unlock_local.or(defaults.unlock_local),
            dying: explicit.dying.or(defaults.dying),
            heartbeat: explicit.heartbeat.or(defaults.heartbeat),
            dialog_script: explicit.dialog_script.or(defaults.dialog_script),
            dialog_local_flags: explicit.dialog_local_flags.or(defaults.dialog_local_flags),
            dialog_local_counters: explicit
                .dialog_local_counters
                .or(defaults.dialog_local_counters),
            dialog_override_script: explicit
                .dialog_override_script
                .or(defaults.dialog_override_script),
        };
        Self {
            current_art,
            flags,
            appearance,
            dexterity,
        }
    }

    pub fn blocks(&self, category: u32) -> bool {
        matches!(category, 2 | 15 | 16)
            && self
                .flags
                .value()
                .is_some_and(|flags| flags & (INACTIVE | NO_BLOCK) == 0)
    }
}

#[derive(Clone)]
pub(crate) struct Resident {
    pub id: OriginalId,
    pub category: u32,
    pub prototype_id: u32,
    pub current_art: Property<u32>,
    pub source: super::super::Source,
    pub bytes: Vec<u8>,
    pub prototype: Option<Arc<Prototype>>,
    pub prototype_error: Option<Arc<str>>,
    pub parent: Option<(OriginalId, bool)>,
    pub inventory: Vec<(OriginalId, bool)>,
    pub conflict: bool,
    pub placement: &'static str,
    pub tile: Option<(u32, u32)>,
    pub blocks: bool,
    pub render_issue: Option<String>,
    pub raster: Option<usize>,
}

impl Resident {
    pub(crate) fn generated_npc(
        id: OriginalId,
        prototype: u32,
        tile: (u32, u32),
        vfs: &mut Vfs,
        prototypes: &mut Prototypes,
    ) -> Result<Self, String> {
        let instance = Instance::from_prototype(id, prototype, 16);
        let prototype = prototypes.get(vfs, &instance).map_err(|e| e.to_string())?;
        let effective = Effective::new(&instance, Some(&prototype));
        Ok(Self {
            id,
            category: 16,
            prototype_id: instance.prototype.0,
            current_art: effective.current_art,
            source: prototype.source.clone(),
            bytes: vec![],
            prototype: Some(prototype),
            prototype_error: None,
            parent: None,
            inventory: vec![],
            conflict: false,
            placement: "generated",
            tile: Some(tile),
            blocks: effective.blocks(16),
            render_issue: None,
            raster: None,
        })
    }
    pub fn instance(&self) -> Instance<'_> {
        if self.id.kind == u16::MAX {
            return Instance::from_prototype(self.id, self.prototype_id, self.category);
        }
        objects::decode_instance(&self.bytes).expect("resident object payload validated")
    }

    pub fn diagnostic(&self) -> serde_json::Value {
        use serde_json::json;
        let object = self.instance();
        let effective = Effective::new(&object, self.prototype.as_deref());
        let pose = effective.current_art.value().map(|art| {
            let item = art >> 28 == 6;
            json!({"direction":if item {0} else {(art>>11)&7},
                "frame":if item {0} else {(art>>14)&31},"palette":(art>>4)&3})
        });
        let encode_ref = |(id, resolved): (OriginalId, bool)| json!({"original_id":id.key(),"found_in_map_index":resolved});
        let slots = object
            .fields
            .iter()
            .map(|&(slot, value)| match value {
                FieldValue::Scalar(n) => json!({"slot":slot,"scalar_bits":n}),
                FieldValue::Wide(n) => json!({"slot":slot,"wide_bits":format!("{n:016x}")}),
                FieldValue::Reference(id) => json!({"slot":slot,"original_id":id.key()}),
                FieldValue::Text(bytes) => json!({"slot":slot,"string_bytes":bytes.len()}),
                FieldValue::Array { width, values, .. } => {
                    json!({"slot":slot,"array_width":width,"entries":values.len()/width})
                }
                FieldValue::Null => json!({"slot":slot,"null":true}),
            })
            .collect::<Vec<_>>();
        json!({
            "id":object.id.key(),"original_id":object.id.key(),
            "category":objects::category_name(object.category),"type":object.category,
            "instance_source":super::super::diagnostics::source(&self.source),
            "prototype":object.prototype.0,
            "description_id":Property::resolve(object.field(22).and_then(FieldValue::scalar), self.prototype.as_ref().and_then(|proto|proto.defaults.description)),
            "prototype_source":self.prototype.as_ref().map(|proto|super::super::diagnostics::source(&proto.source)),
            "prototype_error":self.prototype_error.as_deref(),
            "field_bitmap":object.bitmap,"fields":slots,"stored_tile":object.tile(),
            "tile":self.tile,"placement":self.placement,
            "parent":self.parent.map(encode_ref),
            "inventory":self.inventory.iter().take(128).copied().map(encode_ref).collect::<Vec<_>>(),
            "inventory_count":self.inventory.len(),"ownership_conflict":self.conflict,
            "inventory_slot":if matches!(object.category,5..=14) {object.field(71).and_then(FieldValue::scalar).map(|n|n as i32)}else{None},
            "effective":{
                "current_art":effective.current_art,"authored_pose":pose,
                "flags":effective.flags,"appearance":effective.appearance,
                "appearance_overrides":object.appearance(),
                "appearance_prototype":self.prototype.as_ref().map(|proto|proto.defaults.appearance),
            },
            "rendering":{"state":if self.raster.is_some() {"prepared"} else {"no_raster"},"reason":self.render_issue},
            "collision_registered":self.blocks,
            "interaction":{"kind":"unimplemented_mobile_behavior"}
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prototype::SerializedPrototypeId;

    fn id(n: u8) -> OriginalId {
        OriginalId {
            kind: 2,
            payload: [n; 16],
        }
    }

    fn fixture(
        n: u8,
        category: u32,
        fields: Vec<(usize, FieldValue<'static>)>,
    ) -> Instance<'static> {
        Instance {
            id: id(n),
            category,
            prototype: SerializedPrototypeId(1),
            bitmap: [0; 5],
            fields,
        }
    }

    #[test]
    fn original_ids_survive_discovery_and_contained_objects_follow_owner_for_inspection() {
        let owner = fixture(1, 2, vec![(1, FieldValue::Wide((32u64 << 32) | 100))]);
        let item = fixture(
            2,
            9,
            vec![
                (1, FieldValue::Wide((32u64 << 32) | 196)),
                (18, FieldValue::Scalar(INVENTORY)),
                (65, FieldValue::Reference(owner.id)),
            ],
        );
        let orphan = fixture(3, 9, vec![(65, FieldValue::Reference(id(7)))]);
        let mut index = Index::default();
        index.add("container.mob".into(), &owner);
        index.add("item.mob".into(), &item);
        index.add("orphan.mob".into(), &orphan);
        index.finish();
        assert_eq!(index.owner(item.id), Some(owner.id));
        assert!(index.is_contained(&item, Some(INVENTORY)));
        assert!(index.is_contained(&orphan, None));
        assert_eq!(index.summary(), (1, 2, 1, 0));
        let keys = index.entries(1).map(|(id, _)| id).collect::<Vec<_>>();
        assert_eq!(keys, [owner.id, item.id]);
        assert_eq!(index.entries(3).count(), 0);
    }

    #[test]
    fn conflicting_guid_locations_do_not_instantiate_twice() {
        let object = fixture(3, 16, vec![(1, FieldValue::Wide((32u64 << 32) | 100))]);
        let mut index = Index::default();
        for path in ["first.mob", "second.mob", "third.mob"] {
            index.add(path.into(), &object);
        }
        index.finish();
        assert!(!index.contains(object.id));
        assert_eq!(index.summary(), (0, 0, 0, 1));
        assert_eq!(index.entries(1).count(), 0);
    }

    #[test]
    fn inheritance_preserves_zero_and_distinguishes_unresolved_properties() {
        assert!(matches!(
            Property::resolve(Some(0), Some(23)),
            Property::Instance(0)
        ));
        assert!(matches!(
            Property::resolve(None, Some(23)),
            Property::Prototype(23)
        ));
        assert!(matches!(
            Property::<u32>::resolve(None, None),
            Property::Unresolved
        ));
        let object = fixture(3, 16, vec![]);
        assert!(Effective::new(&object, None).current_art.value().is_none());
        assert!(!Effective::new(&object, None).blocks(16));
    }
}
