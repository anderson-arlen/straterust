//! Harvest an accessible forest edge when it blocks needed construction.
use super::*;

impl World {
    pub(super) fn ai_clearable_resource(&self, resource: &ResourceNode) -> bool {
        self.map
            .resources
            .get(resource.id.0.saturating_sub(1) as usize)
            .is_some_and(|r| r.terrain_corners.is_some())
            && self.rules.units.iter().any(|u| {
                u.worker
                    .as_ref()
                    .is_some_and(|w| w.resource_kinds.contains(&resource.kind))
            })
    }

    pub(super) fn ai_clear_build_site(
        &self,
        player: PlayerId,
        builders: &[EntityId],
        building: &UnitType,
        position: Position,
    ) -> Vec<(EntityId, ResourceId)> {
        let gap = i64::from(self.ai_building_gap(player, building) / 2);
        let mut trees: Vec<_> = self
            .state
            .resources
            .iter()
            .filter(|r| {
                r.amount > 0
                    && (i64::from(position.x) - i64::from(r.position.x)).abs() * 2
                        < i64::from(building.placement.width)
                            + i64::from(r.footprint.width)
                            + gap * 2
                    && (i64::from(position.y) - i64::from(r.position.y)).abs() * 2
                        < i64::from(building.placement.height)
                            + i64::from(r.footprint.height)
                            + gap * 2
                    && self.ai_clearable_resource(r)
            })
            .collect();
        trees.sort_by_key(|r| (rts::distance(position, r.position), r.id));
        let Some(first) = trees.first() else {
            return Vec::new();
        };
        let kind = &first.kind;
        let mut builders = builders.to_vec();
        // Reuse a woodcutter so balancing does not send a displaced gold miner
        // straight back to the mine before it can finish a single chop.
        builders.sort_by_key(|id| {
            let actor = &self.state.entities[self.index(*id).unwrap()];
            let gathering_kind = match actor.order {
                UnitOrder::Gather { resource } => self
                    .state
                    .resources
                    .iter()
                    .find(|r| r.id == resource)
                    .map(|r| &r.kind),
                _ => None,
            };
            let already_clearing = actor.harvest_progress > 0
                && matches!(actor.order,
                UnitOrder::Gather {resource} if trees.iter().any(|r|r.id == resource));
            (
                !already_clearing,
                gathering_kind != Some(kind),
                actor.cargo.is_some(),
                rts::distance(actor.position, position),
                *id,
            )
        });
        let mut orders = Vec::new();
        let mut assigned = BTreeSet::new();
        for entity in builders {
            let actor = &self.state.entities[self.index(entity).unwrap()];
            let gathering_kind = match actor.order {
                UnitOrder::Gather { resource } => self
                    .state
                    .resources
                    .iter()
                    .find(|r| r.id == resource)
                    .map(|r| &r.kind),
                _ => None,
            };
            // Keep the rest of the economy working. A sole worker can clear
            // its first farm; established towns use up to four woodcutters.
            if orders.len() == 4 || !orders.is_empty() && gathering_kind != Some(kind) {
                break;
            }
            if actor.harvest_progress > 0
                && let UnitOrder::Gather { resource } = actor.order
                && trees.iter().any(|r| r.id == resource)
                && assigned.insert(resource)
            {
                orders.push((entity, resource));
                continue;
            }
            let worker = self.unit_type(actor.unit_type).unwrap();
            let finder = self.navigator();
            let targets: Vec<_> = trees
                .iter()
                .filter(|tree| {
                    !assigned.contains(&tree.id) && self.gather_rejection(entity, tree.id).is_none()
                })
                .flat_map(|tree| {
                    rts::perimeter(
                        tree.position,
                        tree.footprint,
                        worker.footprint,
                        actor.position,
                    )
                    .into_iter()
                    .map(|p| (p, tree.id))
                })
                .collect();
            let goals: Vec<_> = targets.iter().map(|(p, _)| *p).collect();
            if let Some((at, _)) = finder.find_path_to_any(
                worker.footprint,
                worker.movement_class,
                actor.position,
                &goals,
            ) && let Some((_, resource)) = targets.iter().find(|(p, _)| *p == at)
            {
                orders.push((entity, *resource));
                assigned.insert(*resource);
            }
        }
        orders
    }
}
