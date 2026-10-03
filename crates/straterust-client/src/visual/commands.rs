//! Click acknowledgement uses presentation time and never modifies orders.
use super::*;
use straterust_engine::sim::ResourceId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandTarget {
    Ground(Position),
    Entity(EntityId),
    Resource(ResourceId),
}

impl CommandTarget {
    pub fn resource(world: &World, id: ResourceId) -> Self {
        world
            .state()
            .resources
            .iter()
            .find(|node| node.id == id)
            .and_then(|node| {
                world.state().entities.iter().find(|entity| {
                    entity.position == node.position
                        && world.unit_type(entity.unit_type).is_some_and(|unit| {
                            unit.extracts
                                .as_ref()
                                .is_some_and(|extraction| extraction.resource == node.kind)
                        })
                })
            })
            .map_or(Self::Resource(id), |entity| Self::Entity(entity.id))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CommandFeedback {
    pub target: CommandTarget,
    pub elapsed: Duration,
}

impl CommandFeedback {
    pub fn visible(self) -> bool {
        (self.elapsed.as_millis() / 100).is_multiple_of(2)
    }
}

impl Visuals {
    pub fn show_command_feedback(&mut self, target: CommandTarget) {
        self.command_feedback = Some(CommandFeedback {
            target,
            elapsed: Duration::ZERO,
        });
    }

    pub fn command_feedback(&self) -> Option<CommandFeedback> {
        self.command_feedback
    }

    pub(super) fn advance_command_feedback(&mut self, elapsed: Duration) {
        if let Some(feedback) = &mut self.command_feedback {
            feedback.elapsed = feedback.elapsed.saturating_add(elapsed);
            if feedback.elapsed >= Duration::from_millis(600) {
                self.command_feedback = None;
            }
        }
    }
}
