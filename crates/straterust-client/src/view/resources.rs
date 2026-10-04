//! Resource inspection shares world picking and the existing selection console.
use super::*;
use straterust_engine::sim::ResourceNode;

impl<'a> View<'a> {
    pub(super) fn draw_resource_circle(
        &self,
        canvas: &mut Canvas<'_, 'a>,
        resource: &ResourceNode,
        p: [f64; 2],
    ) {
        let source = self.assets.and_then(|assets| {
            let pack = assets.indicators.as_ref()?;
            let art = assets
                .resources
                .iter()
                .find(|art| art.manifest.kind == resource.kind)?;
            let circle = art.manifest.selection_circle?;
            Some((
                pack.circles.get(usize::from(circle))?,
                art.manifest.selection_y,
            ))
        });
        if let Some((image, offset)) = source {
            let height = image.height / 3;
            canvas.image_region(
                image,
                [
                    p[0] - f64::from(image.width) * self.camera.zoom / 2.0,
                    p[1] + (f64::from(offset) - f64::from(height) / 2.0) * self.camera.zoom,
                ],
                [image.width, height],
                self.camera.zoom,
                [0, height, image.width, height],
            );
        } else {
            canvas.selection_circle(
                p,
                [
                    f64::from(resource.footprint.width.max(24)) * self.camera.zoom / 2.0 + 4.0,
                    f64::from(resource.footprint.height.max(12)) * self.camera.zoom / 2.0 + 2.0,
                ],
                0xffff00,
            );
        }
    }

    pub(super) fn draw_resource_details(
        &self,
        canvas: &mut Canvas<'_, 'a>,
        rect: [f64; 4],
        scale: f64,
    ) -> bool {
        let Some(resource) = self.world.state().resources.iter().find(|resource| {
            self.selected_resource == Some(resource.id)
                && (resource.amount > 0 || resource.requires_extractor)
                && self
                    .world
                    .visibility(self.world.view_player(), resource.position)
                    != Visibility::Unexplored
        }) else {
            return false;
        };
        let name = match resource.kind.as_str() {
            "minerals" => "MINERAL FIELD",
            "gas" => "VESPENE GEYSER",
            _ => resource.kind.as_str(),
        };
        let [x, y, width, _] = rect;
        canvas.text(
            &shorten(name, (width / 8.0) as usize),
            x + 8.0,
            y + 5.0,
            1.0,
            0xd3d8bf,
        );
        let mut detail_x = x + 8.0;
        if let Some(art) = self.assets.and_then(|assets| {
            assets
                .resources
                .iter()
                .find(|art| art.manifest.kind == resource.kind)
        }) {
            let zoom = (64.0 * scale / f64::from(art.image.width))
                .min(56.0 * scale / f64::from(art.image.height));
            canvas.image(
                &art.image,
                [x + 4.0 * scale, y + 23.0 * scale],
                [art.image.width, art.image.height],
                zoom,
            );
            detail_x = x + 76.0 * scale;
        }
        canvas.text("REMAINING", detail_x, y + 28.0 * scale, 1.0, 0x9aab98);
        canvas.text(
            &resource.amount.to_string(),
            detail_x,
            y + 46.0 * scale,
            1.0,
            0xffff00,
        );
        if resource.amount == 0 && resource.requires_extractor {
            canvas.text("DEPLETED", detail_x, y + 64.0 * scale, 1.0, 0x9aab98);
        }
        true
    }
}
