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
        } else if reason.contains("same-tile wall opening") || reason.contains("wall opening's") {
            "portal_opening_unsupported"
        } else if reason.contains("unsupported portal") {
            "portal_variant_unsupported"
        } else if reason.contains("unsupported scenery appearance")
            || reason.contains("unsupported wall appearance")
        {
            "object_appearance_unsupported"
        } else if issue.art_failure {
            "art_unresolved_or_rejected"
        } else {
            "object_import_rejected"
        };
        Self {
            category: Some(category),
            raw_art: Some(issue.art_id),
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
    authored_roof_entries: usize,
    roof_rasters_prepared: usize,
    surface_cells_prepared: usize,
    facade_cells_prepared: usize,
    mobile_resources_attempted: usize,
    mobile_records_decoded: usize,
    use_transitions_prepared: usize,
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
        || options
            .group
            .as_deref()
            .is_some_and(|code| issue.code != code)
    {
        return;
    }
    let group = groups.entry(issue.code).or_default();
    group.impact = issue.impact;
    group.count += 1;
    group.maps.extend(map);
    if let Some(path) = resource.as_str().or_else(|| resource["path"].as_str()) {
        group.resources.insert(path.to_owned());
    }
    group.sectors.extend(sector.map(|id| (map, id)));
    if group.examples.len() < options.examples.min(32) {
        group
            .examples
            .push(json!({"map":map,"sector":sector,"resource":resource,"issue":issue}));
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
    let mut groups = BTreeMap::new();
    let mut scanned_maps = Vec::new();
    let mut templates_seen = BTreeSet::new();
    let mut loader = SectorLoader {
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
            let (sector, _) = loader.load(id);
            coverage.authored_attempted += usize::from(files.sectors.contains(&id));
            coverage.explicit_location_attempted += usize::from(options.sector.is_some());
            account(&mut coverage, &sector.report);
            coverage.use_transitions_prepared += sector
                .interactions
                .values()
                .filter(|target| matches!(target.action, interaction::Action::Travel(_)))
                .count();
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
        if options.sector.is_none() {
            let prefix = format!("maps/{}/", map.directory);
            let mobiles: Vec<_> = loader
                .vfs
                .paths_with_prefix(&prefix)
                .into_iter()
                .filter(|path| path.ends_with(".mob"))
                .map(str::to_owned)
                .collect();
            for path in mobiles {
                coverage.mobile_resources_attempted += 1;
                let mobile = super::super::read_bounded(
                    &mut loader.vfs,
                    &path,
                    crate::prototype::MAX_PROTOTYPE_BYTES,
                )
                .map_err(|e| e.to_string())
                .and_then(|(_, bytes)| {
                    crate::sector::objects::decode_mobile(&bytes).map_err(|e| e.to_string())
                });
                match mobile {
                    Ok(_) => coverage.mobile_records_decoded += 1,
                    Err(error) => add(
                        &mut groups,
                        &options,
                        Some(map.id),
                        None,
                        &json!(path),
                        &Issue {
                            category: Some("mobile"),
                            ..Issue::new("mobile_format_rejected", &error)
                        },
                    ),
                }
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
    let issues: Vec<_> = groups.into_iter().map(|(code, group)| json!({"code":code,"impact":group.impact,
        "occurrences":group.count,"affected_maps":group.maps.len(),"affected_world_sectors":group.sectors.len(),"affected_resources":group.resources.len(),"examples":group.examples})).collect();
    let (reads, art) = loader.stats();
    Ok(
        json!({"version":1,"map_catalog":map_source,"maps":scanned_maps,"coverage":coverage,"issues":issues,
        "elapsed_ms":start.elapsed().as_millis(),"vfs_reads":reads,"art_cache":art,
        "scope":{"map":options.map,"sector":options.sector,"limit_per_sector_kind":options.limit,
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
}
