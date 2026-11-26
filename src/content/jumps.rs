//! Base-map jump points as serialized by CE's jumppoint_read_all.

use super::{
    Error, read_bounded,
    script::{Code, Script},
};
use crate::vfs::Vfs;
use std::collections::BTreeMap;

type Tile = (u32, u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Destination {
    /// None means the destination uses the same original map namespace.
    pub map: Option<i32>,
    pub tile: Tile,
}

pub(super) type Jumps = BTreeMap<Tile, Destination>;

const WIDTH: usize = 32;
const LIMIT: usize = 4096;

pub(super) fn load(vfs: &mut Vfs, map: &str) -> Result<Jumps, Box<dyn std::error::Error>> {
    let path = format!("maps/{map}/map.jmp");
    match read_bounded(vfs, &path, 4 + WIDTH * LIMIT) {
        Ok((source, bytes)) => {
            decode(&bytes).map_err(|error| format!("{path} from {source}: {error}").into())
        }
        Err(Error::Resource {
            error: crate::vfs::Error::NotFound(_),
            ..
        }) => Ok(Jumps::new()),
        Err(error) => Err(error.into()),
    }
}

fn decode(bytes: &[u8]) -> Result<Jumps, &'static str> {
    let (count, records) = bytes.split_first_chunk::<4>().ok_or("missing jump count")?;
    let count = u32::from_le_bytes(*count) as usize;
    if count > LIMIT || records.len() != count * WIDTH {
        return Err("invalid jump point table length");
    }
    let mut jumps = Jumps::new();
    for record in records.as_chunks::<WIDTH>().0 {
        let word = |offset| u32::from_le_bytes(record[offset..offset + 4].try_into().unwrap());
        let raw_map = word(16) as i32;
        let map = match raw_map {
            -1 => None,
            1.. => Some(
                raw_map
                    .checked_add(4999)
                    .ok_or("jump destination map overflow")?,
            ),
            _ => return Err("invalid jump destination map"),
        };
        // CE compares exact source coordinates and uses the first matching record.
        jumps.entry((word(8), word(12))).or_insert(Destination {
            map,
            tile: (word(24), word(28)),
        });
    }
    Ok(jumps)
}

/// Accept only a two-row unconditional Use handler that teleports its triggerer
/// to literal original coordinates and explicitly returns. Unused operand slots
/// in original scripts contain uninitialized words.
pub(super) fn use_destination(vfs: &mut Vfs, id: i32) -> Result<Destination, String> {
    let script = Script::load(vfs, id)?;
    decode_use(script.code()).map_err(|error| {
        format!(
            "{} from {}: {error}",
            script.source.virtual_path, script.source
        )
    })
}

pub(super) fn decode_use(code: &Code) -> Result<Destination, &'static str> {
    let unsupported = "Use script requires unsupported behavior";
    code.check_flags().map_err(|_| unsupported)?;
    let [teleport, ret] = code.statements.as_slice() else {
        return Err(unsupported);
    };
    if teleport.condition.opcode != 0
        || teleport.action.opcode != 44
        || teleport.action.focus(0) != 0 // SFO_TRIGGERER
        || ret.condition.opcode != 0
        || !matches!(ret.action.opcode, 1 | 2)
    {
        return Err(unsupported);
    }
    let map = teleport
        .action
        .literal(1)
        .filter(|&id| id >= 5000)
        .ok_or(unsupported)?;
    Ok(Destination {
        map: Some(map),
        tile: (
            teleport.action.literal(2).ok_or(unsupported)? as u32,
            teleport.action.literal(3).ok_or(unsupported)? as u32,
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconditional_script_decodes_a_literal_destination_only() {
        let mut bytes = vec![0; 64 + 2 * 132];
        for (offset, word) in [
            (52, 2u32),
            (108, 44),
            (124, 5012),
            (128, 99),
            (132, 91),
            (240, 1),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        bytes[112..116].copy_from_slice(&[0, 3, 3, 3]);
        assert_eq!(
            decode_use(&Code::decode(&bytes).unwrap()),
            Ok(Destination {
                map: Some(5012),
                tile: (99, 91)
            })
        );
        bytes[114] = 4;
        assert!(decode_use(&Code::decode(&bytes).unwrap()).is_err());
        assert!(Code::decode(&bytes[..64]).is_err());
    }

    #[test]
    fn length_and_original_map_numbering() {
        let mut bytes = Vec::new();
        for word in [
            2,
            0,
            0,
            109,
            93,
            1,
            0,
            61976,
            65664,
            0,
            0,
            3,
            4,
            u32::MAX,
            0,
            9,
            10,
        ] {
            bytes.extend(word.to_le_bytes());
        }
        let jumps = decode(&bytes).unwrap();
        assert_eq!(
            jumps[&(109, 93)],
            Destination {
                map: Some(5000),
                tile: (61976, 65664)
            }
        );
        assert_eq!(
            jumps[&(3, 4)],
            Destination {
                map: None,
                tile: (9, 10)
            }
        );
        assert!(decode(&bytes[..bytes.len() - 1]).is_err());
    }
}
