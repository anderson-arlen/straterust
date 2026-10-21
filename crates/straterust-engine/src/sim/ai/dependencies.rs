//! Paid prerequisite and producer planning for adaptive skirmish towns.
use super::*;

impl World {
    pub(super) fn ai_production_types(&self, player: PlayerId) -> BTreeSet<UnitTypeId> {
        let mut types: BTreeSet<_> = self
            .state
            .entities
            .iter()
            .filter(|e| e.owner == player)
            .map(|e| e.unit_type)
            .collect();
        loop {
            let before = types.len();
            for producer in &self.rules.units {
                if !types.contains(&producer.id) {
                    continue;
                }
                for &child in producer.builds.iter().chain(&producer.trains) {
                    let child = self.researched_unit_type(player, child);
                    if self.creation_allowed(player, child) {
                        types.insert(child);
                    }
                }
            }
            if types.len() == before {
                return types;
            }
        }
    }

    pub(super) fn ai_requested_count(
        &self,
        controller: &AiController,
        state: &AiState,
        request: &AiRequest,
    ) -> u16 {
        self.ai_unit_count(
            controller,
            state,
            request.unit_type,
            request.include_deployed,
        )
    }

    pub(super) fn ai_dependencies(
        &self,
        controller: &AiController,
        state: &AiState,
        requests: Vec<AiRequest>,
    ) -> Vec<AiRequest> {
        if !controller.takeover {
            return requests;
        }
        let mut result: Vec<AiRequest> = Vec::new();
        let producible = self.ai_production_types(controller.player);
        for request in requests {
            self.ai_dependency(
                controller,
                state,
                request,
                &mut BTreeSet::new(),
                &mut result,
                &producible,
            );
        }
        result
    }

    fn ai_dependency(
        &self,
        controller: &AiController,
        state: &AiState,
        request: AiRequest,
        visiting: &mut BTreeSet<UnitTypeId>,
        result: &mut Vec<AiRequest>,
        producible: &BTreeSet<UnitTypeId>,
    ) {
        if visiting.len() >= 16 || !visiting.insert(request.unit_type) {
            return;
        }
        if self.ai_requested_count(controller, state, &request) < request.count
            && let Some(unit) =
                self.unit_type(self.researched_unit_type(controller.player, request.unit_type))
        {
            let mut dependencies = unit.prerequisites.clone();
            let producer_exists = self.state.entities.iter().any(|e| {
                e.owner == controller.player
                    && self.unit_type(e.unit_type).is_some_and(|p| {
                        p.builds.contains(&unit.id) || self.can_train_type(e, unit.id)
                    })
            });
            if !producer_exists {
                let producer = self
                    .rules
                    .units
                    .iter()
                    .filter(|p| {
                        p.builds.contains(&unit.id)
                            || p.trains.iter().any(|id| {
                                self.researched_unit_type(controller.player, *id) == unit.id
                            })
                    })
                    .filter(|p| self.creation_allowed(controller.player, p.id))
                    .filter(|p| producible.contains(&p.id))
                    .min_by_key(|p| {
                        (
                            p.cost.iter().map(|c| u64::from(c.amount)).sum::<u64>(),
                            p.id,
                        )
                    });
                if let Some(producer) = producer {
                    dependencies.push(producer.id);
                }
            }
            for unit_type in dependencies {
                if self.creation_allowed(controller.player, unit_type)
                    && self.ai_unit_count(controller, state, unit_type, true) == 0
                {
                    self.ai_dependency(
                        controller,
                        state,
                        AiRequest {
                            unit_type,
                            count: 1,
                            priority: request.priority,
                            include_deployed: true,
                        },
                        visiting,
                        result,
                        producible,
                    );
                }
            }
        }
        visiting.remove(&request.unit_type);
        if let Some(existing) = result.iter_mut().find(|r| r.unit_type == request.unit_type) {
            existing.count = existing.count.max(request.count);
            existing.priority = existing.priority.max(request.priority);
            existing.include_deployed |= request.include_deployed;
        } else {
            result.push(request);
        }
    }

    pub(super) fn ai_producer_exists(&self, controller: &AiController, unit: &UnitType) -> bool {
        self.has_prerequisites(controller.player, unit)
            && self.creation_allowed(controller.player, unit.id)
            && self.state.entities.iter().any(|e| {
                e.owner == controller.player
                    && e.construction.is_none()
                    && self.unit_type(e.unit_type).is_some_and(|p| {
                        p.builds.contains(&unit.id) || self.can_train_type(e, unit.id)
                    })
            })
    }
}
