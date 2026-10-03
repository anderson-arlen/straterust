//! Translate source worker cargo into generic resource-kind presentation.
use std::io::Cursor;

use anyhow::{Context, Result, ensure};
use straterust_engine::{
    assets::{AssetManifest, CarriedResourceManifest, ClipKind, ImageRef, SpriteManifest},
    sim::{Rules, UnitTypeId},
};

use crate::{Archive, Files, add_image, formats, ron_bytes, terran, terran_media};

pub(crate) fn refresh(
    archive: &mut Archive<Cursor<Vec<u8>>>,
    files: &mut Files,
    assets: &mut AssetManifest,
    rules: &Rules,
) -> Result<()> {
    let Some(carrier) = assets
        .extra_units
        .iter()
        .find(|sprite| sprite.unit_type == UnitTypeId(2))
    else {
        return Ok(());
    };
    let carrier = carrier.clone();
    let capacity = rules
        .units
        .iter()
        .find(|unit| unit.id == carrier.unit_type)
        .and_then(|unit| unit.worker.as_ref())
        .context("cargo carrier is not a worker")?
        .capacity;
    let definitions = archive.read_file("arr\\images.dat", 28690)?;
    let names = archive.read_file("arr\\images.tbl", 65536)?;
    let scripts = archive.read_file("scripts\\iscript.bin", 65536)?;
    let palette = formats::palette(&archive.read_file("tileset\\badlands.wpe", 1024)?)?;
    ensure!(
        definitions.len() == 28690,
        "unsupported carried resource image layout"
    );
    let word = |base, image: usize| {
        u32::from_le_bytes(
            definitions[base + image * 4..base + image * 4 + 4]
                .try_into()
                .unwrap(),
        )
    };
    let name = |index| -> Result<String> {
        Ok(format!(
            "unit\\{}",
            terran_media::table_string(&names, index)?
        ))
    };
    ensure!(
        name(word(0, 247))? == "unit\\terran\\SCV.grp",
        "unexpected carrier body"
    );
    let lo_name = name(word(755 * 26, 247))?;
    let lo = archive.read_file(&lo_name, 1024 * 1024)?;
    let attachments = read_attachments(&lo)?;
    assets
        .carried_resources
        .retain(|mapping| mapping.full.unit_type != carrier.unit_type);
    for (kind, full_image, partial_image) in [("minerals", 397, 398), ("gas", 403, 404)] {
        let path = name(word(0, full_image))?;
        ensure!(
            word(0, full_image) == word(0, partial_image),
            "cargo variants must share their source GRP"
        );
        let images = formats::decode_grp(&archive.read_file(&path, 8 * 1024 * 1024)?, &palette)?;
        let first = images.first().context("missing resource cargo art")?;
        let anchor = [first.width as i32 / 2, first.height as i32 / 2];
        let references = images
            .iter()
            .enumerate()
            .map(|(index, image)| {
                add_image(files, &format!("carried-{kind}-{index:02}.srim"), image)
            })
            .collect::<Result<Vec<_>>>()?;
        let variant = |image| -> Result<SpriteManifest> {
            let script = u16::try_from(word(755 * 10, image))?;
            let init = terran::script_animation(&scripts, script, 0)?;
            ensure!(
                init.len() >= 3 && init.first() == Some(&0),
                "cargo must start with a source pose"
            );
            let base = u16::from_le_bytes(init[1..3].try_into().unwrap());
            cargo_sprite(
                &carrier,
                kind,
                references.clone(),
                anchor,
                &attachments,
                base,
                definitions[755 * 4 + image] != 0,
            )
        };
        assets.carried_resources.push(CarriedResourceManifest {
            kind: kind.into(),
            full_amount: capacity,
            full: variant(full_image)?,
            partial: Some(variant(partial_image)?),
        });
    }
    files.insert("carried-resources-reference.ron".into(), ron_bytes(&(
        "Retail SCV image247 uses special attachment terran/scv.loo:51 poses, one signed-byte offset each. Source carried mineral images397/398 use OreChunk.grp/script221/222 pose0 with directional frames; Terran gas images403/404 use GasTank.grp/script227/228 poses0/1 without directionality. Full loads carry8; partial/depleted gas uses the second tank. Native overlay clips follow the body's existing action/heading timing and transformed attachment coordinates. Resource ownership, amount, harvesting and deposits remain engine state."
    ))?);
    Ok(())
}

fn read_attachments(bytes: &[u8]) -> Result<Vec<[i16; 2]>> {
    ensure!(bytes.len() >= 8, "truncated cargo LO header");
    let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
    let frames = word(0);
    ensure!(
        frames == 51 && word(4) == 1 && bytes.len() >= 8 + frames * 4,
        "unexpected SCV cargo attachment layout"
    );
    (0..frames)
        .map(|frame| {
            let offset = word(8 + frame * 4);
            let point = bytes
                .get(offset..offset.saturating_add(2))
                .context("truncated cargo LO point")?;
            ensure!(point != [127, 127], "missing SCV cargo attachment");
            Ok([i16::from(point[0] as i8), i16::from(point[1] as i8)])
        })
        .collect()
}

fn cargo_sprite(
    carrier: &SpriteManifest,
    kind: &str,
    frames: Vec<ImageRef>,
    anchor: [i32; 2],
    attachments: &[[i16; 2]],
    base: u16,
    directional: bool,
) -> Result<SpriteManifest> {
    let mut clips = carrier
        .clips
        .iter()
        .filter(|clip| {
            matches!(
                clip.kind,
                ClipKind::Idle | ClipKind::Walk | ClipKind::Attack | ClipKind::Work
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    for frame in clips.iter_mut().flat_map(|clip| &mut clip.frames) {
        let reference = &carrier.frames[usize::from(frame.frame)];
        let pose = reference
            .file
            .strip_prefix("scv-")
            .and_then(|file| file.strip_suffix(".srim"))
            .context("cargo clip does not reference a source SCV body")?
            .parse::<usize>()?;
        let offset = *attachments
            .get(pose)
            .context("missing cargo attachment pose")?;
        // The source 72px body is centered in the native 128px canvas. Resolve
        // its origin once; mirror the LO point, not the already resolved offset.
        let center_x = if frame.flip_x {
            carrier.anchor[0] - 64
        } else {
            64 - carrier.anchor[0]
        };
        frame.offset = [
            frame.offset[0]
                + i16::try_from(center_x)?
                + if frame.flip_x { -offset[0] } else { offset[0] },
            frame.offset[1] + i16::try_from(64 - carrier.anchor[1])? + offset[1],
        ];
        frame.frame = base + if directional { (pose % 17) as u16 } else { 0 };
        ensure!(
            usize::from(frame.frame) < frames.len(),
            "cargo pose outside its art"
        );
        frame.flip_x &= directional;
    }
    Ok(SpriteManifest {
        unit_type: carrier.unit_type,
        unit_name: format!("{} carrying {kind}", carrier.unit_name),
        frame_ms: carrier.frame_ms,
        anchor,
        frames,
        clips,
    })
}
