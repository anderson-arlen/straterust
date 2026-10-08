//! Content-defined charged projectiles and remote, descending strikes.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrikeDelivery {
    pub charge_ticks: u32,
    pub ascent_ticks: u32,
    pub warning_ticks: u32,
    pub transit_ticks: u32,
    pub descent_height: u32,
    pub speed_fp8: u32,
    pub acceleration_fp8: u32,
    pub impact_ticks: u32,
    #[serde(default)]
    pub reveal_radius: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum StrikeStage {
    Charge,
    Ascent,
    Transit,
    Flight,
    Impact,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrikeFlight {
    pub started: Tick,
    pub stage: StrikeStage,
    pub elapsed: u32,
    pub position: Position,
    pub leg_origin: Position,
    pub destination: Position,
    pub distance_fp8: u32,
    pub velocity_fp8: u32,
    pub warning: bool,
}

/// Only visible positions cross the server boundary. A global warning never
/// discloses the launcher, caster, hidden destination or missile position.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrikeAppearance {
    pub ability: AbilityId,
    pub started: Tick,
    pub stage: Option<StrikeStage>,
    pub elapsed: u32,
    pub position: Option<Position>,
    pub marker: Option<Position>,
    pub caster: Option<EntityId>,
    pub heading: [i16; 2],
    pub velocity_fp8: u32,
    pub warning: bool,
}

impl StrikeFlight {
    pub(super) fn new(
        started: Tick,
        origin: Position,
        destination: Position,
        d: &StrikeDelivery,
    ) -> Self {
        Self {
            started,
            stage: if d.ascent_ticks > 0 {
                StrikeStage::Ascent
            } else {
                StrikeStage::Charge
            },
            elapsed: 0,
            position: origin,
            leg_origin: origin,
            destination,
            distance_fp8: 0,
            velocity_fp8: 0,
            warning: false,
        }
    }
    fn enter(&mut self, stage: StrikeStage, position: Position) {
        self.stage = stage;
        self.elapsed = 0;
        self.position = position;
        self.leg_origin = position;
        self.distance_fp8 = 0;
        self.velocity_fp8 = 0;
    }
    fn travel(&mut self, to: Position, d: &StrikeDelivery) -> bool {
        self.velocity_fp8 = self
            .velocity_fp8
            .saturating_add(d.acceleration_fp8)
            .min(d.speed_fp8);
        self.distance_fp8 = self.distance_fp8.saturating_add(self.velocity_fp8);
        let dx = i64::from(to.x - self.leg_origin.x);
        let dy = i64::from(to.y - self.leg_origin.y);
        let length = ((dx * dx + dy * dy) as u64).isqrt().max(1) * 256;
        let travelled = u64::from(self.distance_fp8).min(length);
        self.position = Position {
            x: self.leg_origin.x + (dx * travelled as i64 / length as i64) as i32,
            y: self.leg_origin.y + (dy * travelled as i64 / length as i64) as i32,
        };
        travelled == length
    }
}

impl World {
    pub fn entity_casting(&self, id: EntityId) -> bool {
        if let Some(view) = &self.view {
            return view.strikes.iter().any(|s| s.caster == Some(id));
        }
        self.state
            .pending_effects
            .iter()
            .any(|e| e.source == id && e.flight.is_some() && e.channel > 0)
    }
    pub fn strike_appearances(&self, player: PlayerId) -> Vec<StrikeAppearance> {
        if let Some(view) = &self.view {
            return view.strikes.clone();
        }
        let visible = |p| self.terrain_visibility(player, p) == Visibility::Visible;
        self.state
            .pending_effects
            .iter()
            .filter_map(|e| {
                let f = e.flight.as_ref()?;
                let caster =
                    (e.channel > 0 && self.entity_visible(player, e.source)).then_some(e.source);
                let marker = matches!(f.stage, StrikeStage::Ascent | StrikeStage::Transit)
                    .then_some(f.destination)
                    .filter(|p| visible(*p));
                let position = (f.stage != StrikeStage::Transit)
                    .then_some(f.position)
                    .filter(|p| visible(*p));
                (position.is_some() || marker.is_some() || caster.is_some() || f.warning).then_some(
                    StrikeAppearance {
                        ability: e.ability,
                        started: f.started,
                        stage: position.map(|_| f.stage),
                        elapsed: if position.is_some() { f.elapsed } else { 0 },
                        position,
                        marker,
                        caster,
                        heading: if position.is_some() {
                            player_view::public_heading(
                                f.leg_origin,
                                if f.stage == StrikeStage::Ascent {
                                    Position {
                                        x: f.leg_origin.x,
                                        y: 0,
                                    }
                                } else {
                                    f.destination
                                },
                            )
                        } else {
                            [0, 0]
                        },
                        velocity_fp8: if position.is_some() {
                            f.velocity_fp8
                        } else {
                            0
                        },
                        warning: f.warning,
                    },
                )
            })
            .collect()
    }

    /// Returns (retain, apply damage). Committed projectiles survive loss of
    /// their caster; an interrupted channel before commitment consumes ammo.
    pub(super) fn advance_strike(
        &mut self,
        effect: &mut PendingEffect,
        d: &StrikeDelivery,
    ) -> (bool, bool) {
        let source = self.index(effect.source);
        let tracking = source.is_some_and(|i| {
            let e = &self.state.entities[i];
            e.hp > 0
                && e.owner == effect.owner
                && !self.disabled(e)
                && e.order
                    == (UnitOrder::Cast {
                        ability: effect.ability,
                        target: effect.target,
                    })
        });
        let f = effect.flight.as_mut().unwrap();
        if !matches!(f.stage, StrikeStage::Flight | StrikeStage::Impact) && !tracking {
            return (false, false);
        }
        if d.ascent_ticks == 0 && effect.channel > 0 {
            effect.channel -= 1;
            if effect.channel == 0 && tracking {
                self.finish(source.unwrap());
            }
        }
        f.elapsed += 1;
        match f.stage {
            StrikeStage::Charge => {
                if let AbilityTarget::Unit(id) = effect.target
                    && let Some(i) = self.index(id)
                {
                    f.destination = self.state.entities[i].position;
                }
                if f.elapsed >= d.charge_ticks {
                    f.enter(StrikeStage::Flight, f.position);
                }
            }
            StrikeStage::Ascent => {
                let reached = f.travel(
                    Position {
                        x: effect.origin.x,
                        y: 0,
                    },
                    d,
                );
                f.warning |= f.elapsed >= d.warning_ticks || reached;
                if reached || f.elapsed >= d.ascent_ticks {
                    f.enter(StrikeStage::Transit, f.position);
                }
            }
            StrikeStage::Transit => {
                if f.elapsed >= d.transit_ticks {
                    f.enter(
                        StrikeStage::Flight,
                        Position {
                            x: f.destination.x,
                            y: (f.destination.y - d.descent_height as i32).max(0),
                        },
                    );
                    effect.channel = 0;
                    if tracking {
                        self.finish(source.unwrap());
                    }
                }
            }
            StrikeStage::Flight => {
                if d.ascent_ticks == 0
                    && let AbilityTarget::Unit(id) = effect.target
                    && let Some(i) = self.index(id)
                {
                    f.destination = self.state.entities[i].position;
                }
                if f.travel(f.destination, d) {
                    f.enter(StrikeStage::Impact, f.destination);
                    return (true, true);
                }
            }
            StrikeStage::Impact => {
                if f.elapsed >= d.impact_ticks && effect.channel == 0 {
                    return (false, false);
                }
            }
        }
        (true, false)
    }
}
