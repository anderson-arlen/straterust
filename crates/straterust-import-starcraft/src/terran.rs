//! Private reference art and selected DAT fields on the original Terran demo.
//! Behavioral rules and the authored scenario remain explicit approximations.
//! Binary layout facts: https://github.com/poiuyqwert/PyMS/blob/master/PyMS/FileFormats/IScriptBIN.py
//! Animation meanings: https://github.com/OpenBW/openbw/blob/master/bwgame.h
//! Only verified source poses are converted; this is not an iscript interpreter.
use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use straterust_engine::{
    assets::{
        AssetManifest, ClipFrame, ClipKind, Image, ResourceManifest, SpriteClip, SpriteManifest,
    },
    sim::{Footprint, ResourceAmount, Rules, UnitTypeId, Weapon},
};

use crate::{
    Archive, Files, MemberReport, Payload, Source, add_image, append_report, formats, member,
    ron_bytes,
    terran_data::{self, ReferenceUnit},
};

const UNIT_MAPPING: [(u16, u16); 5] = [(0, 1), (7, 2), (106, 3), (109, 4), (111, 5)];
const GAMEPLAY: &str = "Playable original Terran demonstration with selected Windows v1.00 DAT fields and private reference art. The map, scenario, gathering, construction, movement and combat behavior are authored/provisional; this is not a compatible StarCraft scenario. See terran-reference.ron for source hashes and unverified behavior.";
const BARRACKS_WORK: [(u16, u8); 7] = [(7, 4), (8, 2), (7, 2), (8, 2), (7, 4), (8, 2), (7, 2)];
const CONTROL_WORK_WAITS: [u8; 12] = [4, 1, 3, 3, 4, 1, 3, 3, 4, 3, 1, 1];

#[derive(Serialize)]
struct ReferenceReport<'a> {
    schema_version: u32,
    source_to_native_units: &'a [(u16, u16)],
    units: &'a [ReferenceUnit],
    movement: &'a [terran_data::ReferenceMotion],
    members: Vec<MemberReport>,
    animation_evidence: Vec<&'static str>,
    uncertainties: Vec<&'static str>,
}

pub fn convert(payload: &Payload, source_path: &Path) -> Result<Files> {
    let source = Source::open(source_path)?;
    let mut installer = Archive::open_region(&source.path, source.offset, source.len)?;
    let mut members = Vec::new();
    let stardat = member(
        &mut installer,
        "files\\stardat.mpq",
        128 * 1024 * 1024,
        "install",
        "Terran reference base archive",
        &mut members,
    )?;
    let executable = member(
        &mut installer,
        "files\\starcraft.exe",
        2 * 1024 * 1024,
        "install",
        "read-only source direction table for worker effect offsets; never executed",
        &mut members,
    )?;
    let work_offsets = source_work_offsets(&executable)?;
    verify_timing_source(&executable)?;
    let mut archive = Archive::from_bytes(stardat)?;
    let units = member(
        &mut archive,
        "arr\\units.dat",
        19192,
        "stardat",
        "selected original unit gameplay fields",
        &mut members,
    )?;
    let weapons = member(
        &mut archive,
        "arr\\weapons.dat",
        4200,
        "stardat",
        "selected original ground weapon fields",
        &mut members,
    )?;
    let reference = terran_data::decode(&units, &weapons)?;
    let mut files = crate::convert(payload)?;
    for (name, bytes) in [
        (
            "manifest.ron",
            include_bytes!("../../../content/terran-demo/manifest.ron").as_slice(),
        ),
        (
            "rules.ron",
            include_bytes!("../../../content/terran-demo/rules.ron").as_slice(),
        ),
        (
            "map.ron",
            include_bytes!("../../../content/terran-demo/map.ron").as_slice(),
        ),
        (
            "scenario.ron",
            include_bytes!("../../../content/terran-demo/scenario.ron").as_slice(),
        ),
        (
            "presentation.ron",
            include_bytes!("../../../content/terran-demo/presentation.ron").as_slice(),
        ),
        (
            "client.ron",
            include_bytes!("../../../content/terran-demo/client.ron").as_slice(),
        ),
        (
            "terrain.srtm",
            include_bytes!("../../../content/terran-demo/terrain.srtm").as_slice(),
        ),
    ] {
        files.insert(name.into(), bytes.to_vec());
    }
    let mut rules: Rules = ron::de::from_bytes(&files["rules.ron"])?;
    apply_reference(&mut rules, &reference)?;

    let palette = formats::palette(&payload.wpe)?;
    let mut assets: AssetManifest = ron::de::from_bytes(&files["assets.ron"])?;
    let scripts = member(
        &mut archive,
        "scripts\\iscript.bin",
        64 * 1024,
        "stardat",
        "verified source directional poses, construction stages and SCV work effect",
        &mut members,
    )?;
    verify_animation_source(&scripts)?;
    let flingy = member(
        &mut archive,
        "arr\\flingy.dat",
        2760,
        "stardat",
        "original fixed-point movement and acceleration",
        &mut members,
    )?;
    let movement = terran_data::decode_motion(&units, &flingy, &scripts, &[0, 7])?;
    terran_data::apply_motion(&mut rules, &movement, &UNIT_MAPPING)?;
    terran_data::apply_attack_timing(&mut rules, &scripts, &UNIT_MAPPING)?;
    terran_data::apply_acquisition(&mut rules, &units, &UNIT_MAPPING)?;
    files.insert("rules.ron".into(), ron_bytes(&rules)?);
    let image_definitions = member(
        &mut archive,
        "arr\\images.dat",
        28690,
        "stardat",
        "verified ordinary drawing mode and animation IDs for building overlays",
        &mut members,
    )?;
    verify_building_overlay_definitions(&image_definitions)?;
    verify_death_source(&scripts, &image_definitions)?;
    let fire_table = member(
        &mut archive,
        "tileset\\badlands\\ofire.pcx",
        128 * 1024,
        "stardat",
        "orange-fire remap for explicitly approximated death explosions",
        &mut members,
    )?;
    let fire_palette = fire_palette(&fire_table, &palette)?;
    let small_explosion = load_grp(
        &mut archive,
        &mut members,
        "unit\\thingy\\tBangS.grp",
        &fire_palette,
        [9, 128, 128],
    )?;
    let large_explosion = load_grp(
        &mut archive,
        &mut members,
        "unit\\thingy\\tBangX.grp",
        &fire_palette,
        [14, 252, 200],
    )?;

    // Replace the earlier east-facing preview with shared source images and directional clips.
    for old in &assets.frames {
        files.remove(&old.file);
    }
    let mut marine = decode_expected(&payload.grp, &palette, [229, 64, 64])?;
    marine.extend(load_grp(
        &mut archive,
        &mut members,
        "unit\\terran\\tmaDeath.grp",
        &palette,
        [3, 64, 64],
    )?);
    assets.frames = marine
        .iter()
        .enumerate()
        .map(|(index, image)| add_image(&mut files, &format!("marine-{index:03}.srim"), image))
        .collect::<Result<_>>()?;
    assets.unit_name = "Marine".into();
    assets.clips = vec![
        directional(ClipKind::Idle, &[68], 100),
        directional(
            ClipKind::Walk,
            &[68, 85, 102, 119, 136, 153, 170, 187, 204],
            rules.tick_ms,
        ),
        directional(
            ClipKind::Attack,
            &[34, 51, 34, 51, 34, 51, 34],
            rules.tick_ms,
        ),
        marine_death(),
    ];
    mark_marine_flashes(&mut assets.clips);

    let mut scv = load_grp(
        &mut archive,
        &mut members,
        "unit\\terran\\SCV.grp",
        &palette,
        [51, 72, 72],
    )?;
    let sparks = load_grp(
        &mut archive,
        &mut members,
        "unit\\bullet\\scvspark.grp",
        &palette,
        [10, 48, 48],
    )?;
    for image in &sparks {
        scv.push(center_canvas(image, 72, 72)?);
    }
    let scv_death_start = scv.len() as u16;
    scv = scv
        .iter()
        .map(|image| center_canvas(image, 128, 128))
        .collect::<Result<_>>()?;
    scv.extend(small_explosion);
    assets.extra_units.push(write_sprite(
        &mut files,
        2,
        "SCV",
        "scv",
        &scv,
        vec![
            directional(ClipKind::Idle, &[0], 100),
            // Source Walking holds body pose 0. Its destination-dependent glow is omitted.
            directional(ClipKind::Walk, &[0], rules.tick_ms),
            directional(ClipKind::Attack, &[17, 34, 17], rules.tick_ms),
            // waitrand 8..10 is represented by its middle value, not cosmetic randomness.
            directional(
                ClipKind::Work,
                &[34, 17, 17, 17, 17, 17, 17, 17, 17, 17],
                100,
            ),
            work_effect_clip(&work_offsets),
            single_direction(
                ClipKind::Death,
                &(scv_death_start..scv_death_start + 9).collect::<Vec<_>>(),
                150,
            ),
        ],
    )?);

    let large = load_grp(
        &mut archive,
        &mut members,
        "unit\\terran\\TBldLrg.grp",
        &palette,
        [3, 160, 128],
    )?;
    let small = load_grp(
        &mut archive,
        &mut members,
        "unit\\terran\\TBldSml.grp",
        &palette,
        [3, 96, 96],
    )?;
    for (native_id, name, path, dimensions, slug, construction) in [
        (
            3,
            "Command Center",
            "unit\\terran\\control.grp",
            [6, 128, 160],
            "command-center",
            &large,
        ),
        (
            4,
            "Supply Depot",
            "unit\\terran\\Depot.grp",
            [2, 96, 128],
            "supply-depot",
            &small,
        ),
        (
            5,
            "Barracks",
            "unit\\terran\\TBarrack.grp",
            [9, 192, 160],
            "barracks",
            &large,
        ),
    ] {
        let decoded = load_grp(&mut archive, &mut members, path, &palette, dimensions)?;
        let width = 252;
        let height = 200;
        let mut frames = std::iter::once(&decoded[0])
            .chain(construction.iter())
            .chain(std::iter::once(&decoded[1]))
            .map(|image| center_canvas(image, width, height))
            .collect::<Result<Vec<_>>>()?;
        let mut clips = vec![
            single_direction(ClipKind::Idle, &[0], 100),
            single_direction(ClipKind::Construction, &[1, 2, 3, 4], 100),
        ];
        match native_id {
            3 => {
                let overlay = load_grp(
                    &mut archive,
                    &mut members,
                    "unit\\terran\\controlT.grp",
                    &palette,
                    [1, 128, 160],
                )?;
                frames.push(center_canvas(
                    &composite(&decoded[0], &overlay[0])?,
                    width,
                    height,
                )?);
                let poses: Vec<_> = CONTROL_WORK_WAITS
                    .iter()
                    .flat_map(|wait| [(5, *wait), (0, 1)])
                    .collect();
                clips.push(single_direction(
                    ClipKind::Production,
                    &wait_steps(&poses),
                    50,
                ));
            }
            4 => {
                let overlay = load_grp(
                    &mut archive,
                    &mut members,
                    "unit\\terran\\DepotT.grp",
                    &palette,
                    [12, 96, 128],
                )?;
                for image in &overlay[..6] {
                    frames.push(center_canvas(
                        &composite(&decoded[0], image)?,
                        width,
                        height,
                    )?);
                }
                clips[0] = single_direction(ClipKind::Idle, &[5, 6, 7, 8, 9, 10], 150);
            }
            5 => {
                frames.push(center_canvas(&decoded[7], width, height)?);
                frames.push(center_canvas(&decoded[8], width, height)?);
                let poses: Vec<_> = BARRACKS_WORK
                    .iter()
                    .map(|(frame, wait)| (frame - 2, *wait))
                    .collect();
                clips.push(single_direction(
                    ClipKind::Production,
                    &wait_steps(&poses),
                    50,
                ));
            }
            _ => unreachable!("fixed building mapping"),
        }
        let rubble = if native_id == 4 {
            load_grp(
                &mut archive,
                &mut members,
                "unit\\thingy\\RubbleS.grp",
                &palette,
                [4, 96, 96],
            )?
        } else {
            load_grp(
                &mut archive,
                &mut members,
                "unit\\thingy\\RubbleL.grp",
                &palette,
                [4, 128, 128],
            )?
        };
        let death_start = frames.len() as u16;
        frames.extend(large_explosion.iter().cloned());
        for image in &rubble {
            frames.push(center_canvas(image, width, height)?);
        }
        clips.push(building_death(death_start));
        assets.extra_units.push(write_sprite(
            &mut files, native_id, name, slug, &frames, clips,
        )?);
    }
    let minerals = member(
        &mut archive,
        "unit\\neutral\\min01.grp",
        crate::ASSET_LIMIT,
        "stardat",
        "mineral presentation, first variant/frame only",
        &mut members,
    )?;
    let decoded = decode_expected(&minerals, &palette, [4, 64, 96])?;
    let image = decoded.first().context("mineral GRP has no frames")?;
    assets.resources.push(ResourceManifest {
        selection_circle: None,
        selection_y: 0,
        kind: "minerals".into(),
        anchor: [image.width as i32 / 2, image.height as i32 / 2],
        image: add_image(&mut files, "minerals.srim", image)?,
    });
    crate::terran_ui::convert(&mut archive, &mut files, &mut assets, &mut members)?;
    crate::carried_resources::refresh(&mut archive, &mut files, &mut assets, &rules)?;
    crate::hotkeys::refresh(&mut archive, &mut files)?;
    assets.validate()?;
    files.insert("assets.ron".into(), ron_bytes(&assets)?);
    crate::terran_media::convert(
        &mut installer,
        &mut archive,
        &units,
        &scripts,
        &mut files,
        &mut members,
    )?;
    files.insert(
        "terran-reference.ron".into(),
        ron_bytes(&ReferenceReport {
            schema_version: 1,
            source_to_native_units: &UNIT_MAPPING,
            units: &reference,
            movement: &movement,
            members,
            animation_evidence: vec![
                "Marine image239/iscript78: Init holds source base68; Walking cycles nine 17-heading sets68..204; repeated ground attack alternates source bases34/51. All 32 native headings reference the 17 source headings, mirroring headings17..31.",
                "SCV image247/iscript84: Init and Walking hold body base0; the source Walking script also creates image249 tscGlow, which is omitted because its draw_function9/remapping1 depends on destination pixels. Attack uses bases34/17; AlmostBuilt work uses34 then17 with waitrand8..10 approximated by9.",
                "SCV mining weapon14 and attack weapon13 use flingy141/sprite340/image526 scvspark.grp. Script237 effect sequence uses frames0..9, on the source48x48 center-anchored canvas padded to72x72. Native WorkEffect stores32 heading offsets20pixels forward, derived from the supplied executable direction table; offsets are applied relative to the unit origin after image mirroring.",
                "Command Center106 and Barracks111 construction image325 uses TBldLrg; Depot109 uses image330 TBldSml. Scripts138/140 Init/SpecialState1/SpecialState2 use frames0/1/2. Own scripts102/96/105 AlmostBuilt use frame1, Built uses frame0. Native construction clip contains generic0/1/2 then own1; finished Idle is own0.",
                "Depot Built script105 creates ordinary-drawn image279 DepotT.grp at0,0. Its script106 repeats frames0..5 with wait2, composited over the completed base for the looping native Idle clip. Barracks script96 IsWorking uses body frames7/8 with waits4,2,2,2,4,2,2. Command Center script102 IsWorking creates ordinary-drawn image276 controlT.grp; script103 alternates show/hide for its source blink pattern. These two native Production clips are selected only during native production.",
                "Marine death script78 uses own frames221..228 then sprite236/image241 tmaDeath frames0..2 (script79). SCV death script84 creates image332 tBangS, script142 frames0..8. Grounded Command Center/Barracks death scripts102/96 and Depot105 create image334 tBangX, script144 frames0..13, then rubble sprites274/273 (images336/335), script145 frames0..3. Death script instructions, image animation IDs and drawing modes are checked during import. Native clips are finite and include the corpse/decay poses.",
                "Read-only original Windows v1.00 executable evidence: 256 signed32-bit direction vectors at file0xd9b28/VA0x4dbd28; table BLAKE3 41c7553b7306494cfd8ad562669ab5e528849337684158ca150fc26cd7f72e5f. Helper VA0x405a60 multiplies each vector by length then arithmetic-shifts8. Mining VA0x41f335..0x41f37f uses length20 before animation15; construction VA0x4152d7..0x415317 also uses20. Import checks the table digest and derives offsets for native headings0,8,..248. SCV attack LO is absent; scv.loo is a separate carried-overlay attachment and is not used for these sparks.",
                "Terran HUD uses source game/tconsole.pcx, cmdicons.grp with ticon.pcx enabled remapping, game/icons.grp mineral/gas/Terran supply frames0/2/5 and full wirefram.grp frames0/7/106/109/111 with twire.pcx healthy colors. Named native UI images preserve source pixels; no source menu or UI implementation executes.",
                "Source pose instructions are checked during import and the complete iscript member hash is recorded. Layout/opcode reference: https://github.com/poiuyqwert/PyMS/blob/master/PyMS/FileFormats/IScriptBIN.py . State semantics reference: https://github.com/OpenBW/openbw/blob/master/bwgame.h .",
            ],
            uncertainties: vec![
                "For standalone import-terran only, the authored fixture map, economy, initial forces, prerequisites and scenario are preserved. import-backwater supplies source mission content and campaign-specific rules; see backwater-reference.ron.",
                "Raw DAT build/train and cooldown frame counts become native ticks. Standalone import-terran uses 50 ms/tick; import-backwater uses the source fastest-speed 42 ms/tick. Cosmetic clip frame_ms values are separate and retain the approximations recorded below.",
                "Marine movement imports its constant move4/wait1 cycle; SCV imports flingy top speed 1280/256 pixels and acceleration 67/256 per native tick. Native normalized fixed-point navigation is not the source steering, braking or collision algorithm. Static source data and executable instructions have been verified; an original-runtime timing run has not been performed.",
                "Source cargo amount/capacity is 8 and the mineral work timer is 75 frames, verified against original executable VA0x41f37b. Gas uses 37 frames and yields 2 when depleted. Native order scheduling, collision approach, construction startup/HP progression, interruptions, cancellation and production exit behavior remain approximations.",
                "Repair remains the authored native rational9/10 HP/build-time rate and cost divisor3 with own completed mechanical-role eligibility. The divisor3 is evidenced by supplied Windows v1.00 helper VA0x414820 (VA0x4148be forms3*maxHP) and OpenBW order_Repair. Native cumulative billing/rounding differs from the source per-resource timer and fixed-point details; original game-speed timing is uncalibrated.",
                "Collision dimensions preserve left+right+1 and up+down+1, but are centered by the native Footprint type. Asymmetric source extents, particularly Depot and Barracks, are approximated; original extents remain recorded here.",
                "Marine/SCV damage is scheduled one frame after repeat-attack start; original executable VA0x4188a7..0x4188be confirms cooldown variation of -1..+2 frames. Native RNG sequencing differs from the original. The standalone base conversion has one normal hit, no splash and zero minimum range; campaign research and additional weapon behavior are supplied separately. Projectile travel, first-attack versus repeat pose transitions, facing and exact range geometry remain uncalibrated.",
                "Directional Idle/Walk/Attack/Work and source construction/production/Depot fan poses are imported without an iscript VM. Mobile Walk/Attack clips use one native tick per verified wait1 pose (50ms standalone, 42ms campaign), including repeat-attack prefix waits. Work and other cosmetic clips retain separate approximations. Building clips preserve waitN frame proportions at provisional50ms per source frame; the original opcode stores N-1. Presentation wall-time remains separately uncalibrated. Idle fidgets, first-attack pose transitions, work-effect spawning, construction HP thresholds, turn rates, shadows and player-color remapping are not reproduced. Construction uses native progress; production uses native queue progress. SCV walking body is held; its destination-dependent orange-fire engine glow is omitted. Work offset directions are quantized to32 headings rather than source256 headings.",
                "Death timing uses provisional50ms source ticks: Marine body8x150ms plus three2550ms corpse poses; SCV explosion9x150ms; buildings14x150ms plus four1500ms rubble poses. Source rubble waits would retain four long decay phases; native decay is deliberately abbreviated to6seconds. Source simultaneous body/explosion/rubble overlap is represented sequentially. Explosion drawing9/remapping1 uses source Badlands ofire.pcx to recover emitted RGB, then unpremultiplies by maximum channel intensity into translucent RGBA; this preserves emission over black and removes dark mats over terrain, while destination-dependent palette remapping remains approximated. Source reference: https://github.com/OpenBW/openbw/blob/master/ui/ui.h (draw_alpha). This base demo conversion omits flying-building and cancellation effects; campaign flight art is supplied separately by flight-reference.ron.",
                "Imported wireframe art uses the source full-health palette; original randomized per-body-section damage colors are not reproduced. Native HP text/bars remain authoritative. Console layout is adapted to the native viewport, commands retain native tooltips/hotkeys, and unsupported frontend/lobby UI is not imported.",
                "Minerals use min01 frame 0 regardless of remaining amount. Standalone import-terran uses the authored Badlands preview tile. import-backwater reconstructs the source mission terrain and overlays.",
            ],
        })?,
    );
    append_report(&mut files, payload, None, Some(GAMEPLAY))?;
    Ok(files)
}

fn verify_death_source(scripts: &[u8], images: &[u8]) -> Result<()> {
    ensure!(images.len() == 28690, "unexpected legacy images.dat layout");
    let mut marine = vec![0x1a, 0x14, 1, 0x15, 1, 0x34, 0];
    for frame in 221..229 {
        marine.extend([0, frame, 0, 5, 2]);
    }
    marine.extend([0x11, 0xec, 0, 0, 0, 5, 1, 0x16]);
    expect_animation(scripts, 78, 1, &marine)?;
    let mut corpse = Vec::new();
    for frame in 0..3 {
        corpse.extend([0, frame, 0, 5, 50]);
    }
    corpse.push(0x16);
    expect_animation(scripts, 79, 0, &corpse)?;
    expect_animation(
        scripts,
        84,
        1,
        &[0x18, 0x71, 1, 8, 0x4c, 1, 0, 0, 5, 3, 0x16],
    )?;
    for (id, sprite) in [(102, 0x12), (96, 0x12), (105, 0x11)] {
        expect_animation(
            scripts,
            id,
            1,
            &[
                0x18, 7, 0, 8, 0x4e, 1, 0, 0, 5, 3, 0x3f, 0x20, 0x79, 0x11, sprite, 1, 0, 0, 5, 1,
                0x16,
            ],
        )?;
    }
    for (id, count) in [(142, 9), (144, 14)] {
        let mut sequence = Vec::new();
        for frame in 0..count {
            sequence.extend([0, frame, 0, 5, 2]);
        }
        sequence.push(0x16);
        expect_animation(scripts, id, 0, &sequence)?;
    }
    let mut rubble = Vec::new();
    for frame in 0..4 {
        rubble.extend([0, frame, 0]);
        for _ in 0..6 {
            rubble.extend([5, 125]);
        }
    }
    rubble.push(0x16);
    expect_animation(scripts, 145, 0, &rubble)?;
    for (image, script, drawing, remap) in [
        (241, 79, 0, 0),
        (332, 142, 9, 1),
        (334, 144, 9, 1),
        (335, 145, 0, 0),
        (336, 145, 0, 0),
    ] {
        ensure!(
            images[755 * 8 + image] == drawing && images[755 * 9 + image] == remap,
            "unsupported death drawing mode"
        );
        let offset = 755 * 10 + image * 4;
        ensure!(
            u32::from_le_bytes(images[offset..offset + 4].try_into().unwrap()) == script,
            "unsupported death animation mapping"
        );
    }
    Ok(())
}

fn wait_steps(poses: &[(u16, u8)]) -> Vec<u16> {
    // The original opcode stores N-1, so waitN spans exactly N source frames.
    poses
        .iter()
        .flat_map(|(frame, wait)| std::iter::repeat_n(*frame, usize::from(*wait)))
        .collect()
}

fn work_effect_clip(offsets: &[[i16; 2]; 32]) -> SpriteClip {
    SpriteClip {
        key_steps: Vec::new(),
        kind: ClipKind::WorkEffect,
        directions: 32,
        frame_ms: 100,
        frames: (51..61)
            .flat_map(|frame| {
                offsets.iter().map(move |offset| ClipFrame {
                    frame,
                    flip_x: false, // scvspark itself is not directional; only its placement turns.
                    offset: *offset,
                })
            })
            .collect(),
        loop_start: None,
        progress_starts: vec![],
    }
}

fn source_work_offsets(executable: &[u8]) -> Result<[[i16; 2]; 32]> {
    const TABLE_OFFSET: usize = 0xd9b28;
    const TABLE_DIGEST: &str = "41c7553b7306494cfd8ad562669ab5e528849337684158ca150fc26cd7f72e5f";
    ensure!(
        executable.len() <= 2 * 1024 * 1024,
        "reference executable exceeds 2 MiB"
    );
    let table = executable
        .get(TABLE_OFFSET..TABLE_OFFSET + 256 * 8)
        .context("reference executable lacks the verified direction table")?;
    ensure!(
        blake3::hash(table).to_hex().as_str() == TABLE_DIGEST,
        "unsupported executable direction table; requires the verified Windows v1.00 layout"
    );
    Ok(std::array::from_fn(|heading| {
        std::array::from_fn(|axis| {
            let offset = heading * 8 * 8 + axis * 4;
            let component = i32::from_le_bytes(table[offset..offset + 4].try_into().unwrap());
            // The checked table contains -256..=256. Match the executable's signed SAR.
            ((component * 20) >> 8) as i16
        })
    }))
}

fn verify_timing_source(executable: &[u8]) -> Result<()> {
    // Windows v1.00 .text file offsets, checked against read-only disassembly.
    // The guards keep a different source revision from silently receiving these rules.
    for (offset, expected) in [
        (0x1e77b, &[0xc6, 0x46, 0x54, 75][..]), // MiningMinerals timer
        (0x1e19f, &[0xc6, 0x46, 0x54, 37][..]), // HarvestGas timer
        (
            0x17cb5,
            &[
                0x24, 3, 0x02, 0xc3, 0x6a, 1, 0xfe, 0xc8, 0x51, 0x88, 0x44, 0x3e, 0x54,
            ][..],
        ), // (RNG & 3) + cooldown - 1
        (0xa250, &[0xfe, 0xc9, 0x88, 0x48, 7][..]), // IScript wait stores N-1
    ] {
        ensure!(
            executable.get(offset..offset + expected.len()) == Some(expected),
            "unsupported reference executable timing at file {offset:#x}"
        );
    }
    Ok(())
}

fn verify_building_overlay_definitions(bytes: &[u8]) -> Result<()> {
    ensure!(bytes.len() == 28690, "unexpected legacy images.dat layout");
    const COUNT: usize = 755;
    for (image, script) in [(276, 103_u32), (279, 106)] {
        ensure!(
            bytes[COUNT * 8 + image] == 0 && bytes[COUNT * 9 + image] == 0,
            "building overlay {image} requires unsupported drawing/remapping"
        );
        let offset = COUNT * 10 + image * 4;
        ensure!(
            u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) == script,
            "unrecognized building overlay animation {image}"
        );
    }
    Ok(())
}

pub(super) fn composite(body: &Image, overlay: &Image) -> Result<Image> {
    ensure!(
        body.width == overlay.width && body.height == overlay.height,
        "building body and overlay must share a source canvas"
    );
    let mut result = body.clone();
    for (destination, source) in result
        .rgba
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(overlay.rgba.as_chunks::<4>().0)
    {
        if source[3] == 255 {
            destination.copy_from_slice(source);
        } else if source[3] != 0 {
            // Keep straight RGBA when a translucent effect extends beyond its
            // body; baking it onto transparent black would recreate dark mats.
            let source_alpha = u32::from(source[3]);
            let backdrop_alpha = u32::from(destination[3]) * (255 - source_alpha);
            let alpha = source_alpha * 255 + backdrop_alpha;
            for channel in 0..3 {
                destination[channel] = ((u32::from(source[channel]) * source_alpha * 255
                    + u32::from(destination[channel]) * backdrop_alpha
                    + alpha / 2)
                    / alpha) as u8;
            }
            destination[3] = ((alpha + 127) / 255) as u8;
        }
    }
    Ok(result)
}

pub(super) fn center_canvas(image: &Image, width: u32, height: u32) -> Result<Image> {
    ensure!(
        width >= image.width && height >= image.height && width <= 1024 && height <= 1024,
        "invalid centered sprite canvas"
    );
    let mut result = Image {
        width,
        height,
        rgba: vec![0; width as usize * height as usize * 4],
    };
    let x = (width - image.width) as usize / 2;
    let y = (height - image.height) as usize / 2;
    for row in 0..image.height as usize {
        let source = row * image.width as usize * 4;
        let destination = ((row + y) * width as usize + x) * 4;
        let length = image.width as usize * 4;
        result.rgba[destination..destination + length]
            .copy_from_slice(&image.rgba[source..source + length]);
    }
    Ok(result)
}

/// Read only the legacy entry table and selected animation pointers. Never execute scripts.
pub(super) fn script_animation(bytes: &[u8], id: u16, animation: usize) -> Result<&[u8]> {
    ensure!(
        bytes.len() <= 64 * 1024,
        "iscript exceeds the legacy address space"
    );
    for entry in bytes.as_chunks::<4>().0 {
        let candidate = u16::from_le_bytes(entry[..2].try_into().unwrap());
        if candidate == u16::MAX {
            break;
        }
        if candidate != id {
            continue;
        }
        let offset = usize::from(u16::from_le_bytes(entry[2..].try_into().unwrap()));
        let header = bytes
            .get(offset..offset + 8)
            .context("truncated iscript entry header")?;
        ensure!(&header[..4] == b"SCPE", "invalid iscript entry signature");
        let slots = match header[4] {
            0 | 1 => 2,
            2 => 4,
            12 | 13 => 14,
            14 | 15 => 16,
            20 | 21 => 22,
            23 => 24,
            24 => 26,
            26 | 27 => 28,
            other => anyhow::bail!("unsupported selected iscript entry type {other}"),
        };
        ensure!(
            animation < slots,
            "iscript animation absent from entry type"
        );
        let table = bytes
            .get(offset + 8..offset + 8 + slots * 2)
            .context("truncated iscript animation table")?;
        let pointer = usize::from(u16::from_le_bytes(
            table[animation * 2..animation * 2 + 2].try_into().unwrap(),
        ));
        ensure!(
            pointer != 0,
            "selected iscript animation has no instructions"
        );
        return bytes
            .get(pointer..)
            .context("iscript animation pointer outside member");
    }
    anyhow::bail!("missing iscript entry {id}")
}

pub(super) fn expect_animation(
    bytes: &[u8],
    id: u16,
    animation: usize,
    prefix: &[u8],
) -> Result<()> {
    ensure!(
        script_animation(bytes, id, animation)?.starts_with(prefix),
        "unrecognized source instructions for iscript{id} animation{animation}; native pose mappings require the verified original layout"
    );
    Ok(())
}

fn verify_animation_source(bytes: &[u8]) -> Result<()> {
    expect_animation(bytes, 78, 0, &[9, 240, 0, 0, 0, 0, 68, 0])?;
    let mut walking = Vec::new();
    for base in [85, 102, 119, 136, 153, 170, 187, 204, 68] {
        walking.extend([0x29, 4, 5, 1, 0, base, 0]);
    }
    expect_animation(bytes, 78, 11, &walking)?;
    expect_animation(
        bytes,
        78,
        5,
        &[5, 1, 0x2e, 0x18, 69, 0, 0x25, 1, 0, 51, 0, 5, 1, 0, 34, 0],
    )?;
    expect_animation(bytes, 84, 0, &[9, 248, 0, 0, 7, 0, 0, 0])?;
    expect_animation(bytes, 84, 11, &[0, 0, 0, 8, 249, 0, 0, 0])?;
    expect_animation(
        bytes,
        84,
        2,
        &[3, 0, 5, 1, 0, 34, 0, 0x25, 1, 5, 1, 0, 17, 0],
    )?;
    expect_animation(
        bytes,
        84,
        15,
        &[3, 0, 5, 1, 0, 34, 0, 0x28, 14, 5, 1, 0, 17, 0, 6, 8, 10],
    )?;
    let mut sparks = vec![0, 0, 0, 0x1a, 35, 0, 39, 0, 0x1b, 5, 1];
    for frame in 1..10 {
        sparks.extend([0, frame, 0, 5, 1]);
    }
    sparks.push(0x16);
    expect_animation(bytes, 237, 1, &sparks)?;
    for id in [138, 140] {
        for (animation, frame) in [(0, 0), (13, 1), (14, 2)] {
            expect_animation(bytes, id, animation, &[0, frame, 0])?;
        }
    }
    for id in [96, 102, 105] {
        expect_animation(bytes, id, 15, &[0, 1, 0])?;
    }
    expect_animation(bytes, 105, 16, &[8, 0x17, 1, 0, 0, 0, 0, 0])?;
    let mut fan = Vec::new();
    for frame in 0..6 {
        fan.extend([0, frame, 0, 5, 2]);
    }
    fan.extend([7, 0x0c, 0x61]);
    expect_animation(bytes, 106, 0, &fan)?;
    let mut barracks = Vec::new();
    for (frame, wait) in BARRACKS_WORK {
        barracks.extend([0, frame as u8, 0, 5, wait]);
    }
    barracks.extend([7, 0x09, 0x5d]);
    expect_animation(bytes, 96, 19, &barracks)?;
    expect_animation(bytes, 102, 19, &[8, 0x14, 1, 0, 0])?;
    let mut control = vec![0, 0, 0];
    for wait in CONTROL_WORK_WAITS {
        control.extend([0x33, 5, wait, 0x32, 5, 1]);
    }
    control.extend([7, 0x22, 0x60]);
    expect_animation(bytes, 103, 0, &control)?;
    Ok(())
}

fn apply_reference(rules: &mut Rules, reference: &[ReferenceUnit]) -> Result<()> {
    rules.prioritize_threats = true;
    ensure!(
        reference.len() == UNIT_MAPPING.len(),
        "expected five reference units"
    );
    for &(source_id, native_id) in &UNIT_MAPPING {
        let source = reference
            .iter()
            .find(|unit| unit.source_id == source_id)
            .with_context(|| format!("missing reference unit {source_id}"))?;
        let unit = rules
            .units
            .iter_mut()
            .find(|unit| unit.id == UnitTypeId(native_id))
            .with_context(|| format!("Terran template lacks native unit {native_id}"))?;
        ensure!(
            source.supply_required_half_units.is_multiple_of(2)
                && source.supply_provided_half_units.is_multiple_of(2),
            "unit {source_id} uses half supply; native demonstration requires whole supply"
        );
        unit.max_hp = source.hitpoints;
        unit.armor = u32::from(source.armor);
        unit.cost = [("minerals", source.minerals), ("gas", source.gas)]
            .into_iter()
            .filter(|(_, amount)| *amount != 0)
            .map(|(kind, amount)| ResourceAmount {
                kind: kind.into(),
                amount: u32::from(amount),
            })
            .collect();
        unit.build_ticks = u32::from(source.build_frames);
        unit.supply_used = u32::from(source.supply_required_half_units / 2);
        unit.supply_provided = u32::from(source.supply_provided_half_units / 2);
        let [left, up, right, down] = source.collision_extents;
        unit.footprint = Footprint {
            width: left + right + 1,
            height: up + down + 1,
        };
        unit.placement = Footprint {
            width: source.placement_size[0],
            height: source.placement_size[1],
        };
        unit.weapon = source
            .weapon
            .as_ref()
            .map(|weapon| {
                ensure!(
                    weapon.minimum_range == 0,
                    "weapon {} minimum range is unsupported",
                    weapon.id
                );
                Ok(Weapon {
                    cooldown_jitter: None,
                    targets_air: source_id == 0,
                    damage: u32::from(weapon.damage),
                    range: weapon.maximum_range,
                    cooldown: u32::from(weapon.cooldown_frames),
                    damage_kind: straterust_engine::sim::DamageKind::Normal,
                    splash: None,
                    strikes: Vec::new(),
                })
            })
            .transpose()?;
        if source_id == 7 {
            let worker = unit.worker.as_mut().context("SCV worker role missing")?;
            worker.capacity = 8;
            worker.harvest_amount = 8;
            worker.harvest_ticks = 75;
            worker.idle_resource_radius = 256;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

mod graphics;
pub(crate) use graphics::*;
