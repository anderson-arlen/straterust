//! Small, read-only StarCraft disc importer. Original formats end at this crate.
mod ai;
mod archive;
mod backwater;
mod burrow;
mod campaign;
mod campaign_units;
mod carried_resources;
mod flight;
mod formats;
mod hotkeys;
mod map_formats;
mod menus;
mod mission_terran;
mod source;
mod terran;
mod terran_data;
mod terran_media;
mod terran_ui;
mod version;
mod zerg;

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use straterust_engine::{
    assets::{AssetManifest, AssetPack, Image, ImageRef, TerrainGrid, encode_image},
    content::Package,
    map::{Terrain, encode_terrain},
    scenario::Scenario,
    sim::{
        Footprint, Map, MovementClass, PlayerId, Position, ResourceSpawn, Rules, Spawn,
        StartLocation, UnitType, UnitTypeId,
    },
};

use archive::{Archive, ArchiveMetadata};
use source::Source;

const IMPORT_REVISION: &str = "straterust-starcraft-preview-36";
// A single east-facing walk cycle, not a gameplay animation interpreter.
const MARINE_FRAMES: [usize; 9] = [76, 93, 110, 127, 144, 161, 178, 195, 212];
const TILE_INDEX: usize = 1;
const ASSET_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Serialize)]
struct SourceReport {
    kind: String,
    volume_label: Option<String>,
    bytes: u64,
    blake3: String,
    installer_offset: u64,
    installer_bytes: u64,
}

#[derive(Serialize)]
struct MemberReport {
    path: String,
    bytes: usize,
    blake3: String,
    category: String,
}

#[derive(Serialize)]
struct Inventory {
    schema_version: u32,
    importer: String,
    source: SourceReport,
    installer_archive: ArchiveMetadata,
    stardat_archive: ArchiveMetadata,
    windows: version::WindowsVersion,
    mac_readme_version: String,
    cd_data_version: String,
    stardat_data_version: String,
    members: Vec<MemberReport>,
    limitations: Vec<String>,
}

struct Payload {
    inventory: Inventory,
    wpe: Vec<u8>,
    vx4: Vec<u8>,
    vr4: Vec<u8>,
    grp: Vec<u8>,
    cv5: Vec<u8>,
    vf4: Vec<u8>,
}

#[derive(Serialize)]
struct ImportReport<'a> {
    schema_version: u32,
    inventory: &'a Inventory,
    terrain_tile: Option<usize>,
    map: Option<MapReport>,
    unit_grp_frames: &'a [usize],
    animation: &'a str,
    gameplay: &'a str,
    outputs: Vec<ImageRef>,
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("import failed: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let action = args
        .next()
        .context("use --help for inventory/import commands")?;
    if action == "--help" || action == "-h" {
        println!(
            "straterust-import-starcraft inventory --source PATH\nstraterust-import-starcraft import --source PATH --output DIR\nstraterust-import-starcraft import-map --source PATH --map ARCHIVE_MEMBER --terrain-only --output DIR\nstraterust-import-starcraft import-terran --source PATH --output DIR\nstraterust-import-starcraft import-backwater --source PATH --output DIR\nstraterust-import-starcraft import-campaign --source PATH --output DIR [--race all|terran|zerg|protoss] [--mission 1..5]\nstraterust-import-starcraft update-menus --source PATH --output EXISTING_GAME_OR_CAMPAIGN\nstraterust-import-starcraft update-hotkeys --source PATH --output EXISTING_PACKAGE_OR_CAMPAIGN\nstraterust-import-starcraft update-effects --source PATH --output EXISTING_PACKAGE_OR_CAMPAIGN\nstraterust-import-starcraft update-campaign --source PATH --output EXISTING_PACKAGE_OR_CAMPAIGN\n\nPATH: reference ISO, INSTALL.EXE, or directory containing INSTALL.EXE.\nInventory prints RON to stdout; import publishes a validated native preview.\nAn identical existing package is retained; different existing output is refused.\nupdate-hotkeys repairs command keys without changing artwork or gameplay.\nupdate-effects refreshes source effects without reimporting maps or gameplay.\nupdate-campaign also enables original automatic threat priorities.\nOnly the Windows retail disc subset is supported; no source files are modified."
        );
        return Ok(());
    }
    ensure!(
        action == "inventory"
            || action == "import"
            || action == "import-map"
            || action == "import-terran"
            || action == "import-backwater"
            || action == "update-effects"
            || action == "update-hotkeys"
            || action == "update-campaign"
            || action == "update-menus"
            || action == "import-campaign",
        "unknown command; use --help"
    );
    let mut input = None;
    let mut output = None;
    let mut map_member = None;
    let mut terrain_only = false;
    let mut mission_number = None;
    let mut race = None;
    let mut race_supplied = false;
    while let Some(arg) = args.next() {
        if arg == "--terrain-only" {
            ensure!(!terrain_only, "--terrain-only supplied twice");
            terrain_only = true;
            continue;
        }
        let value = args
            .next()
            .with_context(|| format!("missing value for {}", arg.to_string_lossy()))?;
        if arg == "--race" {
            ensure!(!race_supplied, "--race supplied twice");
            race_supplied = true;
            race = campaign::Race::parse(value.to_str().context("invalid race")?)?;
        } else if arg == "--mission" {
            ensure!(mission_number.is_none(), "--mission supplied twice");
            mission_number = Some(
                value
                    .to_str()
                    .context("invalid mission number")?
                    .parse::<u8>()?,
            );
        } else if arg == "--source" {
            ensure!(input.is_none(), "--source supplied twice");
            input = Some(PathBuf::from(value));
        } else if arg == "--output" {
            ensure!(output.is_none(), "--output supplied twice");
            output = Some(PathBuf::from(value));
        } else if arg == "--map" {
            ensure!(map_member.is_none(), "--map supplied twice");
            map_member = Some(
                value
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("map archive path must be UTF-8"))?,
            );
        } else {
            bail!("unknown option {}; use --help", arg.to_string_lossy());
        }
    }
    ensure!(
        action != "inventory" || output.is_none(),
        "inventory writes RON to stdout; omit --output"
    );
    ensure!(
        action == "import-campaign" || !race_supplied,
        "--race requires import-campaign"
    );
    let input = input.context("--source PATH is required")?;
    ensure!(
        action == "import-campaign" || mission_number.is_none(),
        "--mission requires import-campaign"
    );
    ensure!(
        action == "import-map" || (map_member.is_none() && !terrain_only),
        "--map and --terrain-only require import-map"
    );
    if action == "import-map" {
        ensure!(
            map_member.is_some() && terrain_only,
            "import-map requires --map ARCHIVE_MEMBER and --terrain-only; scenario behavior is not yet converted"
        );
    }
    if action != "inventory" {
        ensure!(output.is_some(), "import requires --output DIR");
    }
    if action == "update-menus" {
        return menus::refresh(&input, &output.context("update requires --output")?);
    }
    if action == "update-hotkeys" {
        return hotkeys::update(&input, &output.context("update requires --output")?);
    }
    if action == "update-effects" || action == "update-campaign" {
        return refresh::update_effects(
            &input,
            &output.context("update requires --output")?,
            action == "update-campaign",
        );
    }
    let payload = inspect(&input)?;
    if let Some(output) = output {
        if action == "import-campaign" && mission_number.is_none() {
            let created = campaign::publish(&payload, &input, &output, race)?;
            println!(
                "{} {} (first five missions per selected campaign; see campaign.ron)",
                if created {
                    "Imported"
                } else {
                    "Already identical:"
                },
                output.display()
            );
            return Ok(());
        }
        let files = if action == "import-campaign" {
            campaign::convert_race(
                &payload,
                &input,
                race.unwrap_or(campaign::Race::Terran),
                mission_number.context("import-campaign requires --mission 1..5")?,
            )?
        } else if action == "import-backwater" {
            backwater::convert(&payload, &input)?
        } else if action == "import-terran" {
            terran::convert(&payload, &input)?
        } else if let Some(map_member) = map_member {
            convert_map(&payload, &input, &map_member)?
        } else {
            convert(&payload)?
        };
        let created = publish(&output, &files)?;
        println!(
            "{} {} ({} native files; see import-report.ron for scope)",
            if created {
                "Imported"
            } else {
                "Already identical:"
            },
            output.display(),
            files.len()
        );
    } else {
        print!("{}", String::from_utf8(ron_bytes(&payload.inventory)?)?);
    }
    Ok(())
}

fn member<R: Read + std::io::Seek>(
    archive: &mut Archive<R>,
    name: &str,
    limit: usize,
    prefix: &str,
    category: &str,
    members: &mut Vec<MemberReport>,
) -> Result<Vec<u8>> {
    let bytes = archive.read_file(name, limit)?;
    members.push(MemberReport {
        path: format!("{prefix}/{name}"),
        bytes: bytes.len(),
        blake3: blake3::hash(&bytes).to_hex().to_string(),
        category: category.into(),
    });
    Ok(bytes)
}

fn inspect(path: &Path) -> Result<Payload> {
    let source = Source::open(path)?;
    let mut file = File::open(&source.path)?;
    let bytes = file.metadata()?.len();
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0; 65536];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let mut installer = if source.offset == 0 && source.len == bytes {
        Archive::open(&source.path)?
    } else {
        Archive::open_region(&source.path, source.offset, source.len)?
    };
    let mut members = Vec::new();
    let exe = member(
        &mut installer,
        "files\\starcraft.exe",
        4 * 1024 * 1024,
        "install",
        "Windows version evidence",
        &mut members,
    )?;
    let windows = version::windows_version(&exe)?;
    let cd_version = member(
        &mut installer,
        "rez\\CDversion.txt",
        4096,
        "install",
        "disc data version",
        &mut members,
    )?;
    let mac = member(
        &mut installer,
        "MacFiles\\DF.Starcraft Read Me",
        1024 * 1024,
        "install",
        "Macintosh version evidence (not the Windows target)",
        &mut members,
    )?;
    // The body uses MacRoman, but this version line is ASCII.
    ensure!(
        mac.starts_with(b"Starcraft\r"),
        "unrecognized Macintosh readme"
    );
    let mac_line = mac
        .split(|byte| *byte == b'\r' || *byte == b'\n')
        .find(|line| line.starts_with(b"Version "))
        .context("unrecognized Macintosh readme version")?;
    let mac_readme_version = std::str::from_utf8(mac_line)?.trim().to_owned();
    let stardat_bytes = member(
        &mut installer,
        "files\\stardat.mpq",
        128 * 1024 * 1024,
        "install",
        "base game archive",
        &mut members,
    )?;
    let mut stardat = Archive::from_bytes(stardat_bytes)?;
    let wpe = member(
        &mut stardat,
        "tileset\\badlands.wpe",
        ASSET_LIMIT,
        "stardat",
        "palette",
        &mut members,
    )?;
    let vx4 = member(
        &mut stardat,
        "tileset\\badlands.vx4",
        ASSET_LIMIT,
        "stardat",
        "terrain megatile graphics references",
        &mut members,
    )?;
    let vr4 = member(
        &mut stardat,
        "tileset\\badlands.vr4",
        ASSET_LIMIT,
        "stardat",
        "terrain minitile pixels",
        &mut members,
    )?;
    let grp = member(
        &mut stardat,
        "unit\\terran\\marine.grp",
        ASSET_LIMIT,
        "stardat",
        "unit sprite frames",
        &mut members,
    )?;
    let cv5 = member(
        &mut stardat,
        "tileset\\badlands.cv5",
        ASSET_LIMIT,
        "stardat",
        "terrain groups/buildability",
        &mut members,
    )?;
    let vf4 = member(
        &mut stardat,
        "tileset\\badlands.vf4",
        ASSET_LIMIT,
        "stardat",
        "minitile walkability/elevation",
        &mut members,
    )?;
    for (name, category) in [
        (
            "arr\\units.dat",
            "unit definitions; inventoried, not converted",
        ),
        (
            "arr\\images.dat",
            "image definitions; inventoried, not converted",
        ),
        (
            "scripts\\iscript.bin",
            "animation programs; inventoried, not interpreted",
        ),
    ] {
        member(
            &mut stardat,
            name,
            ASSET_LIMIT,
            "stardat",
            category,
            &mut members,
        )?;
    }
    let data_version = member(
        &mut stardat,
        "rez\\DataVersion.txt",
        4096,
        "stardat",
        "base archive data version",
        &mut members,
    )?;
    let inventory = Inventory {
        schema_version: 1,
        importer: IMPORT_REVISION.into(),
        source: SourceReport { kind: source.kind, volume_label: source.volume_label, bytes, blake3: hasher.finalize().to_hex().to_string(), installer_offset: source.offset, installer_bytes: source.len },
        installer_archive: installer.metadata(),
        stardat_archive: stardat.metadata(),
        windows,
        mac_readme_version,
        cd_data_version: std::str::from_utf8(&cd_version)?.trim().into(),
        stardat_data_version: std::str::from_utf8(&data_version)?.trim().into(),
        members,
        limitations: vec![
            "This importer was verified against the supplied English Windows retail v1.00 disc. The version fields and digest above identify this input; other releases are unverified. Macintosh evidence is recorded separately.".into(),
            "MPQ v0 with bounded raw, binary PKWARE, Huffman and mono/stereo ADPCM sector decoding; neutral/English Windows locale only. No patch overlays, expansion or remastered support.".into(),
            "Inventory covers known members, not a complete archive file listing.".into(),
            "Selected classic Badlands terrain, GRP/PCX artwork, WAV audio and Smacker portraits are converted into native assets. Selected source scripts become explicit native clips; no runtime IScript interpreter, general palette cycling or team-color remapping is provided. Backwater decorations retain source shadow coverage with approximate RGBA opacity.".into(),
            "import-map is limited to Badlands terrain/placement inspection: that command does not translate scenario triggers, custom gameplay or DAT rules. import-terran adds selected source data/art to an authored fixture; import-backwater translates the complete original Terran Mission 2 content and its required mechanics. Their individual reports describe supported behavior and remaining calibration limits.".into(),
            "Imported art retains its original ownership; keep the output private. The import report distinguishes the original fixture, map inspection and original Backwater campaign. Source content conversion does not establish complete StarCraft gameplay compatibility.".into(),
        ],
    };
    Ok(Payload {
        inventory,
        wpe,
        vx4,
        vr4,
        grp,
        cv5,
        vf4,
    })
}

fn ron_bytes(value: &impl Serialize) -> Result<Vec<u8>> {
    Ok(format!(
        "{}\n",
        ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default())?
    )
    .into_bytes())
}

type Files = BTreeMap<String, Vec<u8>>;

fn add_image(files: &mut Files, name: &str, image: &Image) -> Result<ImageRef> {
    let bytes = encode_image(image)?;
    let reference = ImageRef {
        file: name.into(),
        blake3: blake3::hash(&bytes).to_hex().to_string(),
    };
    files.insert(name.into(), bytes);
    Ok(reference)
}

fn native_files(terrain: &Image, frames: &[Image]) -> Result<Files> {
    let mut files = Files::new();
    for (name, bytes) in [
        (
            "manifest.ron",
            include_str!("../../../content/fixtures/manifest.ron"),
        ),
        (
            "rules.ron",
            include_str!("../../../content/fixtures/rules.ron"),
        ),
        ("map.ron", include_str!("../../../content/fixtures/map.ron")),
        (
            "scenario.ron",
            include_str!("../../../content/fixtures/scenario.ron"),
        ),
        (
            "presentation.ron",
            include_str!("../../../content/fixtures/presentation.ron"),
        ),
        (
            "client.ron",
            include_str!("../../../content/fixtures/client.ron"),
        ),
    ] {
        files.insert(name.into(), bytes.as_bytes().to_vec());
    }
    let terrain = add_image(&mut files, "terrain.srim", terrain)?;
    let mut refs = Vec::new();
    for (index, image) in frames.iter().enumerate() {
        refs.push(add_image(
            &mut files,
            &format!("marine-{index:02}.srim"),
            image,
        )?);
    }
    let first = frames.first().context("animation needs frames")?;
    let manifest = AssetManifest {
        schema_version: 1,
        terrain,
        terrain_grid: None,
        unit_type: UnitTypeId(1),
        unit_name: "Marine (east-facing art preview)".into(),
        frame_ms: 120,
        anchor: [first.width as i32 / 2, first.height as i32 / 2],
        frames: refs,
        clips: Vec::new(),
        extra_units: Vec::new(),
        resources: Vec::new(),
        carried_resources: Vec::new(),
        ui: Vec::new(),
        map_images: Vec::new(),
        scan_effect: None,
        projectiles: Vec::new(),
        damage_effects: None,
        gas_effects: None,
        creep: None,
        indicators: None,
    };
    manifest.validate()?;
    files.insert("assets.ron".into(), ron_bytes(&manifest)?);
    Ok(files)
}

fn convert(payload: &Payload) -> Result<Files> {
    let palette = formats::palette(&payload.wpe)?;
    let terrain = formats::decode_tile(&payload.vx4, &payload.vr4, &palette, TILE_INDEX)?;
    let decoded = formats::decode_grp(&payload.grp, &palette)?;
    let frames: Vec<_> = MARINE_FRAMES
        .iter()
        .map(|&index| {
            decoded.get(index).cloned().with_context(|| {
                format!("Marine GRP lacks preview frame {index}; unsupported source")
            })
        })
        .collect::<Result<_>>()?;
    let mut files = native_files(&terrain, &frames)?;
    append_report(&mut files, payload, None, None)?;
    Ok(files)
}

fn append_report(
    files: &mut Files,
    payload: &Payload,
    map: Option<MapReport>,
    gameplay: Option<&str>,
) -> Result<()> {
    files.remove("import-report.ron");
    let assets: AssetManifest = ron::de::from_bytes(&files["assets.ron"])?;
    let animated = !assets.clips.is_empty();
    let animation = if files.contains_key("mission.ron") {
        "Native directional/action clips cover the mission roster's movement, combat, work, construction, production, death, burrow and building flight. Scanner effects and static source decorations are included. assets.ron and the unit/flight reference reports record source frames, finite sequences and remaining presentation approximations; no source script VM runs at runtime."
    } else if animated {
        "Native directional/action clips use selected source frames for idle, movement, combat, work, construction, production and death. assets.ron and terran-reference.ron record source mappings, timing and presentation approximations; no source script VM runs at runtime."
    } else {
        "Fixed east-facing walk preview at 120 ms/frame, looped by presentation time. This duration is provisional, not reference simulation timing."
    };
    let outputs = files
        .iter()
        .map(|(name, bytes)| ImageRef {
            file: name.clone(),
            blake3: blake3::hash(bytes).to_hex().to_string(),
        })
        .collect();
    let report = ImportReport {
        schema_version: 1,
        inventory: &payload.inventory,
        terrain_tile: if map.is_some() {
            None
        } else {
            Some(TILE_INDEX)
        },
        unit_grp_frames: if animated { &[] } else { &MARINE_FRAMES },
        animation,
        gameplay: gameplay.unwrap_or(if map.is_some() {
            "Terrain/placement inspection only. Original scenario behavior is not executed. Preview Marines at starts are additions, not original placed units; movement uses fixture rules."
        } else {
            "Unmodified original fixture rules/map/scenario. Unit type 1 uses Marine art; type 2 retains geometric fallback. No StarCraft map or mechanics are converted."
        }),
        map,
        outputs,
    };
    files.insert("import-report.ron".into(), ron_bytes(&report)?);
    Ok(())
}

#[derive(Serialize)]
struct MapReport {
    member: String,
    scm_blake3: Option<String>,
    chk_blake3: String,
    dimensions_tiles: [u16; 2],
    unique_megatiles: usize,
    source_unit_records: usize,
    resources: usize,
    starts: usize,
    added_preview_marines: usize,
    owners: [u8; 12],
    races: [u8; 12],
    sections: Vec<(String, usize)>,
    unconverted: Vec<String>,
}

fn write_map_terrain(
    payload: &Payload,
    files: &mut Files,
    parsed: &map_formats::ParsedMap,
    terrain: &map_formats::DecodedTerrain,
) -> Result<usize> {
    let palette = formats::palette(&payload.wpe)?;
    let mut indices = BTreeMap::new();
    for &megatile in &terrain.megatile_indices {
        indices.entry(megatile).or_insert(0_u32);
    }
    ensure!(
        indices.len() <= 4096,
        "map requires more than 4096 distinct graphics tiles"
    );
    let atlas_columns = indices.len().min(64) as u32;
    let atlas_rows = (indices.len() as u32).div_ceil(atlas_columns);
    let mut atlas = Image {
        width: atlas_columns * 32,
        height: atlas_rows * 32,
        rgba: vec![0; (atlas_columns * atlas_rows * 32 * 32 * 4) as usize],
    };
    for (index, (megatile, native)) in indices.iter_mut().enumerate() {
        *native = index as u32;
        let tile =
            formats::decode_tile(&payload.vx4, &payload.vr4, &palette, usize::from(*megatile))?;
        let x = *native % atlas_columns * 32;
        let y = *native / atlas_columns * 32;
        for row in 0..32 {
            let offset = ((y + row) * atlas.width + x) as usize * 4;
            atlas.rgba[offset..offset + 128]
                .copy_from_slice(&tile.rgba[row as usize * 128..(row as usize + 1) * 128]);
        }
    }
    let mut assets: AssetManifest = ron::de::from_bytes(&files["assets.ron"])?;
    assets.terrain = add_image(files, "terrain.srim", &atlas)?;
    assets.terrain_grid = Some(TerrainGrid {
        tile_size: 32,
        columns: u32::from(parsed.width),
        rows: u32::from(parsed.height),
        tiles: terrain
            .megatile_indices
            .iter()
            .map(|tile| indices[tile])
            .collect(),
    });
    files.insert("assets.ron".into(), ron_bytes(&assets)?);
    files.insert(
        "terrain.srtm".into(),
        encode_terrain(&Terrain {
            cell_size: 8,
            columns: u32::from(parsed.width) * 4,
            rows: u32::from(parsed.height) * 4,
            flags: terrain.flags.clone(),
        })?,
    );
    Ok(indices.len())
}

fn convert_map(payload: &Payload, source_path: &Path, map_member: &str) -> Result<Files> {
    let source = Source::open(source_path)?;
    let mut archive = Archive::open_region(&source.path, source.offset, source.len)?;
    let scm = archive.read_file(map_member, 16 * 1024 * 1024)?;
    let scm_blake3 = blake3::hash(&scm).to_hex().to_string();
    let mut archive = Archive::from_bytes(scm)?;
    let chk = archive.read_file("staredit\\scenario.chk", 8 * 1024 * 1024)?;
    let parsed = map_formats::parse_chk(&chk)?;
    ensure!(
        parsed.tileset == 0,
        "terrain-only previews support Badlands; campaign imports support the other retail tilesets"
    );
    let terrain = map_formats::decode_terrain(&parsed, &payload.cv5, &payload.vf4)?;
    let mut files = convert(payload)?;
    let unique_megatiles = write_map_terrain(payload, &mut files, &parsed, &terrain)?;
    let mut assets: AssetManifest = ron::de::from_bytes(&files["assets.ron"])?;
    assets.unit_name = "Terrain and placement inspection".into();
    files.insert("assets.ron".into(), ron_bytes(&assets)?);
    let mut map = Map {
        initial_explored: Default::default(),
        creation: Default::default(),
        ai: Vec::new(),
        mission: None,
        fog_of_war: false,
        // Raw CHK hashes belong to provenance: ignored text/art must not change gameplay identity.
        id: "straterust.map-inspection".into(),
        width: i32::from(parsed.width) * 32,
        height: i32::from(parsed.height) * 32,
        players: 12,
        spawns: Vec::new(),
        terrain: None,
        start_locations: Vec::new(),
        resources: Vec::new(),
    };
    let mut rules: Rules = ron::de::from_str(include_str!("../../../content/fixtures/rules.ron"))?;
    let mut unconverted = parsed.unsupported.clone();
    unconverted.push("Resources preserve neutral positions and quantities; mineral variants use a common original geometric marker rather than their source art.".into());
    for unit in &parsed.units {
        let position = Position {
            x: i32::from(unit.x),
            y: i32::from(unit.y),
        };
        match unit.unit_type {
            176..=178 | 188 => {
                ensure!(
                    unit.owner == 11,
                    "non-neutral resource ownership is unsupported (UNIT serial {})",
                    unit.serial
                );
                map.resources.push(ResourceSpawn {
                    requires_extractor: false,
                    footprint: if unit.unit_type == 188 {
                        Footprint {
                            width: 128,
                            height: 64,
                        }
                    } else {
                        Footprint {
                            width: 64,
                            height: 32,
                        }
                    },
                    kind: if unit.unit_type == 188 {
                        "gas"
                    } else {
                        "minerals"
                    }
                    .into(),
                    position,
                    amount: unit.resource_amount.with_context(|| {
                        format!(
                            "resource at {},{} lacks an explicit amount; unsupported map",
                            unit.x, unit.y
                        )
                    })?,
                });
            }
            214 => map.start_locations.push(StartLocation {
                player: PlayerId(u16::from(unit.owner)),
                position,
            }),
            source_type => {
                let id = UnitTypeId(source_type + 1);
                if !rules.units.iter().any(|unit| unit.id == id) {
                    rules.units.push(UnitType {
                        id,
                        speed: 1,
                        footprint: Footprint::default(),
                        movement_class: MovementClass::Ground,
                        ..UnitType::default()
                    });
                }
                map.spawns.push(Spawn {
                    owner: PlayerId(u16::from(unit.owner)),
                    unit_type: id,
                    position,
                    ..Spawn::default()
                });
                unconverted.push(format!("UNIT serial {} type {} retains placement only; artwork, footprint, health, state and movement semantics use inspection placeholders", unit.serial, source_type));
            }
        }
    }
    for start in &map.start_locations {
        map.spawns.push(Spawn {
            owner: start.player,
            unit_type: UnitTypeId(1),
            position: start.position,
            ..Spawn::default()
        });
    }
    ensure!(
        !map.spawns.is_empty(),
        "inspection needs a placed unit or start location"
    );
    files.insert("map.ron".into(), ron_bytes(&map)?);
    files.insert("rules.ron".into(), ron_bytes(&rules)?);
    files.insert(
        "scenario.ron".into(),
        ron_bytes(&Scenario {
            schema_version: 1,
            seed: 42,
            ticks: 240,
            commands: Vec::new(),
        })?,
    );
    let report = MapReport {
        member: map_member.into(),
        scm_blake3: Some(scm_blake3),
        chk_blake3: blake3::hash(&chk).to_hex().to_string(),
        dimensions_tiles: [parsed.width, parsed.height],
        unique_megatiles,
        source_unit_records: parsed.units.len(),
        resources: map.resources.len(),
        starts: map.start_locations.len(),
        added_preview_marines: map.start_locations.len(),
        owners: parsed.owners,
        races: parsed.races,
        sections: parsed
            .sections
            .iter()
            .map(|section| (section.name.clone(), section.bytes))
            .collect(),
        unconverted,
    };
    append_report(&mut files, payload, Some(report), None)?;
    Ok(files)
}

/// Stage beside the destination, validate, then rename. Never replace different output.
fn publish(output: &Path, files: &Files) -> Result<bool> {
    ensure!(
        output.file_name().is_some(),
        "output must name a package directory"
    );
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create output parent {}", parent.display()))?;
    let stage = tempfile::Builder::new()
        .prefix(".straterust-import-")
        .tempdir_in(parent)?;
    for (name, bytes) in files {
        ensure!(
            !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\']),
            "invalid output filename {name}"
        );
        fs::write(stage.path().join(name), bytes)?;
    }
    let world = Package::load(stage.path())
        .context("validate staged gameplay package")?
        .world(0)?;
    AssetPack::load(stage.path())
        .context("validate staged presentation")?
        .context("staged package has no assets")?
        .validate_for_world(&world)
        .context("validate staged presentation mapping")?;
    if let Some(media) =
        straterust_engine::media::MediaPack::load(stage.path()).context("validate staged media")?
    {
        media
            .validate_world(&world)
            .context("validate staged media mappings")?;
    }
    match fs::symlink_metadata(output) {
        Ok(metadata) => {
            ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "existing output is not a regular directory; choose another output path"
            );
            ensure!(
                fs::read_dir(output)?.count() == files.len(),
                "existing output differs; choose a new output directory"
            );
            for (name, bytes) in files {
                let path = output.join(name);
                let metadata = fs::symlink_metadata(&path).with_context(|| {
                    format!("existing output differs at {name}; choose a new output directory")
                })?;
                ensure!(
                    metadata.is_file() && metadata.len() == bytes.len() as u64,
                    "existing output differs at {name}; choose a new output directory"
                );
                let mut current = Vec::new();
                File::open(path)?
                    .take(bytes.len() as u64 + 1)
                    .read_to_end(&mut current)?;
                ensure!(
                    current == *bytes,
                    "existing output differs at {name}; choose a new output directory"
                );
            }
            Ok(false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::rename(stage.path(), output)
                .with_context(|| format!("publish {}", output.display()))?;
            Ok(true)
        }
        Err(error) => Err(error).context("inspect output directory"),
    }
}

#[cfg(test)]
mod tests;

mod refresh;
