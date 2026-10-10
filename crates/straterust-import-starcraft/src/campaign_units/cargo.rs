//! Source worker attachment coordinates translated alongside the body poses.
use super::*;
use straterust_engine::assets::CarriedResourceManifest;

#[allow(clippy::too_many_arguments)]
pub(super) fn convert(
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    files: &mut Files,
    assets: &mut AssetManifest,
    source: u16,
    image: usize,
    definitions: &[u8],
    names: &[u8],
    scripts: &[u8],
    palette: &[[u8; 4]; 256],
    poses: &BTreeMap<u16, u16>,
) -> Result<()> {
    let id = native_id(source).unwrap();
    let body = assets
        .extra_units
        .iter()
        .find(|sprite| sprite.unit_type == id)
        .unwrap()
        .clone();
    let path = format!(
        "unit\\{}",
        terran_media::table_string(names, dword(definitions, 755 * 26 + image * 4))?
    );
    let lo = archive.read_file(&path, 1024 * 1024)?;
    let count = dword(&lo, 0) as usize;
    ensure!(
        count > 0 && count <= 4096 && dword(&lo, 4) == 1,
        "invalid worker cargo attachments"
    );
    let original_pose = |frame: u16| -> Result<usize> {
        poses
            .iter()
            .find_map(|(&pose, &base)| {
                (frame >= base && frame < base + 17).then_some(usize::from(pose + frame - base))
            })
            .context("missing source worker pose")
    };
    assets
        .carried_resources
        .retain(|mapping| mapping.full.unit_type != id);
    for (kind, full, partial) in [
        ("minerals", 397, 398),
        (
            "gas",
            if source == 41 { 401 } else { 399 },
            if source == 41 { 402 } else { 400 },
        ),
    ] {
        let path = format!(
            "unit\\{}",
            terran_media::table_string(names, dword(definitions, full * 4))?
        );
        let decoded = formats::decode_grp(&archive.read_file(&path, 8 * 1024 * 1024)?, palette)?;
        let anchor = [decoded[0].width as i32 / 2, decoded[0].height as i32 / 2];
        let frames = decoded
            .iter()
            .enumerate()
            .map(|(n, frame)| {
                crate::add_image(
                    files,
                    &format!("carried-{source}-{kind}-{n:02}.srim"),
                    frame,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let variant = |image| -> Result<_> {
            let base =
                iscript::timeline(scripts, dword(definitions, 755 * 10 + image * 4) as u16, 0)
                    .first()
                    .copied()
                    .unwrap_or(0);
            let directional = definitions[755 * 4 + image] != 0;
            let mut clips = body
                .clips
                .iter()
                .filter(|clip| {
                    matches!(
                        clip.kind,
                        ClipKind::Idle | ClipKind::Walk | ClipKind::Work | ClipKind::Attack
                    )
                })
                .cloned()
                .collect::<Vec<_>>();
            for frame in clips.iter_mut().flat_map(|clip| &mut clip.frames) {
                let pose = original_pose(frame.frame)?;
                ensure!(pose < count, "worker cargo pose outside attachment table");
                let offset = dword(&lo, 8 + pose * 4) as usize;
                let point = lo
                    .get(offset..offset + 2)
                    .context("truncated cargo attachment")?;
                frame.offset = [
                    i16::from(point[0] as i8) * if frame.flip_x { -1 } else { 1 },
                    i16::from(point[1] as i8),
                ];
                frame.frame = base + if directional { (pose % 17) as u16 } else { 0 };
                ensure!(
                    usize::from(frame.frame) < frames.len(),
                    "invalid carried resource pose"
                );
                frame.flip_x &= directional;
            }
            Ok(straterust_engine::assets::SpriteManifest {
                unit_type: id,
                unit_name: format!("{} carrying {kind}", body.unit_name),
                frame_ms: body.frame_ms,
                anchor,
                frames: frames.clone(),
                clips,
            })
        };
        assets.carried_resources.push(CarriedResourceManifest {
            replaces_body: false,
            kind: kind.into(),
            full_amount: 8,
            full: variant(full)?,
            partial: Some(variant(partial)?),
        });
    }
    Ok(())
}
