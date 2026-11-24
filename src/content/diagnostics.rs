//! Structured provenance from winning VFS records.
use super::{Source, art, read_bounded};
use crate::{
    prototype,
    vfs::{Candidate, Vfs},
};
use serde_json::{Value, json};

pub(crate) fn source(source: &Source) -> Value {
    json!({
        "path": source.virtual_path,
        "mount_id": source.mount_id.to_string(),
        "mount_path": source.mount_path,
        "loose_path": source.loose_path,
        "storage": source.storage.to_string(),
        "mod_id": source.mod_id.as_ref().map(ToString::to_string),
    })
}

fn candidate(record: Candidate<'_>) -> Value {
    json!({
        "mount_id": record.mount.id.to_string(),
        "mount_path": record.mount.path,
        "mod_id": record.mount.mod_id.as_ref().map(ToString::to_string),
        "storage": record.storage.to_string(),
        "size_bytes": record.size,
        "loose_path": record.loose_path,
        "exclusion": record.exclusion.map(|reason| reason.to_string()),
    })
}

pub fn resource(
    vfs: &mut Vfs,
    path: &str,
    frame: Option<usize>,
) -> Result<Value, crate::vfs::Error> {
    let found = vfs.resolve(path)?;
    let path = found.path.clone();
    let winner = found.winner().map(candidate);
    let candidates: Vec<_> = found
        .candidates()
        .iter()
        .copied()
        .take(64)
        .map(candidate)
        .collect();
    let candidate_count = found.candidates().len();
    let kind = path.rsplit_once('.').map(|(_, ext)| ext);
    let decode = match (winner.is_some(), kind, frame) {
        (true, Some("art"), Some(frame)) => match art::load_frame(vfs, &path, frame) {
            Ok(asset) => {
                json!({"status":"decoded","frame":frame,"frame_count":asset.decoded.frame_count,
                "rotations":asset.decoded.rotation_count,"size":[asset.decoded.frame.width,asset.decoded.frame.height]})
            }
            Err(error) => {
                json!({"status":"failed","code":"art_decode_failed","message":error.to_string()})
            }
        },
        (true, Some("pro"), None) => match read_bounded(vfs, &path, prototype::MAX_PROTOTYPE_BYTES)
            .map_err(|err| err.to_string())
            .and_then(|(_, bytes)| prototype::decode_header(&bytes).map_err(|err| err.to_string()))
        {
            Ok(header) => {
                json!({"status":"header_decoded","version":header.version,"prototype":header.id.0,"type":header.object_type})
            }
            Err(error) => {
                json!({"status":"failed","code":"prototype_header_failed","message":error})
            }
        },
        (false, _, _) => json!({"status":"missing"}),
        (_, _, Some(_)) => json!({"status":"unsupported","code":"frame_requires_art"}),
        _ => json!({"status":"untested"}),
    };
    Ok(
        json!({"path":path,"kind":kind,"winner":winner,"candidates":candidates,
        "candidate_count":candidate_count,"decode":decode}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::Policy;

    #[test]
    fn resource_diagnostics_distinguish_override_decode_failure_and_missing() {
        let root = std::env::temp_dir().join(format!(
            "arcanum-resource-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let base = root.join("base");
        let overlay = root.join("overlay");
        for directory in [&base, &overlay] {
            std::fs::create_dir_all(directory).unwrap();
            std::fs::write(directory.join("fixture.pro"), b"short prototype").unwrap();
        }
        let mut vfs = Vfs::new(Policy::Ordered);
        vfs.mount_directory(&base).unwrap();
        vfs.mount_directory(&overlay).unwrap();
        let result = resource(&mut vfs, "FIXTURE.PRO", None).unwrap();
        assert_eq!(result["candidate_count"], 2);
        assert_eq!(result["winner"]["mount_path"], json!(overlay));
        assert_eq!(result["decode"]["status"], "failed");
        let missing = resource(&mut vfs, "missing.pro", None).unwrap();
        assert_eq!(missing["winner"], Value::Null);
        assert_eq!(missing["decode"]["status"], "missing");
        drop(vfs);
        std::fs::remove_dir_all(root).unwrap();
    }
}
