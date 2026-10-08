//! Content-configured ambient movement through the ordinary collision and motion paths.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdleWander {
    pub distance: u16,
    pub pause_ticks: [u16; 2],
    /// Stop retrying a short ambient walk if it remains obstructed.
    pub move_ticks: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WanderState {
    pub target: Option<Position>,
    pub remaining: u32,
}

impl World {
    pub(in crate::sim) fn advance_wander(&mut self, index: usize) {
        let Some(config) = self.unit_at(index).idle_wander.clone() else {
            return;
        };
        if self.movement_locked(&self.state.entities[index]) {
            return;
        }
        if let Some(state) = self.state.entities[index].wander.clone() {
            let finished = state
                .target
                .is_some_and(|target| self.navigate(index, target, false));
            if !finished && state.remaining > 0 {
                self.state.entities[index]
                    .wander
                    .as_mut()
                    .unwrap()
                    .remaining -= 1;
                return;
            }
            self.assign(index, UnitOrder::Idle, false);
            if state.target.is_some() {
                let [low, high] = config.pause_ticks;
                let remaining = u32::from(low)
                    + (splitmix64(&mut self.state.rng_state) % u64::from(high - low + 1)) as u32;
                self.state.entities[index].wander = Some(WanderState {
                    target: None,
                    remaining,
                });
                return;
            }
        }
        let x = (splitmix64(&mut self.state.rng_state) % 65535) as i64 - 32767;
        let y = (splitmix64(&mut self.state.rng_state) % 65535) as i64 - 32767;
        let length = ((x * x + y * y) as u64).isqrt().max(1) as i64;
        let footprint = self.unit_at(index).footprint;
        let position = self.state.entities[index].position;
        let target = Position {
            x: (position.x + (x * i64::from(config.distance) / length) as i32).clamp(
                i32::from(footprint.width / 2),
                self.map.width - i32::from(footprint.width.div_ceil(2)),
            ),
            y: (position.y + (y * i64::from(config.distance) / length) as i32).clamp(
                i32::from(footprint.height / 2),
                self.map.height - i32::from(footprint.height.div_ceil(2)),
            ),
        };
        self.state.entities[index].wander = Some(WanderState {
            target: Some(target),
            remaining: u32::from(config.move_ticks),
        });
        self.navigate(index, target, false);
    }
}

pub(super) fn validate(rules: &Rules) -> Result<()> {
    for unit in &rules.units {
        if let Some(config) = &unit.idle_wander {
            ensure!(
                !unit.structure
                    && unit.speed > 0
                    && (1..=2048).contains(&config.distance)
                    && config.pause_ticks[0] <= config.pause_ticks[1]
                    && config.move_ticks > 0,
                "invalid idle wandering configuration"
            );
        }
    }
    Ok(())
}

pub(super) fn put_rules(bytes: &mut Vec<u8>, rules: &Rules) {
    for unit in &rules.units {
        if let Some(config) = &unit.idle_wander {
            bytes.extend(b"idle-wander-v1");
            bytes.extend(unit.id.0.to_le_bytes());
            put_string(
                bytes,
                &ron::ser::to_string(config).expect("serializable idle wander"),
            );
        }
    }
}
pub(super) fn put_state(bytes: &mut Vec<u8>, state: &State) {
    for entity in &state.entities {
        if let Some(wander) = &entity.wander {
            bytes.extend(b"idle-wander-v1");
            bytes.extend(entity.id.0.to_le_bytes());
            put_string(
                bytes,
                &ron::ser::to_string(wander).expect("serializable idle wander"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_wandering_resumes_deterministically_and_yields_to_player_orders() {
        let rules = Rules {
            id: "wildlife".into(),
            units: vec![UnitType {
                speed: 4,
                idle_wander: Some(IdleWander {
                    distance: 32,
                    pause_ticks: [0, 75],
                    move_ticks: 75,
                }),
                ..Default::default()
            }],
            ..Default::default()
        };
        let map: Map = ron::from_str(
            "(id: \"wildlife\", width: 512, height: 512, players: 1,
            spawns: [(owner: 0, unit_type: 0, position: (x: 256, y: 256))])",
        )
        .unwrap();
        let mut w = World::new(rules, map, 5).unwrap();
        let origin = w.state.entities[0].position;
        for _ in 0..8 {
            w.step(&[]).unwrap();
        }
        assert_ne!(w.state.entities[0].position, origin);
        assert_eq!(w.state.entities[0].order, UnitOrder::Idle);
        let mut resumed = w.restore_snapshot(w.save_snapshot().unwrap()).unwrap();
        for _ in 0..160 {
            w.step(&[]).unwrap();
            resumed.step(&[]).unwrap();
        }
        assert_eq!(w.state_hash(), resumed.state_hash());
        let target = Position { x: 400, y: 400 };
        w.step(&[Command {
            tick: w.tick(),
            player: PlayerId(0),
            sequence: 1,
            order: Order::Move {
                entity: EntityId(1),
                target,
            },
        }])
        .unwrap();
        assert!(w.state.entities[0].wander.is_none());
        for _ in 0..80 {
            w.step(&[]).unwrap();
        }
        w.step(&[Command {
            tick: w.tick(),
            player: PlayerId(0),
            sequence: 2,
            order: Order::Hold {
                entity: EntityId(1),
            },
        }])
        .unwrap();
        let position = w.state.entities[0].position;
        for _ in 0..100 {
            w.step(&[]).unwrap();
        }
        assert_eq!(w.state.entities[0].position, position);
        assert!(w.state.entities[0].wander.is_none());
    }
}
