//! Package-provided ground coverage art; provider rules stay in the simulation.
use super::*;

impl<'a> View<'a> {
    pub(super) fn draw_coverage(&self, canvas: &mut Canvas<'_, 'a>, size: [f64; 2]) {
        let Some(assets) = self.assets else {
            return;
        };
        let placing = self
            .placement_type
            .or_else(|| self.placement.map(|p| p.0))
            .is_some_and(|kind| {
                self.world
                    .unit_type(kind)
                    .is_some_and(|u| u.requires_power || u.power_field.is_some())
                    || self.world.state().entities.iter().any(|e| {
                        e.owner == self.world.view_player()
                            && self.selected.contains(&e.id)
                            && self.world.unit_type(e.unit_type).is_some_and(|u| {
                                u.builds.contains(&kind)
                                    && u.builds.iter().any(|id| {
                                        self.world.unit_type(*id).is_some_and(|u| u.requires_power)
                                    })
                            })
                    })
            });
        for provider in self.world.state().entities.iter().filter(|e| {
            e.owner == self.world.view_player()
                && e.construction.is_none()
                && !e.airborne
                && self.world.transformation_progress(e).is_none()
                && (placing || self.selected.contains(&e.id))
                && self
                    .world
                    .unit_type(e.unit_type)
                    .is_some_and(|u| u.power_field.is_some())
        }) {
            let Some(frame) = visual::coverage_image(assets, provider) else {
                continue;
            };
            let position = self.camera.world_to_screen(
                f64::from(provider.position.x),
                f64::from(provider.position.y),
                size,
            );
            canvas.image(
                frame.image,
                [
                    position[0] - f64::from(frame.anchor[0]) * self.camera.zoom,
                    position[1] - f64::from(frame.anchor[1]) * self.camera.zoom,
                ],
                [frame.image.width, frame.image.height],
                self.camera.zoom,
            );
        }
    }
}
