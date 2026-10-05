//! Player-scoped inspection. Never serialize the authoritative World as a
//! network player view: private orders/jobs are available only to their owner.
use super::*;

#[derive(Debug, Serialize)]
pub struct EntityInspection<'a> {
    pub id: EntityId,
    pub owner: PlayerId,
    pub unit_type: UnitTypeId,
    pub position: Position,
    pub hp: u32,
    pub under_construction: bool,
    /// Public appearance only; does not identify training, research or its job.
    pub working: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owned: Option<&'a Entity>,
}

impl World {
    /// Placement reserves space before the builder arrives. It has no visible
    /// foundation yet; addons start at their parent and do not wait for a worker.
    pub fn construction_pending(&self, entity: &Entity) -> bool {
        entity.construction.as_ref().is_some_and(|work| {
            work.worker.is_some()
                && work.work_position.is_none()
                && self
                    .unit_type(entity.unit_type)
                    .is_some_and(|u| u.addon_parent.is_none())
        })
    }

    /// Working artwork is observable without exposing the job causing it.
    pub fn entity_working(&self, id: EntityId) -> bool {
        if let Some(view) = &self.view {
            return view.working.contains(&id);
        }
        let Some(entity) = self.state.entities.iter().find(|entity| entity.id == id) else {
            return false;
        };
        let active = |entity: &Entity| {
            entity.research.is_some()
                || entity
                    .production
                    .front()
                    .is_some_and(|job| job.started && job.remaining > 0)
        };
        entity.construction.is_none()
            && (active(entity)
                || entity.parent.is_some_and(|id| {
                    self.state
                        .entities
                        .iter()
                        .any(|parent| parent.id == id && active(parent))
                })
                || self.constructing_addon(entity.id))
    }

    pub fn inspect_entity(&self, player: PlayerId, id: EntityId) -> Option<EntityInspection<'_>> {
        if player.0 >= self.map.players || !self.entity_visible(player, id) {
            return None;
        }
        let entity = self.state.entities.iter().find(|entity| entity.id == id)?;
        Some(EntityInspection {
            id,
            owner: entity.owner,
            unit_type: entity.unit_type,
            position: entity.position,
            hp: entity.hp,
            under_construction: entity.construction.is_some(),
            working: self.entity_working(id),
            owned: (entity.owner == player).then_some(entity),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspection_serialization_omits_enemy_jobs_orders_and_cargo_but_keeps_activity() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/terran-demo");
        let mut world = crate::content::Package::load(&path)
            .unwrap()
            .world(42)
            .unwrap();
        let enemy = world
            .state
            .entities
            .iter()
            .find(|entity| entity.owner == PlayerId(1))
            .unwrap()
            .id;
        let index = world
            .state
            .entities
            .iter()
            .position(|entity| entity.id == enemy)
            .unwrap();
        world.state.entities[index]
            .production
            .push_back(ProductionJob {
                producer_type: None,
                unit_type: UnitTypeId(1),
                remaining: 40,
                total: 80,
                started: true,
            });
        world.state.entities[index]
            .queued_orders
            .push_back(UnitOrder::Hold);
        world.state.entities[index].cargo = Some(ResourceAmount {
            kind: "private-cargo".into(),
            amount: 5,
        });
        let public = world.inspect_entity(PlayerId(0), enemy).unwrap();
        assert!(public.working && public.owned.is_none());
        let encoded = ron::ser::to_string(&public).unwrap();
        for secret in [
            "owned",
            "production",
            "research",
            "queued_orders",
            "cargo",
            "private-cargo",
            "remaining",
            "total",
            "Hold",
        ] {
            assert!(!encoded.contains(secret), "leaked {secret}: {encoded}");
        }
        assert!(encoded.contains("working:true"));
        let own = world.inspect_entity(PlayerId(1), enemy).unwrap();
        assert!(own.owned.is_some());
        assert!(ron::ser::to_string(&own).unwrap().contains("private-cargo"));
        assert!(world.inspect_entity(PlayerId(99), enemy).is_none());
        assert!(world.inspect_entity(PlayerId(0), EntityId(99999)).is_none());
    }
}
