use arcanum_rs::content::effects::{EffectDefinition, EffectId, EffectOperation};
use arcanum_rs::content::items::{GenericDefinition, ItemId, ResistanceKind};
use arcanum_rs::dat::{Archive, Entry};
use arcanum_rs::resource_path::normalize;
use arcanum_rs::runtime::{
    ActivationId, ActorId, EffectActivation, ItemDefinitionRef, ItemInstance, Runtime,
};
use arcanum_rs::vfs::{Policy, Vfs};
use flate2::{Compression, write::ZlibEncoder};
use std::fs::{self, File};
use std::hint::black_box;
use std::io::{self, BufWriter, Write};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::time::Instant;

const SAMPLES: usize = 7;
const GRANTS_PER_ACTOR: usize = 4;
const ENTRIES: usize = 4096;
const PLAIN_PATH: &str = "bench/asset-00000.bin";
const ZLIB_PATH: &str = "bench/asset-00001.bin";
const MES_PATH: &str = "bench/asset-00002.bin";

fn measure(label: &str, iterations: usize, mut operation: impl FnMut()) {
    let mut batch = || {
        for _ in 0..iterations {
            operation();
        }
    };
    batch();
    let mut samples = [0.0; SAMPLES];
    for sample in &mut samples {
        let start = Instant::now();
        batch();
        *sample = start.elapsed().as_secs_f64() * 1e9 / iterations as f64;
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "{label:26} {:>10.1} ns/op [{:.1}, {:.1}] ({iterations}/sample)",
        samples[SAMPLES / 2],
        samples[0],
        samples[SAMPLES - 1],
    );
}

fn definitions() -> (GenericDefinition, EffectDefinition) {
    use EffectOperation::{AddArmourClass, AddResistance, ResistanceFloor};
    use ResistanceKind::{Electrical, Fire, Poison};
    let item = GenericDefinition {
        id: ItemId("bench:item/ward".into()),
        display_name: "Benchmark ward".into(),
        base_weight: 1,
        base_worth: 0,
        magic_tech_complexity: 0,
    };
    let effect = EffectDefinition {
        id: EffectId("bench:effect/ward".into()),
        operations: vec![
            AddArmourClass { points: 1 },
            ResistanceFloor {
                channel: Fire,
                minimum_percent: 50,
            },
            AddResistance {
                channel: Electrical,
                percentage_points: 8,
            },
            ResistanceFloor {
                channel: Electrical,
                minimum_percent: 30,
            },
            ResistanceFloor {
                channel: Poison,
                minimum_percent: 97,
            },
        ],
    };
    (item, effect)
}

fn populate<'a>(
    count: usize,
    item: &'a GenericDefinition,
    effect: &'a EffectDefinition,
) -> (Runtime<'a>, Vec<(ActorId, ActivationId)>) {
    let mut runtime = Runtime::default();
    let actors = (0..count)
        .map(|_| {
            let actor = runtime.create_actor(10).unwrap();
            runtime
                .set_underlying_electrical_resistance(actor, 30)
                .unwrap();
            let mut last = None;
            for _ in 0..2 {
                let source = runtime
                    .instantiate_item(ItemDefinitionRef::Generic(item))
                    .unwrap();
                runtime.equip(source, actor).unwrap();
                for _ in 0..GRANTS_PER_ACTOR / 2 {
                    last = Some(runtime.activate_effect(source, actor, effect).unwrap());
                }
            }
            (actor, last.unwrap())
        })
        .collect();
    (runtime, actors)
}

fn runtime_bench() {
    let (item, effect) = definitions();
    for (count, samples) in [(1, 100_000), (100, 10_000), (1000, 2_000), (5000, 400)] {
        let start = Instant::now();
        let (mut runtime, actors) = populate(count, &item, &effect);
        let setup = start.elapsed();
        let (actor, _) = actors[0];
        let extra = runtime
            .instantiate_item(ItemDefinitionRef::Generic(&item))
            .unwrap();
        println!(
            "\n{count} actors, {} activations, setup {:.3} ms",
            count * GRANTS_PER_ACTOR,
            setup.as_secs_f64() * 1000.0,
        );
        let mut cycle = actors.iter().copied().cycle();
        measure("actor base AC lookup", samples * 10, || {
            let (actor, _) = black_box(cycle.next().unwrap());
            black_box(runtime.base_armour_class(actor).unwrap());
        });
        measure("activation lookup", samples * 10, || {
            let (_, id) = black_box(cycle.next().unwrap());
            black_box(runtime.activation(id).unwrap());
        });
        measure("effective AC", samples, || {
            let (actor, _) = black_box(cycle.next().unwrap());
            black_box(runtime.armour_class(actor).unwrap());
        });
        measure("three resistance queries", samples, || {
            let (actor, _) = black_box(cycle.next().unwrap());
            black_box(runtime.fire_resistance(actor).unwrap());
            black_box(runtime.electrical_resistance(actor).unwrap());
            black_box(runtime.poison_resistance(actor).unwrap());
        });
        runtime.equip(extra, actor).unwrap();
        measure("activate + remove", samples, || {
            let id = runtime
                .activate_effect(extra, actor, black_box(&effect))
                .unwrap();
            runtime.remove_activation(black_box(id)).unwrap();
        });
        runtime.unequip(extra, actor).unwrap();
        measure("equip + grant + unequip", samples, || {
            runtime.equip(black_box(extra), actor).unwrap();
            black_box(runtime.activate_effect(extra, actor, &effect).unwrap());
            runtime.unequip(extra, actor).unwrap();
        });
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> io::Result<Self> {
        let path = std::env::temp_dir().join(format!("arcanum-baseline-{}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn archive_fixture(path: &Path, entries: usize) -> io::Result<()> {
    let mut seed = 0x1234_5678u32;
    let block: Vec<_> = (0..16_384)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as u8
        })
        .collect();
    let plain = block.repeat(4);
    let mut compressor = ZlibEncoder::new(Vec::new(), Compression::default());
    compressor.write_all(&plain)?;
    let compressed = compressor.finish()?;
    let mut mes = String::new();
    for id in 0..1024 {
        use std::fmt::Write as _;
        writeln!(mes, "{{{id}}}{{Synthetic content entry}}").unwrap();
    }
    let mut file = BufWriter::new(File::create(path)?);
    for chunk in [&plain[..], &compressed, mes.as_bytes()] {
        file.write_all(chunk)?;
    }
    let payload_size = (plain.len() + compressed.len() + mes.len()) as u32;
    let word = |file: &mut BufWriter<File>, n: u32| file.write_all(&n.to_le_bytes());
    word(&mut file, payload_size + 4)?;
    word(&mut file, entries as u32)?;
    let mut name_bytes = 0;
    for index in 0..entries {
        let name = format!("bench/asset-{index:05}.bin\0");
        name_bytes += name.len() as u32;
        word(&mut file, name.len() as u32)?;
        file.write_all(name.as_bytes())?;
        let (flags, size, stored, offset) = match index {
            1 => (2, plain.len(), compressed.len(), plain.len()),
            2 => (1, mes.len(), mes.len(), plain.len() + compressed.len()),
            _ => (1, plain.len(), plain.len(), 0),
        };
        for value in [0, flags, size as u32, stored as u32, offset as u32] {
            word(&mut file, value)?;
        }
    }
    file.write_all(b" TAD")?;
    word(&mut file, name_bytes)?;
    word(&mut file, entries as u32 * 24 + name_bytes + 16)?;
    file.flush()
}

fn resource_bench() -> io::Result<()> {
    let scratch = Scratch::new()?;
    let file = scratch.0.join("archive.dat");
    archive_fixture(&file, ENTRIES)?;
    let mut one = Vfs::new(Policy::Ordered);
    let mut four = Vfs::new(Policy::Ordered);
    one.mount_archive(&file).unwrap();
    let mut winner = four.mount_archive(&file).unwrap();
    for index in 1..4 {
        let other = scratch.0.join(format!("archive-{index}.dat"));
        fs::copy(&file, &other)?;
        winner = four.mount_archive(other).unwrap();
    }
    let hit = r".\BENCH\ASSET-00001.BIN";
    assert_eq!(
        four.resolve(hit).unwrap().winner().unwrap().mount.id,
        winner
    );
    println!("\n{ENTRIES} entries/archive, 64 KiB binary payloads, 1024 MES entries");
    measure("DAT open + drop", 30, || {
        black_box(Archive::open(black_box(&file)).unwrap());
    });
    measure("normalize", 100_000, || {
        black_box(normalize(black_box(hit)).unwrap());
    });
    measure("resolve 1 mount, hit", 50_000, || {
        black_box(one.resolve(black_box(hit)).unwrap());
    });
    measure("resolve 4 mounts, hit", 50_000, || {
        black_box(four.resolve(black_box(hit)).unwrap());
    });
    measure("resolve 4 mounts, miss", 50_000, || {
        black_box(four.resolve(black_box("bench/missing.bin")).unwrap());
    });
    for (label, path) in [("VFS plain copy", PLAIN_PATH), ("VFS zlib copy", ZLIB_PATH)] {
        measure(label, 4000, || {
            black_box(four.copy(black_box(path), io::sink()).unwrap());
        });
    }
    measure("VFS MES load + drop", 300, || {
        black_box(arcanum_rs::content::load_mes(&mut four, black_box(MES_PATH)).unwrap());
    });
    Ok(())
}

fn memory() -> (Option<u64>, Option<u64>) {
    let value = |file: &str, key: &str| {
        fs::read_to_string(file).ok()?.lines().find_map(|line| {
            line.strip_prefix(key)?
                .split_ascii_whitespace()
                .next()?
                .parse()
                .ok()
        })
    };
    (
        value("/proc/self/smaps_rollup", "Rss:"),
        value("/proc/self/status", "VmHWM:"),
    )
}

fn memory_line() {
    let (rss, peak) = memory();
    println!("RSS KiB {rss:?}, peak RSS KiB {peak:?}");
}

fn runtime_memory(count: usize) {
    let (item, effect) = definitions();
    memory_line();
    let (runtime, actors) = populate(count, &item, &effect);
    println!(
        "{count} actors, {} activations, byte sizes Runtime {} ItemInstance {} EffectActivation {} ID {}",
        count * GRANTS_PER_ACTOR,
        size_of::<Runtime<'_>>(),
        size_of::<ItemInstance<'_>>(),
        size_of::<EffectActivation<'_>>(),
        size_of::<ActorId>()
    );
    memory_line();
    black_box((&runtime, &actors));
}

fn archive_memory(entries: usize) -> io::Result<()> {
    let scratch = Scratch::new()?;
    let file = scratch.0.join("archive.dat");
    archive_fixture(&file, entries)?;
    memory_line();
    let archive = Archive::open(&file).unwrap();
    let paths: usize = archive
        .entries()
        .iter()
        .map(|entry| entry.path.capacity())
        .sum();
    println!(
        "{entries} entries, file bytes {}, Entry bytes {}, index slot bytes {}, path capacity bytes {paths}",
        fs::metadata(file)?.len(),
        size_of::<Entry>(),
        size_of::<usize>()
    );
    memory_line();
    black_box(&archive);
    Ok(())
}

fn main() -> io::Result<()> {
    let args: Vec<_> = std::env::args()
        .skip(1)
        .filter(|arg| arg != "--bench")
        .collect();
    match args.as_slice() {
        [] => {
            runtime_bench();
            resource_bench()?;
        }
        [mode] if mode == "--test" => {
            let (item, effect) = definitions();
            let (runtime, actors) = populate(1, &item, &effect);
            assert_eq!(runtime.armour_class(actors[0].0).unwrap(), 14);
        }
        [mode] if mode == "runtime" => runtime_bench(),
        [mode] if mode == "resource" => resource_bench()?,
        [mode, count] if mode == "runtime-memory" => {
            runtime_memory(bounded_count(count, 1, 5000)?);
        }
        [mode, count] if mode == "archive-memory" => {
            archive_memory(bounded_count(count, 3, 65_536)?)?;
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected runtime, resource, runtime-memory <actors> or archive-memory <entries>",
            ));
        }
    }
    Ok(())
}

fn bounded_count(value: &str, min: usize, max: usize) -> io::Result<usize> {
    value
        .parse()
        .ok()
        .filter(|count| (min..=max).contains(count))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("count must be between {min} and {max}"),
            )
        })
}
