//! Headless compatibility accounting using the gameplay sector loader.
use super::*;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize)]
pub struct Issue {
    pub code: &'static str,
    pub impact: &'static str,
    pub detail: String,
    pub record: Option<usize>,
    pub category: Option<&'static str>,
    pub tile: Option<(u32, u32)>,
    pub raw_art: Option<u32>,
    pub prototype: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub research: Option<Research>,
}

#[derive(Debug, Serialize)]
pub struct Research {
    pub cause: &'static str,
    pub stage: &'static str,
    pub verification: &'static str,
    pub related: &'static [&'static str],
    pub evidence: Value,
}

impl Research {
    fn new(
        cause: &'static str,
        stage: &'static str,
        verification: &'static str,
        related: &'static [&'static str],
        evidence: Value,
    ) -> Self {
        Self {
            cause,
            stage,
            verification,
            related,
            evidence,
        }
    }
}

impl Issue {
    pub(super) fn new(code: &'static str, detail: &str) -> Self {
        let impact = match code {
            "object_list_rejected" | "mobile_format_rejected" | "map_metadata_failed" => "parsing",
            "sector_missing"
            | "sector_load_failed"
            | "terrain_lookup_failed"
            | "terrain_variant_unsupported"
            | "template_missing" => "traversal",
            "interaction_unsupported"
            | "trap_runtime_unsupported"
            | "transition_metadata_failed" => "interaction",
            _ => "rendering",
        };
        Self {
            code,
            impact,
            detail: detail.into(),
            record: None,
            category: None,
            tile: None,
            raw_art: None,
            prototype: None,
            research: None,
        }
    }

    pub(super) fn sector_failure(detail: &str) -> Self {
        let code = if detail.contains("terrain lookup unavailable") {
            "terrain_lookup_failed"
        } else if detail.contains("original terrain template") {
            "template_missing"
        } else if detail.contains("not found") {
            "sector_missing"
        } else {
            "sector_load_failed"
        };
        Self::new(code, detail)
    }

    pub(super) fn record(
        code: &'static str,
        record: usize,
        tile: (u32, u32),
        detail: &str,
    ) -> Self {
        Self {
            record: Some(record),
            tile: Some(tile),
            ..Self::new(code, detail)
        }
    }

    pub(super) fn list_rejection(
        detail: &str,
        cause: &'static str,
        evidence: &crate::sector::objects::ListEvidence,
    ) -> Self {
        let header = evidence.header.as_ref();
        let decoded = evidence.preparation_budget_bytes.is_some();
        Self {
            record: evidence.record,
            category: header
                .and_then(|h| h.category)
                .and_then(crate::sector::objects::category_name),
            prototype: header.and_then(|h| h.prototype),
            research: Some(Research::new(
                cause,
                if decoded {
                    "sector_preparation"
                } else {
                    "sector_object_decode"
                },
                if decoded
                    || matches!(
                        cause,
                        "sector_instance_category_unsupported"
                            | "sector_instance_fields_unsupported"
                    )
                {
                    "confirmed_engine_limitation"
                } else {
                    "unclassified_original_representation"
                },
                &[
                    "object_placement",
                    "collision",
                    "appearance",
                    "interaction",
                    "object_lighting",
                ],
                json!(evidence),
            )),
            ..Self::new("object_list_rejected", detail)
        }
    }

    pub(super) fn scene(issue: &super::super::scenery::SceneIssue) -> Self {
        use crate::sector::objects::StaticCategory::*;
        let category = match issue.category {
            Wall => "wall",
            Portal => "portal",
            Scenery => "scenery",
        };
        // Importers share fallible APIs. Use their diagnostic kind and specific
        // format-validation messages here, before formatting any reports.
        let reason = &issue.reason;
        let code = if reason.contains("prototype")
            && (reason.contains("not found")
                || reason.contains("no files")
                || reason.contains("missing"))
        {
            "prototype_missing"
        } else if reason.contains("unsupported wall ART fields") {
            "wall_art_variant_unsupported"
        } else if issue.opening.is_some() {
            "portal_opening_unsupported"
        } else if reason.contains("unsupported portal") {
            "portal_variant_unsupported"
        } else if issue.appearance.is_some() {
            "object_appearance_unsupported"
        } else if issue.art_failure {
            "art_unresolved_or_rejected"
        } else {
            "object_import_rejected"
        };
        let research = if let Some(appearance) = issue.appearance {
            let (cause, verification) = if appearance.flags & 0x103 != 0 {
                (
                    "appearance_authored_suppression_flag",
                    "reference_draw_suppression",
                )
            } else if appearance.flags & 0x200 != 0 {
                (
                    "appearance_detect_invisible_state",
                    "runtime_verification_required",
                )
            } else if appearance.flags & 0x20000 != 0 {
                ("appearance_wading_state", "runtime_verification_required")
            } else if appearance.flags & 0x1000 != 0 {
                (
                    "appearance_inventory_state",
                    "runtime_verification_required",
                )
            } else if appearance.blend_rejected || appearance.scale != 100 {
                (
                    "appearance_blend_or_scale_gate",
                    "confirmed_engine_limitation",
                )
            } else if appearance.disallowed_flags != 0 {
                (
                    "appearance_other_object_flag_gate",
                    "unclassified_original_representation",
                )
            } else {
                (
                    "appearance_subtype_or_flat_gate",
                    "unclassified_original_representation",
                )
            };
            Some(Research::new(
                cause,
                "appearance_preparation",
                verification,
                &["prototype_inheritance", "art_lookup_not_reached"],
                json!(appearance),
            ))
        } else {
            issue.opening.map(|opening| {
                Research::new(
                    opening.code,
                    "portal_wall_association",
                    "runtime_verification_required",
                    &[
                        "wall_art",
                        "portal_art_lookup_not_reached",
                        "interaction_preparation",
                    ],
                    json!(opening),
                )
            })
        };
        Self {
            category: Some(category),
            raw_art: Some(issue.art_id),
            prototype: Some(issue.prototype),
            research,
            ..Self::record(code, issue.record, issue.tile, reason)
        }
    }

    pub(super) fn trap(record: usize, prototype: u32) -> Self {
        Self {
            record: Some(record),
            category: Some("trap"),
            prototype: Some(prototype),
            ..Self::new(
                "trap_runtime_unsupported",
                "Parsed trap; detection and disarming are unimplemented",
            )
        }
    }
}

#[derive(Default)]
pub struct ScanOptions {
    pub map: Option<String>,
    pub sector: Option<u64>,
    pub limit: Option<usize>,
    pub category: Option<String>,
    pub group: Option<String>,
    pub examples: usize,
}

#[derive(Default, Serialize)]
struct Coverage {
    available_maps: usize,
    selected_maps: usize,
    maps_with_metadata: usize,
    authored_resources_selected: usize,
    authored_attempted: usize,
    explicit_location_attempted: usize,
    templates_indexed: usize,
    templates_attempted: usize,
    terrain_grids_decoded: usize,
    terrain_template_paths_referenced: usize,
    sectors_with_surfaces: usize,
    collision_prepared: usize,
    object_lists_decoded: usize,
    object_lists_rejected: usize,
    serialized_records_decoded: usize,
    imported_walls: usize,
    imported_scenery: usize,
    imported_portals: usize,
    rasters_rejected: usize,
    object_preparation_peak_bytes: usize,
    authored_roof_entries: usize,
    roof_rasters_prepared: usize,
    surface_cells_prepared: usize,
    facade_cells_prepared: usize,
    mobile_resources_attempted: usize,
    mobile_records_decoded: usize,
    mobile_typed_fields: usize,
    mobile_prototypes_resolved: usize,
    mobile_explicit_art: usize,
    mobile_inherited_art: usize,
    mobile_world_placement_candidates: usize,
    mobile_contained: usize,
    mobile_contained_owner_resolved: usize,
    mobile_duplicate_identities: usize,
    mobile_residents_in_scanned_sectors: usize,
    mobile_rasters_prepared: usize,
    mobile_collision_registered: usize,
    mobile_world_residents_without_raster: usize,
    use_transitions_prepared: usize,
    door_targets_prepared: usize,
    map_jump_records_decoded: usize,
}

#[derive(Default)]
struct Group {
    impact: &'static str,
    count: usize,
    maps: BTreeSet<i32>,
    sectors: BTreeSet<(Option<i32>, u64)>,
    resources: BTreeSet<String>,
    examples: Vec<Value>,
    causes: BTreeMap<(&'static str, Option<&'static str>), CauseGroup>,
}

struct CauseGroup {
    stage: &'static str,
    verification: &'static str,
    related: &'static [&'static str],
    counts: Group,
    variants: BTreeMap<&'static str, usize>,
}

impl Group {
    fn count(&mut self, map: Option<i32>, sector: Option<u64>, resource: &Value) {
        self.count += 1;
        self.maps.extend(map);
        if let Some(path) = resource.as_str().or_else(|| resource["path"].as_str()) {
            self.resources.insert(path.to_owned());
        }
        self.sectors.extend(sector.map(|id| (map, id)));
    }
}

#[derive(Default, Serialize)]
struct MobileHeaders {
    version: BTreeMap<u32, usize>,
    prototype_kind: BTreeMap<u16, usize>,
    instance_kind: BTreeMap<u16, usize>,
    by_category: BTreeMap<&'static str, MobileCategory>,
    unavailable: usize,
}

#[derive(Default, Serialize)]
struct MobileCategory {
    resources: usize,
    structurally_decoded: usize,
    explicit_location_bit: usize,
    explicit_art_bit: usize,
    location_value_decoded: usize,
    current_art_value_decoded: usize,
    nonzero_location_value: usize,
    parent_field_bit: usize,
    inventory_list_bit: usize,
    field_bitmap_observed: usize,
    field_count_differs_from_bitmap: usize,
}

impl MobileHeaders {
    fn observe(&mut self, header: Option<&crate::sector::objects::InstanceHeader>, decoded: bool) {
        let Some(header) = header else {
            self.unavailable += 1;
            return;
        };
        *self.version.entry(header.version).or_default() += 1;
        if let Some(kind) = header.prototype_kind {
            *self.prototype_kind.entry(kind).or_default() += 1;
        }
        if let Some(kind) = header.instance_kind {
            *self.instance_kind.entry(kind).or_default() += 1;
        }
        let name = header
            .category
            .and_then(crate::sector::objects::category_name)
            .unwrap_or("unknown");
        let category = self.by_category.entry(name).or_default();
        category.resources += 1;
        category.structurally_decoded += usize::from(decoded);
        category.location_value_decoded += usize::from(header.location.is_some());
        category.nonzero_location_value +=
            usize::from(header.location.is_some_and(|tile| tile != (0, 0)));
        category.current_art_value_decoded += usize::from(header.current_art.is_some());
        if let Some(bitmap) = header.common_bitmap {
            category.explicit_art_bit += usize::from(bitmap[0] & 1 != 0);
            category.explicit_location_bit += usize::from(bitmap[0] & 2 != 0);
        }
        if let Some(bitmap) = &header.bitmap {
            category.field_bitmap_observed += 1;
            let set = bitmap.iter().map(|word| word.count_ones()).sum::<u32>();
            category.field_count_differs_from_bitmap +=
                usize::from(header.field_count.map(u32::from) != Some(set));
            category.parent_field_bit +=
                usize::from(matches!(header.category, Some(5..=14)) && bitmap[2] & 2 != 0);
            category.inventory_list_bit += usize::from(match header.category {
                Some(2) => bitmap[2] & (1 << 4) != 0,
                Some(15 | 16) => bitmap[2] & (1 << 20) != 0,
                _ => false,
            });
        }
    }
}

fn add(
    groups: &mut BTreeMap<&'static str, Group>,
    options: &ScanOptions,
    map: Option<i32>,
    sector: Option<u64>,
    resource: &Value,
    issue: &Issue,
) {
    if options
        .category
        .as_deref()
        .is_some_and(|category| issue.category != Some(category))
        || options.group.as_deref().is_some_and(|code| {
            issue.code != code && issue.research.as_ref().is_none_or(|r| r.cause != code)
        })
    {
        return;
    }
    let group = groups.entry(issue.code).or_default();
    group.impact = issue.impact;
    group.count(map, sector, resource);
    let limit = options.examples.min(32);
    let example = || json!({"map":map,"sector":sector,"resource":resource,"issue":issue});
    if group.examples.len() < limit {
        group.examples.push(example());
    }
    if let Some(research) = &issue.research {
        let cause = group
            .causes
            .entry((research.cause, issue.category))
            .or_insert_with(|| CauseGroup {
                stage: research.stage,
                verification: research.verification,
                related: research.related,
                counts: Group::default(),
                variants: BTreeMap::new(),
            });
        cause.counts.count(map, sector, resource);
        let variant = if issue.code == "portal_opening_unsupported" {
            issue.raw_art.map(|art| {
                if art >> 28 != 3 {
                    "unclassified_art"
                } else if art & 0x400 != 0 {
                    "door_art"
                } else {
                    "window_art"
                }
            })
        } else if issue.code == "mobile_format_rejected" {
            research.evidence["header"]["category"]
                .as_u64()
                .and_then(|category| u32::try_from(category).ok())
                .and_then(crate::sector::objects::category_name)
        } else {
            None
        };
        if let Some(variant) = variant {
            *cause.variants.entry(variant).or_default() += 1;
        }
        if cause.counts.examples.len() < limit.min(2) {
            cause.counts.examples.push(example());
        }
    }
}

fn account(coverage: &mut Coverage, report: &SectorReport) {
    coverage.sectors_with_surfaces += usize::from(report.source.is_some());
    coverage.collision_prepared += usize::from(report.collision_available);
    coverage.object_lists_decoded += usize::from(report.decoded_records.is_some());
    coverage.object_lists_rejected +=
        usize::from(report.source.is_some() && report.object_list_failed);
    coverage.serialized_records_decoded += report.decoded_records.unwrap_or(0);
    coverage.imported_walls += report.wall_count;
    coverage.imported_scenery += report.scenery_count;
    coverage.imported_portals += report.portal_count;
    coverage.rasters_rejected += report.object_raster_rejections;
    coverage.object_preparation_peak_bytes = coverage
        .object_preparation_peak_bytes
        .max(report.object_preparation_bytes);
    coverage.authored_roof_entries += report.roofs.0;
    coverage.roof_rasters_prepared += report.roofs.1;
    coverage.surface_cells_prepared += report.surfaces;
    coverage.facade_cells_prepared += report.facades;
}

pub fn compatibility_scan(
    mut vfs: Vfs,
    options: ScanOptions,
) -> Result<Value, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let catalog = super::super::maps::Catalog::load(&mut vfs)?;
    let mut maps: Vec<_> = match &options.map {
        Some(name) => vec![catalog.find(name)?],
        None => catalog.maps.iter().collect(),
    };
    maps.sort_by_key(|map| map.id);
    let mut coverage = Coverage {
        available_maps: catalog.maps.len(),
        selected_maps: maps.len(),
        ..Coverage::default()
    };
    let map_source = super::super::diagnostics::source(&catalog.source);
    let quest_resources = super::super::quest::Catalog::load(&mut vfs).coverage();
    let mut groups = BTreeMap::new();
    let mut mobile_headers = MobileHeaders::default();
    let mut prepared_mobile_kinds = BTreeMap::<&str, BTreeMap<&str, usize>>::new();
    let mut prepared_mobile_examples = BTreeMap::<&str, BTreeMap<&str, Vec<Value>>>::new();
    let mut scanned_maps = Vec::new();
    let mut locations = Vec::new();
    let mut templates_seen = BTreeSet::new();
    let mut loader = SectorLoader {
        mobile_index: Default::default(),
        mobile_prototypes: Default::default(),
        terrain: Ok(None),
        map: String::new(),
        reference: (0, 0),
        vfs,
        frames: FrameCache::default(),
        lighting: None,
        walking: false,
        playing: true,
        audit: false,
        roofs: true,
    };
    for map in maps {
        loader.map.clone_from(&map.directory);
        loader.mobile_index = Default::default();
        let files = match map.files(&mut loader.vfs) {
            Ok(files) => files,
            Err(error) => {
                add(
                    &mut groups,
                    &options,
                    Some(map.id),
                    None,
                    &map_source,
                    &Issue::new("map_metadata_failed", &error.to_string()),
                );
                continue;
            }
        };
        coverage.maps_with_metadata += 1;
        scanned_maps.push(
            json!({"map":map.id,"name":map.name,"directory":map.directory,"bounds":files.bounds(),
            "authored_sectors":files.sectors.len()}),
        );
        match super::super::jumps::load(&mut loader.vfs, &map.directory) {
            Ok(jumps) => coverage.map_jump_records_decoded += jumps.len(),
            Err(error) => add(
                &mut groups,
                &options,
                Some(map.id),
                None,
                &json!(format!("maps/{}/map.jmp", map.directory)),
                &Issue::new("transition_metadata_failed", &error.to_string()),
            ),
        }
        loader.terrain = super::super::terrain::TerrainMap::load(&mut loader.vfs, &map.directory)
            .map_err(|e| e.to_string());
        match &loader.terrain {
            Ok(Some(terrain)) => {
                coverage.terrain_grids_decoded += 1;
                let source = super::super::diagnostics::source(&terrain.source);
                for (word, result) in terrain.unique_template_references() {
                    match result {
                        Ok(path) => {
                            templates_seen.insert(path.clone());
                            if loader.vfs.resolve(&path)?.winner().is_none() {
                                add(
                                    &mut groups,
                                    &options,
                                    Some(map.id),
                                    None,
                                    &source,
                                    &Issue::new(
                                        "template_missing",
                                        &format!("Terrain word {word:#06x} references {path}"),
                                    ),
                                );
                            }
                        }
                        Err(error) => add(
                            &mut groups,
                            &options,
                            Some(map.id),
                            None,
                            &source,
                            &Issue::new(
                                "terrain_variant_unsupported",
                                &format!("{word:#06x}: {error}"),
                            ),
                        ),
                    }
                }
            }
            Err(error) => add(
                &mut groups,
                &options,
                Some(map.id),
                None,
                &json!(map.directory),
                &Issue::new("terrain_lookup_failed", error),
            ),
            Ok(None) => {}
        }
        if options.sector.is_none() {
            use crate::sector::objects::{InstanceHeader, category_name, decode_instance};
            use mobile::population::{self, Effective, Index, Property};
            let mut index = Index::default();
            for path in population::paths(&loader.vfs, &map.directory) {
                coverage.mobile_resources_attempted += 1;
                match super::super::read_bounded(
                    &mut loader.vfs,
                    &path,
                    crate::prototype::MAX_PROTOTYPE_BYTES,
                ) {
                    Ok((source, bytes)) => {
                        let instance = decode_instance(&bytes);
                        let header = InstanceHeader::observe(&bytes);
                        mobile_headers.observe(header.as_ref(), instance.is_ok());
                        match instance {
                            Ok(object) => {
                                coverage.mobile_records_decoded += 1;
                                coverage.mobile_typed_fields += object.fields.len();
                                let prototype =
                                    loader.mobile_prototypes.get(&mut loader.vfs, &object);
                                coverage.mobile_prototypes_resolved +=
                                    usize::from(prototype.is_ok());
                                if let Err(error) = &prototype {
                                    add(
                                        &mut groups,
                                        &options,
                                        Some(map.id),
                                        None,
                                        &super::super::diagnostics::source(&source),
                                        &Issue {
                                            category: Some("mobile"),
                                            prototype: Some(object.prototype.0),
                                            ..Issue::new("mobile_prototype_unresolved", error)
                                        },
                                    );
                                }
                                let effective = Effective::new(
                                    &object,
                                    prototype.as_ref().ok().map(AsRef::as_ref),
                                );
                                match effective.current_art {
                                    Property::Instance(_) => coverage.mobile_explicit_art += 1,
                                    Property::Prototype(_) => coverage.mobile_inherited_art += 1,
                                    Property::Unresolved => {}
                                }
                                index.add(path, &object);
                            }
                            Err(error) => add(
                                &mut groups,
                                &options,
                                Some(map.id),
                                None,
                                &super::super::diagnostics::source(&source),
                                &Issue {
                                    category: Some("mobile"),
                                    prototype: header.as_ref().and_then(|h| h.prototype),
                                    research: Some(Research::new(
                                        error.code,
                                        "mobile_instance_decode",
                                        "unclassified_original_representation",
                                        &["shared_instance_fields"],
                                        json!({"object_category":header.as_ref().and_then(|h|h.category).and_then(category_name),
                                            "header":header,"file_bytes":bytes.len(),"error_offset":error.offset}),
                                    )),
                                    ..Issue::new("mobile_format_rejected", &error.to_string())
                                },
                            ),
                        }
                    }
                    Err(error) => {
                        mobile_headers.observe(None, false);
                        add(
                            &mut groups,
                            &options,
                            Some(map.id),
                            None,
                            &json!(path),
                            &Issue {
                                category: Some("mobile"),
                                research: Some(Research::new(
                                    "mobile_resource_unreadable",
                                    "resource_read",
                                    "resource_unavailable",
                                    &[],
                                    Value::Null,
                                )),
                                ..Issue::new("mobile_format_rejected", &error.to_string())
                            },
                        );
                    }
                }
            }
            index.finish();
            let (placed, contained, resolved, duplicates) = index.summary();
            coverage.mobile_world_placement_candidates += placed;
            coverage.mobile_contained += contained;
            coverage.mobile_contained_owner_resolved += resolved;
            coverage.mobile_duplicate_identities += duplicates;
            loader.mobile_index = Arc::new(std::sync::OnceLock::from(index));
        }
        let selected: Vec<_> = options
            .sector
            .map_or_else(|| files.sectors.iter().copied().collect(), |id| vec![id]);
        coverage.authored_resources_selected += selected
            .iter()
            .filter(|id| files.sectors.contains(id))
            .count();
        for id in selected {
            if options
                .limit
                .is_some_and(|limit| coverage.authored_attempted >= limit)
            {
                break;
            }
            let coordinates = Coordinates::from_id(id).ok_or("Invalid sector ID")?;
            loader.reference = coordinates
                .world_tile(0, 0)
                .ok_or("Sector reference outside engine coordinate range")?;
            let (sector, stages) = loader.load(id);
            coverage.authored_attempted += usize::from(files.sectors.contains(&id));
            coverage.explicit_location_attempted += usize::from(options.sector.is_some());
            coverage.mobile_residents_in_scanned_sectors += sector.mobiles.len();
            for object in &sector.mobiles {
                if let Some(slot) = object.raster {
                    coverage.mobile_rasters_prepared += 1;
                    let kind =
                        crate::sector::objects::category_name(object.category).unwrap_or("unknown");
                    let origin = match object.current_art {
                        mobile::population::Property::Instance(_) => "instance",
                        mobile::population::Property::Prototype(_) => "prototype",
                        mobile::population::Property::Unresolved => "unresolved",
                    };
                    *prepared_mobile_kinds
                        .entry(kind)
                        .or_default()
                        .entry(origin)
                        .or_default() += 1;
                    let examples = prepared_mobile_examples
                        .entry(kind)
                        .or_default()
                        .entry(origin)
                        .or_default();
                    if examples.len() < options.examples.min(2) {
                        let asset = sector
                            .draws
                            .iter()
                            .find(|draw| draw.key.1.5 == slot)
                            .map(|draw| &draw.frame.source.virtual_path);
                        examples.push(json!({"map":map.id,"sector":id,"original_id":object.id.key(),"prototype":object.prototype_id,
                            "tile":object.tile,"art":object.current_art,"asset":asset}));
                    }
                }
                coverage.mobile_collision_registered += usize::from(object.blocks);
                if object.placement == "world" && object.raster.is_none() {
                    coverage.mobile_world_residents_without_raster += 1;
                    let issue = Issue {
                        category: Some("mobile"),
                        prototype: Some(object.prototype_id),
                        tile: object.tile,
                        ..Issue::new(
                            "mobile_raster_unprepared",
                            object
                                .render_issue
                                .as_deref()
                                .unwrap_or("no raster prepared"),
                        )
                    };
                    add(
                        &mut groups,
                        &options,
                        Some(map.id),
                        Some(id),
                        &super::super::diagnostics::source(&object.source),
                        &issue,
                    );
                }
            }
            account(&mut coverage, &sector.report);
            coverage.use_transitions_prepared += sector
                .interactions
                .values()
                .filter(|target| matches!(target.action, interaction::Action::Travel(_)))
                .count();
            let doors = sector.doors_at.values().map(Vec::len).sum::<usize>();
            coverage.door_targets_prepared += doors;
            if options.sector.is_some() {
                let r = &sector.report;
                locations.push(json!({"map":map.id,"sector":id,"source":r.source_details,
                    "decoded_records":r.decoded_records,"collision_prepared":r.collision_available,
                    "object_preparation_bytes":r.object_preparation_bytes,
                    "prepared_objects":{"walls":r.wall_count,"scenery":r.scenery_count,"portals":r.portal_count,"mobiles":sector.mobiles.iter().filter(|mobile| mobile.raster.is_some()).count()},
                    "door_targets":doors,"interaction_targets":sector.interactions.len(),
                    "timing_us":{"sector_read":stages.sector_read.as_micros(),"object_parse":stages.object_parse.as_micros(),
                        "object_import":stages.object_import.as_micros(),"raster_prepare":stages.rasters.as_micros()}}));
            }
            let source = sector
                .report
                .source_details
                .unwrap_or_else(|| json!(format!("maps/{}/{id}.sec", map.directory)));
            for issue in &sector.report.structured_issues {
                add(
                    &mut groups,
                    &options,
                    Some(map.id),
                    Some(id),
                    &source,
                    issue,
                );
            }
        }
    }
    coverage.terrain_template_paths_referenced = templates_seen.len();
    let templates: Vec<_> = loader
        .vfs
        .paths_with_prefix("terrain/")
        .into_iter()
        .filter(|path| path.ends_with(".sec"))
        .map(str::to_owned)
        .collect();
    coverage.templates_indexed = templates.len();
    loader.terrain = Ok(None);
    loader.map.clear();
    for path in templates {
        if options.sector.is_some() || (options.map.is_some() && !templates_seen.contains(&path)) {
            continue;
        }
        if options
            .limit
            .is_some_and(|limit| coverage.templates_attempted >= limit)
        {
            break;
        }
        let Some(id) = path
            .rsplit('/')
            .next()
            .and_then(|name| name.strip_suffix(".sec"))
            .and_then(|id| id.parse::<u64>().ok())
        else {
            continue;
        };
        let Some(tile) = Coordinates::from_id(id).and_then(|sector| sector.world_tile(0, 0)) else {
            continue;
        };
        loader.reference = tile;
        let (sector, _) = loader.load_source(id, Some(&path));
        coverage.templates_attempted += 1;
        account(&mut coverage, &sector.report);
        let source = sector.report.source_details.unwrap_or_else(|| json!(path));
        for issue in &sector.report.structured_issues {
            add(&mut groups, &options, None, None, &source, issue);
        }
    }
    let issues: Vec<_> = groups.into_iter().map(|(code, group)| {
        let causes: Vec<_> = group.causes.into_iter().map(|((cause, category), group)| {
            let counts = group.counts;
            json!({"code":cause,"category":category,"stage":group.stage,"verification":group.verification,
                "related":group.related,"occurrences":counts.count,"affected_maps":counts.maps.len(),
                "affected_world_sectors":counts.sectors.len(),"affected_resources":counts.resources.len(),
                "variants":group.variants,"examples":counts.examples})
        }).collect();
        json!({"code":code,"impact":group.impact,"occurrences":group.count,"affected_maps":group.maps.len(),
            "affected_world_sectors":group.sectors.len(),"affected_resources":group.resources.len(),"examples":group.examples,"causes":causes})
    }).collect();
    let (reads, art) = loader.stats();
    Ok(
        json!({"version":3,"map_catalog":map_source,"maps":scanned_maps,"coverage":coverage,"issues":issues,
        "research":{"quest_resources":quest_resources,"mobile_headers":mobile_headers,"explicit_locations":locations,
            "prepared_mobile_kinds":prepared_mobile_kinds,"prepared_mobile_examples":prepared_mobile_examples,
            "coverage_model":{"discovery":"counted_in_selected_vfs_scope","structural_decode":"partial",
                "semantic_interpretation":"partial","prototype_and_art_resolution":"partial",
                "placement_and_appearance_preparation":"partial","rendering":"not_evaluated",
                "collision_preparation":"partial","traversal":"not_evaluated","interaction_preparation":"partial",
                "dynamic_and_scripted_behavior":"not_evaluated"}},
        "elapsed_ms":start.elapsed().as_millis(),"vfs_reads":reads,"art_cache":art,
        "scope":{"map":options.map,"sector":options.sector,"limit_per_sector_kind":options.limit,
            "issue_filters":{"category":options.category,"group":options.group},
            "excluded":["dynamic template placement at every overworld coordinate","actual GPU submission during scan",
                "general scripts and mobile simulation","unparsed objects and unreferenced prototype or ART variants"]}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_sectors_never_count_as_decoded_or_collision_ready() {
        let mut coverage = Coverage::default();
        let mut cached = CachedSector::missing(0, "resource not found".into());
        account(&mut coverage, &cached.report);
        assert_eq!(coverage.object_lists_decoded, 0);
        assert_eq!(coverage.collision_prepared, 0);
        assert_eq!(coverage.sectors_with_surfaces, 0);
        assert_eq!(cached.report.structured_issues[0].code, "sector_missing");
        cached.report.source = Some("fixture".into());
        account(&mut coverage, &cached.report);
        assert_eq!(coverage.object_lists_rejected, 1);
        cached.report.object_list_failed = false;
        cached.report.decoded_records = Some(10);
        cached.report.scenery_count = 2;
        account(&mut coverage, &cached.report);
        assert_eq!(coverage.serialized_records_decoded, 10);
        assert_eq!(coverage.imported_scenery, 2);
        assert_eq!(coverage.collision_prepared, 0);
    }

    #[test]
    fn issue_examples_are_bounded_independently_of_accounting() {
        let mut groups = BTreeMap::new();
        let options = ScanOptions {
            examples: 2,
            ..ScanOptions::default()
        };
        for sector in 0..8 {
            add(
                &mut groups,
                &options,
                Some(5000),
                Some(sector),
                &json!("test"),
                &Issue::new("roof_unavailable", "fixture"),
            );
        }
        let group = &groups["roof_unavailable"];
        assert_eq!(group.count, 8);
        assert_eq!(group.examples.len(), 2);
        assert_eq!(group.sectors.len(), 8);
        assert_eq!(group.maps.len(), 1);
    }

    #[test]
    fn cause_filters_keep_parent_accounting_and_bound_evidence() {
        let mut groups = BTreeMap::new();
        let options = ScanOptions {
            group: Some("sector_preparation_memory_budget".into()),
            examples: 4,
            ..ScanOptions::default()
        };
        for sector in 0..5 {
            let evidence = crate::sector::objects::ListEvidence {
                declared_records: Some(1025),
                decoded_prefix_records: 1025,
                preparation_bytes: Some(9 * 1024 * 1024),
                preparation_budget_bytes: Some(8 * 1024 * 1024),
                ..Default::default()
            };
            add(
                &mut groups,
                &options,
                Some(5000),
                Some(sector),
                &json!("fixture"),
                &Issue::list_rejection(
                    "arbitrary human text",
                    "sector_preparation_memory_budget",
                    &evidence,
                ),
            );
        }
        let group = &groups["object_list_rejected"];
        assert_eq!(group.count, 5);
        assert_eq!(group.examples.len(), 4);
        let cause = &group.causes[&("sector_preparation_memory_budget", None)];
        assert_eq!(cause.counts.count, 5);
        assert_eq!(cause.counts.examples.len(), 2);
    }
}
