use super::*;

fn world() -> World {
    let caster = UnitType {
        id: UnitTypeId(1),
        max_hp: 200,
        speed: 8,
        vision_range: 192,
        energy_pool: Some(EnergyPool {
            maximum: 200,
            initial: 200,
            regeneration: 8,
        }),
        abilities: vec![
            TargetedAbility {
                id: AbilityId(1),
                research: Some(ResearchId(1)),
                energy: 100,
                range: 128,
                effect: AbilityEffect::DrainArea { radius: 32 },
            },
            TargetedAbility {
                id: AbilityId(2),
                research: Some(ResearchId(2)),
                energy: 75,
                range: 128,
                effect: AbilityEffect::DamageAura {
                    radius: 32,
                    damage_fp8: 10 * 256,
                    period: 8,
                    duration: 32,
                    affected: vec![UnitTypeId(3)],
                },
            },
        ],
        ..Default::default()
    };
    let facility = UnitType {
        id: UnitTypeId(2),
        max_hp: 100,
        speed: 0,
        structure: true,
        flight: Some(Flight {
            speed: 8,
            lift_ticks: 2,
            land_ticks: 2,
        }),
        ..Default::default()
    };
    let organic = UnitType {
        id: UnitTypeId(3),
        max_hp: 30,
        armor: 100,
        speed: 0,
        ..Default::default()
    };
    let mechanical = UnitType {
        id: UnitTypeId(4),
        max_hp: 200,
        max_shields: 100,
        speed: 0,
        energy_pool: Some(EnergyPool {
            maximum: 200,
            initial: 200,
            regeneration: 0,
        }),
        ..Default::default()
    };
    let tank = UnitType {
        id: UnitTypeId(5),
        max_hp: 150,
        speed: 4,
        mode: Some(ModeChange {
            target: UnitTypeId(6),
            ticks: 4,
            research: Some(ResearchId(4)),
        }),
        ..Default::default()
    };
    let siege = UnitType {
        id: UnitTypeId(6),
        max_hp: 150,
        speed: 0,
        mode: Some(ModeChange {
            target: UnitTypeId(5),
            ticks: 3,
            research: None,
        }),
        ..Default::default()
    };
    World::new(
        Rules {
            id: "abilities".into(),
            tick_ms: 42,
            units: vec![caster, facility, organic, mechanical, tank, siege],
            starting_resources: vec![ResourceAmount {
                kind: "minerals".into(),
                amount: 1000,
            }],
            research: vec![
                Research {
                    available: true,
                    id: ResearchId(1),
                    facility: UnitTypeId(2),
                    previous: None,
                    prerequisites: Vec::new(),
                    ticks: 2,
                    cost: vec![ResourceAmount {
                        kind: "minerals".into(),
                        amount: 100,
                    }],
                    effect: ResearchEffect::Ability {
                        units: vec![UnitTypeId(1)],
                        ability: AbilityId(1),
                    },
                },
                Research {
                    available: true,
                    id: ResearchId(2),
                    facility: UnitTypeId(2),
                    previous: None,
                    prerequisites: Vec::new(),
                    ticks: 2,
                    cost: vec![],
                    effect: ResearchEffect::Ability {
                        units: vec![UnitTypeId(1)],
                        ability: AbilityId(2),
                    },
                },
                Research {
                    available: true,
                    id: ResearchId(3),
                    facility: UnitTypeId(2),
                    previous: None,
                    prerequisites: Vec::new(),
                    ticks: 2,
                    cost: vec![],
                    effect: ResearchEffect::EnergyCapacity {
                        units: vec![UnitTypeId(1)],
                        amount: 50,
                    },
                },
                Research {
                    available: true,
                    id: ResearchId(4),
                    facility: UnitTypeId(2),
                    previous: None,
                    prerequisites: Vec::new(),
                    ticks: 2,
                    cost: vec![],
                    effect: ResearchEffect::Mode {
                        units: vec![UnitTypeId(5)],
                    },
                },
            ],
            ..Default::default()
        },
        Map {
            id: "abilities".into(),
            width: 512,
            height: 512,
            players: 2,
            spawns: vec![
                spawn(0, 1, 64, 64),
                spawn(0, 2, 64, 128),
                spawn(0, 5, 64, 192),
                spawn(1, 3, 160, 64),
                spawn(1, 4, 176, 64),
            ],
            ai: vec![],
            start_locations: vec![],
            resources: vec![],
            creation: BTreeMap::new(),
            initial_explored: BTreeMap::new(),
            fog_of_war: false,
            mission: None,
            terrain: None,
        },
        42,
    )
    .unwrap()
}
fn spawn(owner: u16, kind: u16, x: i32, y: i32) -> Spawn {
    Spawn {
        owner: PlayerId(owner),
        unit_type: UnitTypeId(kind),
        position: Position { x, y },
        ..Default::default()
    }
}
fn id(w: &World, kind: u16) -> EntityId {
    w.state
        .entities
        .iter()
        .find(|e| e.unit_type == UnitTypeId(kind))
        .unwrap()
        .id
}
fn issue(w: &mut World, order: Order) -> Option<Rejection> {
    w.step(&[Command {
        tick: w.tick(),
        player: PlayerId(0),
        sequence: w.state.last_sequences[0] + 1,
        order,
    }])
    .unwrap()[0]
        .rejection
        .clone()
}
fn research(w: &mut World, research: u16) {
    assert_eq!(
        issue(
            w,
            Order::Research {
                entity: id(w, 2),
                research: ResearchId(research)
            }
        ),
        None
    );
    w.step(&[]).unwrap();
    assert!(w.has_research(PlayerId(0), ResearchId(research)));
}

#[test]
fn facility_research_pays_unlocks_and_preserves_upgraded_energy_in_saves() {
    let mut w = world();
    let caster = id(&w, 1);
    let lab = id(&w, 2);
    let point = AbilityTarget::Point(Position { x: 176, y: 64 });
    assert_eq!(
        issue(
            &mut w,
            Order::Cast {
                entity: caster,
                ability: AbilityId(1),
                target: point
            }
        ),
        Some(Rejection::MissingPrerequisite)
    );
    research(&mut w, 1);
    assert_eq!(w.resource_balance(PlayerId(0), "minerals"), 900);
    assert_eq!(
        issue(
            &mut w,
            Order::Cast {
                entity: caster,
                ability: AbilityId(1),
                target: point
            }
        ),
        None
    );
    let victim = &w.state.entities[w.index(id(&w, 4)).unwrap()];
    assert_eq!((victim.shields, victim.energy, victim.hp), (0, 0, 200));
    assert_eq!(w.state.entities[w.index(caster).unwrap()].energy, 100 * 256);
    research(&mut w, 3);
    let i = w.index(caster).unwrap();
    w.state.entities[i].energy = 250 * 256;
    let saved = w.save_snapshot().unwrap();
    assert_eq!(
        w.restore_snapshot(saved).unwrap().state_hash(),
        w.state_hash()
    );
    assert_eq!(issue(&mut w, Order::Lift { entity: lab }), None);
    assert_eq!(
        issue(
            &mut w,
            Order::Research {
                entity: lab,
                research: ResearchId(2)
            }
        ),
        Some(Rejection::UnsupportedOrder)
    );
}

#[test]
fn aura_ignores_armor_spares_mechanical_units_and_keeps_attribution_after_caster_dies() {
    let mut w = world();
    let caster = id(&w, 1);
    let host = id(&w, 4);
    research(&mut w, 2);
    assert_eq!(
        issue(
            &mut w,
            Order::Cast {
                entity: caster,
                ability: AbilityId(2),
                target: AbilityTarget::Unit(host)
            }
        ),
        None
    );
    let view = w.player_view(PlayerId(1)).unwrap();
    let own = view
        .entities
        .iter()
        .find_map(|e| match e {
            ViewedEntity::Owned(e) if e.id == host => Some(e),
            _ => None,
        })
        .unwrap();
    assert_eq!(own.ability_auras[0].source, None);
    assert_eq!(own.ability_auras[0].owner, PlayerId(1));
    let i = w.index(caster).unwrap();
    w.state.entities[i].hp = 0;
    w.step(&[]).unwrap();
    let mut restored = w.restore_snapshot(w.save_snapshot().unwrap()).unwrap();
    for _ in 0..32 {
        w.step(&[]).unwrap();
        restored.step(&[]).unwrap();
    }
    assert_eq!(w.state_hash(), restored.state_hash());
    assert!(
        !w.state
            .entities
            .iter()
            .any(|e| e.unit_type == UnitTypeId(3))
    );
    assert_eq!(w.state.entities[w.index(host).unwrap()].hp, 200);
    assert_eq!(w.state.statistics[0].units_killed, 1);
    assert!(
        w.state.entities[w.index(host).unwrap()]
            .ability_auras
            .is_empty()
    );
}

#[test]
fn researched_modes_finish_once_lock_movement_and_restore_mid_transition() {
    let mut w = world();
    let tank = id(&w, 5);
    assert_eq!(
        issue(&mut w, Order::ChangeMode { entity: tank }),
        Some(Rejection::MissingPrerequisite)
    );
    research(&mut w, 4);
    assert_eq!(issue(&mut w, Order::ChangeMode { entity: tank }), None);
    let mut restored = w.restore_snapshot(w.save_snapshot().unwrap()).unwrap();
    let public = w.player_view(PlayerId(1)).unwrap();
    assert!(public.entities.iter().any(|e| matches!(e, ViewedEntity::Visible(e) if e.id == tank && e.appearance.mode_transition.is_some())));
    for _ in 0..3 {
        w.step(&[]).unwrap();
        restored.step(&[]).unwrap();
    }
    assert_eq!(w.state_hash(), restored.state_hash());
    assert_eq!(
        w.state.entities[w.index(tank).unwrap()].unit_type,
        UnitTypeId(6)
    );
    assert_eq!(
        issue(
            &mut w,
            Order::Move {
                entity: tank,
                target: Position { x: 256, y: 192 }
            }
        ),
        Some(Rejection::UnsupportedOrder)
    );
    assert_eq!(issue(&mut w, Order::ChangeMode { entity: tank }), None);
    for _ in 0..2 {
        w.step(&[]).unwrap();
    }
    assert_eq!(
        w.state.entities[w.index(tank).unwrap()].unit_type,
        UnitTypeId(5)
    );
    assert_eq!(w.state.entities[w.index(tank).unwrap()].hp, 150);
}
