//! Damage atlases preserve the four source wireframe sections without adding
//! hundreds of individual UI entries. Each atlas contains twenty square rows:
//! the executable's two ten-level Terran health tables.
use super::*;

// Windows retail v1.00 data at 0x4e9928/0x4e9950. The draw callback
// 0x49efe0 selects floor(ceil(HP) * 9 / maxHP), then maps these table indices
// through game/twire.pcx into the four indexed body sections (208..211).
const HEALTH: [[[u8; 4]; 10]; 2] = [
    [
        [10, 10, 10, 10],
        [10, 10, 10, 0],
        [10, 10, 0, 0],
        [10, 10, 0, 1],
        [10, 10, 1, 1],
        [10, 0, 1, 1],
        [10, 1, 1, 1],
        [0, 1, 1, 1],
        [0, 1, 1, 1],
        [1, 1, 1, 1],
    ],
    [
        [10, 10, 10, 10],
        [10, 10, 10, 0],
        [10, 10, 0, 0],
        [10, 0, 0, 0],
        [10, 0, 0, 1],
        [0, 0, 0, 1],
        [0, 0, 1, 1],
        [0, 1, 1, 1],
        [0, 1, 1, 1],
        [1, 1, 1, 1],
    ],
];

pub(crate) fn refresh_wireframes(
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    files: &mut Files,
    assets: &mut AssetManifest,
    rules: &Rules,
) -> Result<()> {
    let colors = formats::decode_pcx(&archive.read_file("game\\twire.pcx", 1024 * 1024)?)?;
    let healthy = crate::terran_ui::wireframe_palette(&colors)?;
    for (kind, path) in [
        ("wireframe", "unit\\wirefram\\wirefram.grp"),
        ("groupwire", "unit\\wirefram\\grpwire.grp"),
    ] {
        let bytes = archive.read_file(path, 1024 * 1024)?;
        // Decode indexed region identities once. Alpha and all non-health pixels
        // are copied unchanged; only section colors change between atlas rows.
        let mut markers = healthy;
        for (section, color) in markers[208..212].iter_mut().enumerate() {
            *color = [section as u8, 254, 253, 255];
        }
        let frames = formats::decode_grp(&bytes, &markers)?;
        for source in 0..228 {
            let Some(id) = native_id(source) else {
                continue;
            };
            if !rules.units.iter().any(|unit| unit.id == id)
                || !assets
                    .ui
                    .iter()
                    .any(|entry| entry.key == format!("{kind}.{}", id.0))
            {
                continue;
            }
            let Some(frame) = frames.get(usize::from(source)) else {
                continue;
            };
            let mut atlas = Image {
                width: frame.width,
                height: frame.height * 20,
                rgba: Vec::with_capacity(frame.rgba.len() * 20),
            };
            for table in HEALTH {
                for level in table {
                    for pixel in frame.rgba.as_chunks::<4>().0 {
                        if pixel[1..] == [254, 253, 255] && pixel[0] < 4 {
                            // Stable source-role permutation substitutes for the
                            // original per-instance randomizer; it affects only art.
                            let section = (usize::from(pixel[0]) + usize::from(source) % 4) % 4;
                            let color = colors.pixels[usize::from(level[section])];
                            atlas
                                .rgba
                                .extend_from_slice(&colors.palette[usize::from(color)]);
                        } else {
                            atlas.rgba.extend_from_slice(pixel);
                        }
                    }
                }
            }
            let key = format!("{kind}.damage.{}", id.0);
            assets.ui.retain(|entry| entry.key != key);
            assets.ui.push(straterust_engine::assets::UiImageManifest {
                image: crate::add_image(files, &format!("ui-{key}.srim"), &atlas)?,
                key,
            });
        }
    }
    Ok(())
}
