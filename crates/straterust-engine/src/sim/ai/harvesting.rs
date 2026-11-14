//! Town orders need a known, reachable resource and a route back to a depot.
use super::*;

impl World {
    pub(super) fn ai_interior_source(&self, actor: &Entity) -> Option<(Position, Footprint)> {
        if let Some(at) = actor.dropoff_target.and_then(|id| self.index(id)) {
            Some((self.state.entities[at].position, self.unit_at(at).footprint))
        } else if let UnitOrder::Gather { resource } = actor.order {
            self.collision_resource(resource)
                .map(|r| (r.position, r.footprint))
        } else {
            None
        }
    }
    pub(super) fn ai_manages_resources(controller: &AiController) -> bool {
        controller.takeover || !controller.harvest_weights.is_empty()
    }

    pub(super) fn ai_gather_candidate(
        &self,
        controller: &AiController,
        actor: &Entity,
        node: &ResourceNode,
    ) -> bool {
        self.unit_type(actor.unit_type)
            .and_then(|u| u.worker.as_ref())
            .is_some_and(|w| {
                node.amount > 0
                    && w.resource_kinds.contains(&node.kind)
                    && (!node.requires_extractor || self.extractor(actor.owner, node).is_some())
                    && if Self::ai_manages_resources(controller) {
                        self.visibility(actor.owner, node.position) != Visibility::Unexplored
                    } else {
                        rts::distance(node.position, controller.home)
                            <= i64::from(controller.radius).pow(2)
                    }
            })
    }

    pub(super) fn ai_gather_choice(
        &self,
        controller: &AiController,
        state: &AiState,
        actor: &Entity,
        candidates: &[ResourceId],
    ) -> Option<ResourceId> {
        if !Self::ai_manages_resources(controller) {
            return candidates.first().copied();
        }
        let candidates = self.ai_allowed_mines(controller, state, actor, candidates);
        // Try preferred nodes first, then rotate a small fallback page. Failed
        // components share the navigator's proof; never probe every tree for
        // every idle worker, or retry only the same inaccessible nodes forever.
        let tail = candidates.len().saturating_sub(4);
        let offset = (self.tick().0 / 64 % tail.div_ceil(4).max(1) as u64) as usize * 4;
        candidates
            .iter()
            .take(4)
            .chain(candidates.iter().skip(4 + offset).take(4))
            .copied()
            .find(|&id| {
                self.collision_resource(id).is_some_and(|node| {
                    !self.ai_mine_threatened(actor.owner, node.position)
                        && self
                            .ai_resource_approach(actor, node)
                            .is_some_and(|arrival| {
                                let unit = self.unit_type(actor.unit_type).unwrap();
                                let mut depots: Vec<_> = self
                                    .state
                                    .entities
                                    .iter()
                                    .filter(|e| {
                                        e.owner == actor.owner
                                            && e.hp > 0
                                            && e.construction.is_none()
                                            && !e.airborne
                                            && self
                                                .unit_type(e.unit_type)
                                                .is_some_and(|u| u.dropoff.contains(&node.kind))
                                    })
                                    .collect();
                                depots.sort_by_key(|e| (rts::distance(arrival, e.position), e.id));
                                let goals: Vec<_> = depots
                                    .into_iter()
                                    .flat_map(|e| {
                                        rts::perimeter(
                                            e.position,
                                            self.unit_type(e.unit_type).unwrap().footprint,
                                            unit.footprint,
                                            arrival,
                                        )
                                    })
                                    .collect();
                                self.navigator()
                                    .find_path_to_any(
                                        unit.footprint,
                                        unit.movement_class,
                                        arrival,
                                        &goals,
                                    )
                                    .is_some()
                            })
                })
            })
    }

    pub(super) fn ai_resource_approach(
        &self,
        actor: &Entity,
        node: &ResourceNode,
    ) -> Option<Position> {
        let unit = self.unit_type(actor.unit_type)?;
        let footprint = self
            .extractor(actor.owner, node)
            .map_or(node.footprint, |at| self.unit_at(at).footprint);
        let starts = if actor.gathering_inside {
            let (position, source) = self.ai_interior_source(actor)?;
            rts::perimeter(position, source, unit.footprint, node.position)
                .into_iter()
                .filter(|&p| self.can_place(p, unit.footprint, unit.movement_class, Some(actor.id)))
                .collect()
        } else {
            vec![actor.position]
        };
        starts.into_iter().find_map(|start| {
            self.navigator()
                .find_path_to_any(
                    unit.footprint,
                    unit.movement_class,
                    start,
                    &rts::perimeter(node.position, footprint, unit.footprint, actor.position),
                )
                .map(|(arrival, _)| arrival)
        })
    }
}
