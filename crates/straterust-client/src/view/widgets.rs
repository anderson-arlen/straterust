use super::*;

pub(super) fn draw_unit_icon<'a>(
    canvas: &mut Canvas<'_, 'a>,
    assets: Option<&'a AssetPack>,
    id: UnitTypeId,
    rect: [f64; 4],
    color: u32,
) {
    if let Some(image) = assets.and_then(|assets| assets.ui_image(&format!("unit.{}", id.0))) {
        let scale = (rect[2] / f64::from(image.width)).min(rect[3] / f64::from(image.height));
        let width = f64::from(image.width) * scale;
        let height = f64::from(image.height) * scale;
        canvas.image_stretched(
            image,
            [
                rect[0] + (rect[2] - width) / 2.0,
                rect[1] + (rect[3] - height) / 2.0,
                width,
                height,
            ],
            0xffffff,
        );
    } else if let Some(sprite) = assets.and_then(|assets| assets.sprite(id)) {
        let (frame_index, flip_x) = sprite
            .clip(straterust_engine::assets::ClipKind::Idle)
            .map_or((0, false), |clip| {
                let direction = if clip.directions == 32 { 8 } else { 0 };
                let reference = &clip.frames[direction];
                (usize::from(reference.frame), reference.flip_x)
            });
        let frame = &sprite.frames[frame_index];
        let zoom = (rect[2] / f64::from(frame.width)).min(rect[3] / f64::from(frame.height));
        canvas.image_mirrored(
            frame,
            [
                rect[0] + (rect[2] - f64::from(frame.width) * zoom) / 2.0,
                rect[1] + (rect[3] - f64::from(frame.height) * zoom) / 2.0,
            ],
            [frame.width, frame.height],
            zoom,
            flip_x,
        );
    } else {
        let r = rect[2].min(rect[3]) * 0.55;
        canvas.outline(
            rect[0] + (rect[2] - r) / 2.0,
            rect[1] + (rect[3] - r) / 2.0,
            r,
            r,
            color,
        );
        canvas.rect(
            rect[0] + rect[2] / 2.0 - 3.0,
            rect[1] + rect[3] / 2.0 - 3.0,
            6.0,
            6.0,
            color,
        );
    }
}

pub(super) fn command_icon(assets: &AssetPack, action: Action) -> Option<&Image> {
    let key = match action {
        Action::AdvancedBuildMenu => "command.advanced-build",
        Action::Build(id) | Action::Train(id) => return assets.ui_image(&format!("unit.{}", id.0)),
        Action::Research(id) => return assets.ui_image(&format!("research.{}", id.0)),
        Action::Cloak(true) => "command.cloak",
        Action::Cloak(false) => "command.decloak",
        Action::Stim => "command.stim",
        Action::Scan => "command.scan",
        Action::Unload => "command.unload",
        Action::Lift => "command.lift",
        Action::Land => "command.land",
        Action::PlaceMine => "command.mine",
        Action::Move => "command.move",
        Action::Stop => "command.stop",
        Action::Hold => "command.hold",
        Action::AttackMove => "command.attack",
        Action::Patrol => "command.patrol",
        Action::Gather => "command.gather",
        Action::Repair => "command.repair",
        Action::BuildMenu => "command.build",
        Action::Rally => "command.rally",
        Action::Back => "command.back",
        Action::Cancel => "command.cancel",
    };
    assets.ui_image(key)
}

pub(super) fn progress_bar(canvas: &mut Canvas<'_, '_>, rect: [f64; 4], progress: f64, color: u32) {
    canvas.rect(rect[0], rect[1], rect[2], rect[3], 0x0d1411);
    canvas.rect(
        rect[0],
        rect[1],
        rect[2] * progress.clamp(0.0, 1.0),
        rect[3],
        color,
    );
}

pub(super) fn shorten(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.into()
    } else {
        text.chars()
            .take(limit.saturating_sub(3))
            .chain("...".chars())
            .collect()
    }
}

pub(super) fn wrapped_lines(text: &str, limit: usize) -> Vec<String> {
    let limit = limit.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + word.chars().count() + 1 > limit {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
        while line.chars().count() > limit {
            lines.push(line.chars().take(limit).collect());
            line = line.chars().skip(limit).collect();
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}
