use super::*;
use crate::path::{CollisionQuery, NavigationData, Pathfinder};

#[derive(Clone, Copy)]
pub(in crate::sim) struct CollisionPolicy {
    except: EntityId,
    arrival: Option<(Position, Footprint)>,
    phasing: bool,
    static_only: bool,
    reservations: bool,
    reservation_origin: Option<Bounds>,
}

pub(in crate::sim) struct CollisionView<'a> {
    world: &'a World,
    policy: CollisionPolicy,
}

impl World {
    pub(in crate::sim) fn movement_obstacles(
        &self,
        except: EntityId,
        arrival: Option<(Position, Footprint)>,
    ) -> CollisionView<'_> {
        let actor = self
            .index(except)
            .map(|at| (&self.state.entities[at], self.unit_at(at)));
        let phasing = actor.is_some_and(|(a, unit)| {
            unit.phases_while_gathering
                && (matches!(a.order, UnitOrder::Gather { .. })
                    || self
                        .collision_candidates(unit.footprint.bounds(a.position))
                        .into_iter()
                        .any(|id| {
                            let ColliderId::Entity(id) = id else {
                                return false;
                            };
                            let Some(at) = self.index(id) else {
                                return false;
                            };
                            let e = &self.state.entities[at];
                            let u = self.unit_at(at);
                            id != except
                                && !self.phases_collision(e)
                                && !self.inside_structure(e)
                                && e.garrisoned_in.is_none()
                                && e.doodad_enabled != Some(false)
                                && self.movement_class(a) == self.movement_class(e)
                                && u.speed > 0
                                && u.blocks_movement
                                && intersects(
                                    unit.footprint.bounds(a.position),
                                    u.footprint.bounds(e.position),
                                )
                        }))
        });
        let reservations = actor.is_some_and(|(a, _)| matches!(a.order, UnitOrder::Gather { .. }));
        let reservation_origin = actor
            .filter(|(a, _)| a.harvest_spot.is_none())
            .map(|(a, u)| u.footprint.bounds(a.position));
        self.collision_view(CollisionPolicy {
            except,
            arrival,
            phasing,
            static_only: false,
            reservations,
            reservation_origin,
        })
    }
    pub(in crate::sim) fn collision_view(&self, policy: CollisionPolicy) -> CollisionView<'_> {
        CollisionView {
            world: self,
            policy,
        }
    }
    pub(in crate::sim) fn static_obstacles(&self) -> CollisionView<'_> {
        let mut view = self.movement_obstacles(EntityId(0), None);
        view.policy.static_only = true;
        view
    }
    pub(in crate::sim) fn resource_obstacles(&self, except: EntityId) -> CollisionView<'_> {
        let mut view = self.movement_obstacles(except, None);
        view.policy.static_only = true;
        view.policy.reservations = true;
        view.policy.reservation_origin = None;
        view
    }
}

impl CollisionQuery for CollisionView<'_> {
    fn blocked(&self, bounds: Bounds, class: MovementClass) -> bool {
        let w = self.world;
        self.world
            .collision_candidates(bounds)
            .into_iter()
            .any(|id| {
                let (position, footprint, layer) = match id {
                    ColliderId::Entity(id) => {
                        let Some(at) = w.index(id) else {
                            return false;
                        };
                        let e = &w.state.entities[at];
                        let u = w.unit_at(at);
                        if id == self.policy.except
                            || self.policy.static_only && (u.speed != 0 || e.airborne)
                            || (self.policy.phasing && u.speed != 0)
                            || w.phases_collision(e)
                            || e.doodad_enabled == Some(false)
                            || !u.blocks_movement
                            || w.inside_structure(e)
                            || e.garrisoned_in.is_some()
                            || u.revealer
                            || self.policy.arrival.is_some_and(|(target, footprint)| {
                                (u.speed != 0 || e.airborne)
                                    && !rts::in_range(
                                        e.position,
                                        u.footprint,
                                        target,
                                        footprint,
                                        u32::from(footprint.width.max(footprint.height)),
                                    )
                            })
                        {
                            return false;
                        }
                        (e.position, u.footprint, w.movement_class(e))
                    }
                    ColliderId::Resource(id) => {
                        let Some(node) = w.collision_resource(id) else {
                            return false;
                        };
                        if !w.resource_blocks_movement(node) {
                            return false;
                        }
                        (node.position, node.footprint, MovementClass::Ground)
                    }
                    ColliderId::Reservation(id) => {
                        let Some(at) = w.index(id) else {
                            return false;
                        };
                        let e = &w.state.entities[at];
                        let Some(spot) = e.harvest_spot else {
                            return false;
                        };
                        let footprint = w.unit_at(at).footprint;
                        if id == self.policy.except
                            || !self.policy.reservations
                            || self
                                .policy
                                .reservation_origin
                                .is_some_and(|origin| intersects(origin, footprint.bounds(spot)))
                        {
                            return false;
                        }
                        (spot, footprint, w.movement_class(e))
                    }
                };
                layer.collides(class) && intersects(bounds, footprint.bounds(position))
            })
    }
}

impl CollisionView<'_> {
    pub fn policy(&self) -> CollisionPolicy {
        self.policy
    }
    pub fn clear(
        &self,
        footprint: Footprint,
        class: MovementClass,
        from: Position,
        to: Position,
    ) -> bool {
        crate::path::segment_clear_indexed(&self.world.map, footprint, class, from, to, self)
    }
    pub fn finder<'a>(&'a self, data: &'a NavigationData) -> Pathfinder<'a> {
        Pathfinder::indexed(&self.world.map, self, data)
    }
    pub fn find_path(
        &self,
        footprint: Footprint,
        class: MovementClass,
        start: Position,
        target: Position,
    ) -> Option<Vec<Position>> {
        let data = self.world.navigation_data.with_unchanged_terrain();
        self.finder(&data)
            .find_path(footprint, class, start, target)
    }
    pub fn find_path_near(
        &self,
        footprint: Footprint,
        class: MovementClass,
        start: Position,
        target: Position,
    ) -> Option<Vec<Position>> {
        let data = self.world.navigation_data.with_unchanged_terrain();
        self.finder(&data)
            .find_path_near(footprint, class, start, target)
    }
    pub fn find_path_to_any(
        &self,
        footprint: Footprint,
        class: MovementClass,
        start: Position,
        targets: &[Position],
    ) -> Option<(Position, Vec<Position>)> {
        let data = self.world.navigation_data.with_unchanged_terrain();
        self.finder(&data)
            .find_path_to_any(footprint, class, start, targets)
    }
}
