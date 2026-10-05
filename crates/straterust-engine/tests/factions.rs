//! Reusable faction mechanics, with entirely original synthetic content.
use straterust_engine::sim::*;

fn spawn(owner: u16, kind: u16, x: i32) -> Spawn {
    Spawn {
        owner: PlayerId(owner),
        unit_type: UnitTypeId(kind),
        position: Position { x, y: 64 },
        ..Spawn::default()
    }
}
fn definitions(units: Vec<UnitType>, spawns: Vec<Spawn>) -> World {
    World::new(
        Rules {
            id: "original-faction-rules".into(),
            units,
            starting_resources: vec![ResourceAmount {
                kind: "minerals".into(),
                amount: 1000,
            }],
            ..Rules::default()
        },
        Map {
            id: "original-faction-map".into(),
            width: 512,
            height: 256,
            players: 2,
            spawns,
            start_locations: vec![],
            resources: vec![],
            initial_explored: Default::default(),
            creation: Default::default(),
            ai: vec![],
            mission: None,
            fog_of_war: false,
            terrain: None,
        },
        42,
    )
    .unwrap()
}
fn step(world: &mut World, n: usize) {
    for _ in 0..n {
        world.step(&[]).unwrap();
    }
}
fn issue(world: &mut World, order: Order) {
    let sequence = world.state().last_sequences[0] + 1;
    let result = world.step(&[Command {
        tick: world.tick(),
        player: PlayerId(0),
        sequence,
        order,
    }]);
    assert!(
        result
            .unwrap()
            .iter()
            .all(|outcome| outcome.rejection.is_none())
    );
}
fn unit(id: u16) -> UnitType {
    UnitType {
        id: UnitTypeId(id),
        speed: 0,
        max_hp: 100,
        footprint: Footprint {
            width: 8,
            height: 8,
        },
        ..UnitType::default()
    }
}

#[test]
fn edge_building_spawn_uses_placement_bounds_like_construction_and_landing() {
    let mut building = unit(1);
    building.structure = true;
    building.placement = Footprint {
        width: 64,
        height: 64,
    };
    building.footprint = Footprint {
        width: 76,
        height: 47,
    };
    let mut parent = unit(2);
    parent.structure = true;
    parent.placement = Footprint {
        width: 128,
        height: 96,
    };
    parent.footprint = Footprint {
        width: 112,
        height: 81,
    };
    let mut attachment = unit(3);
    attachment.structure = true;
    attachment.placement = building.placement;
    attachment.footprint = Footprint {
        width: 71,
        height: 49,
    };
    let mut world = definitions(
        vec![building, parent, attachment],
        vec![spawn(0, 1, 480), spawn(0, 2, 160), spawn(0, 3, 256)],
    );
    step(&mut world, 1);
    assert_eq!(world.state().entities[0].position.x, 480);
    assert_eq!(
        world
            .restore_snapshot(world.save_snapshot().unwrap())
            .unwrap()
            .state_hash(),
        world.state_hash()
    );
}

#[test]
fn shields_absorb_normal_damage_before_health_armor_and_size_reduction() {
    let mut attacker = unit(1);
    attacker.weapon = Some(Weapon {
        damage: 20,
        range: 100,
        cooldown: 100,
        damage_kind: DamageKind::Explosive,
        cooldown_jitter: None,
        targets_air: false,
        splash: None,
        strikes: vec![],
    });
    let mut victim = unit(2);
    victim.max_shields = 10;
    victim.armor = 2;
    victim.size = UnitSize::Small;
    let mut world = definitions(
        vec![attacker, victim],
        vec![spawn(0, 1, 64), spawn(1, 2, 96)],
    );
    step(&mut world, 1);
    assert_eq!(world.state().entities[1].shields, 0);
    assert_eq!(world.state().entities[1].hp, 96);
    let view = world.player_view(PlayerId(1)).unwrap();
    assert_eq!(view.entities.len(), 2);
}

#[test]
fn damage_absorbed_by_shields_still_provokes_retaliation_beyond_sight() {
    let mut attacker = unit(1);
    attacker.vision_range = 256;
    attacker.weapon = Some(Weapon {
        damage: 20,
        range: 200,
        cooldown: 100,
        damage_kind: DamageKind::Normal,
        cooldown_jitter: None,
        targets_air: false,
        splash: None,
        strikes: vec![],
    });
    let mut victim = attacker.clone();
    victim.id = UnitTypeId(2);
    victim.speed = 4;
    victim.vision_range = 32;
    victim.max_shields = 40;
    let baseline = definitions(
        vec![attacker, victim],
        vec![spawn(0, 1, 64), spawn(1, 2, 160)],
    );
    let mut map = baseline.map().clone();
    map.fog_of_war = true;
    let mut world = World::new(baseline.rules().clone(), map, 42).unwrap();
    assert!(!world.entity_visible(PlayerId(1), EntityId(1)));
    step(&mut world, 1);
    let victim = &world.state().entities[1];
    assert_eq!(victim.hp, 100);
    assert_eq!(victim.shields, 20 * 256);
    assert_eq!(victim.auto_attack_target, Some(EntityId(1)));
}

#[test]
fn timed_morph_preserves_identity_pays_once_and_produces_a_pair() {
    let mut parent = unit(1);
    parent.transforms_on_production = true;
    parent.production_form = Some(UnitTypeId(2));
    parent.trains = vec![UnitTypeId(3)];
    parent.supply_provided = 10;
    let egg = unit(2);
    let mut child = unit(3);
    child.speed = 4;
    child.production_count = 2;
    child.build_ticks = 3;
    child.supply_used = 1;
    let mut world = definitions(vec![parent, egg, child], vec![spawn(0, 1, 64)]);
    issue(
        &mut world,
        Order::Train {
            entity: EntityId(1),
            unit_type: UnitTypeId(3),
        },
    );
    assert_eq!(world.state().entities[0].unit_type, UnitTypeId(2));
    assert_eq!(world.supply(PlayerId(0)).0, 2);
    step(&mut world, 3);
    assert_eq!(world.state().entities.len(), 2);
    assert_eq!(world.state().entities[0].id, EntityId(1));
    assert!(
        world
            .state()
            .entities
            .iter()
            .all(|e| e.unit_type == UnitTypeId(3))
    );
}

#[test]
fn automatic_children_are_bounded_and_replace_consumed_children() {
    let mut provider = unit(1);
    provider.structure = true;
    provider.offspring = Some(Offspring {
        unit_type: UnitTypeId(2),
        interval: 2,
        maximum: 2,
        initial: 2,
    });
    let mut child = unit(2);
    child.transforms_on_production = true;
    child.trains = vec![UnitTypeId(3)];
    let mut output = unit(3);
    output.build_ticks = 1;
    let mut world = definitions(vec![provider, child, output], vec![spawn(0, 1, 64)]);
    assert_eq!(world.state().entities.len(), 3);
    step(&mut world, 4);
    assert_eq!(world.state().entities.len(), 3);
    issue(
        &mut world,
        Order::Train {
            entity: EntityId(2),
            unit_type: UnitTypeId(3),
        },
    );
    step(&mut world, 4);
    assert_eq!(
        world
            .state()
            .entities
            .iter()
            .filter(|e| e.unit_type == UnitTypeId(2))
            .count(),
        2
    );
}

#[test]
fn transformation_reserves_supply_at_acceptance_and_rejects_another_without_payment() {
    let mut parent = unit(1);
    parent.transforms_on_production = true;
    parent.production_form = Some(UnitTypeId(2));
    parent.trains = vec![UnitTypeId(3)];
    let mut child = unit(3);
    child.supply_used = 1;
    child.build_ticks = 5;
    child.cost = vec![ResourceAmount {
        kind: "minerals".into(),
        amount: 10,
    }];
    let mut provider = unit(4);
    provider.supply_provided = 1;
    let mut world = definitions(
        vec![parent, unit(2), child, provider],
        vec![spawn(0, 1, 64), spawn(0, 1, 96), spawn(0, 4, 128)],
    );
    let balance = world.resource_balance(PlayerId(0), "minerals");
    let outcomes = world
        .step(
            &[1_u32, 2]
                .into_iter()
                .enumerate()
                .map(|(n, id)| Command {
                    tick: Tick(0),
                    player: PlayerId(0),
                    sequence: n as u64 + 1,
                    order: Order::Train {
                        entity: EntityId(id),
                        unit_type: UnitTypeId(3),
                    },
                })
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(outcomes[0].rejection, None);
    assert_eq!(outcomes[1].rejection, Some(Rejection::InsufficientSupply));
    assert_eq!(
        world.resource_balance(PlayerId(0), "minerals"),
        balance - 10
    );
    assert_eq!(world.state().entities[1].unit_type, UnitTypeId(1));
    assert!(world.state().entities[1].production.is_empty());
}

#[test]
fn started_transformation_hatches_despite_supply_loss_and_surrounding_mobile_units() {
    let mut parent = unit(1);
    parent.transforms_on_production = true;
    parent.production_form = Some(UnitTypeId(2));
    parent.trains = vec![UnitTypeId(3)];
    let mut child = unit(3);
    child.speed = 4;
    child.footprint = Footprint {
        width: 32,
        height: 32,
    };
    child.production_count = 2;
    child.supply_used = 1;
    child.build_ticks = 3;
    let mut provider = unit(4);
    provider.supply_provided = 2;
    provider.max_hp = 1;
    let mut attacker = unit(5);
    attacker.weapon = Some(Weapon {
        range: 10,
        damage: 1,
        cooldown: 100,
        targets_air: false,
        damage_kind: DamageKind::Normal,
        cooldown_jitter: None,
        splash: None,
        strikes: vec![],
    });
    let mut traffic = unit(6);
    // Larva are immobile, but are still units rather than solid structures.
    traffic.speed = 0;
    let mut world = definitions(
        vec![parent, unit(2), child, provider, attacker, traffic],
        vec![
            spawn(0, 1, 64),
            spawn(0, 4, 240),
            spawn(1, 5, 248),
            spawn(0, 6, 80),
            spawn(0, 6, 48),
        ],
    );
    issue(
        &mut world,
        Order::Train {
            entity: EntityId(1),
            unit_type: UnitTypeId(3),
        },
    );
    assert_eq!(world.supply(PlayerId(0)), (2, 0));
    step(&mut world, 4);
    let first = world
        .state()
        .entities
        .iter()
        .find(|e| e.id == EntityId(1))
        .unwrap();
    assert_eq!(first.unit_type, UnitTypeId(3));
    assert_eq!(first.position.x, 64);
    assert!(first.production.is_empty());
    assert_eq!(
        world
            .state()
            .entities
            .iter()
            .filter(|e| e.unit_type == UnitTypeId(3))
            .count(),
        2
    );
}

#[test]
fn configured_intermediate_is_destroyed_on_cancel_without_restoring_the_producer() {
    let mut parent = unit(1);
    parent.transforms_on_production = true;
    parent.production_form = Some(UnitTypeId(2));
    parent.trains = vec![UnitTypeId(3)];
    parent.supply_provided = 2;
    let mut form = unit(2);
    form.destroyed_on_production_cancel = true;
    let mut child = unit(3);
    child.build_ticks = 10;
    let mut world = definitions(vec![parent, form, child], vec![spawn(0, 1, 64)]);
    issue(
        &mut world,
        Order::Train {
            entity: EntityId(1),
            unit_type: UnitTypeId(3),
        },
    );
    issue(
        &mut world,
        Order::Cancel {
            entity: EntityId(1),
        },
    );
    assert!(world.state().entities.is_empty());
    assert_eq!(world.state().deaths[&PlayerId(0)][&UnitTypeId(2)], 1);
}

#[test]
fn power_placement_requires_an_owned_completed_provider_but_preserves_exempt_buildings() {
    let mut worker = unit(1);
    worker.speed = 4;
    worker.builds = vec![UnitTypeId(3), UnitTypeId(4)];
    worker.worker = Some(WorkerStats {
        capacity: 8,
        harvest_amount: 8,
        harvest_ticks: 2,
        build_rate: 1,
        resource_kinds: vec!["ore".into()],
    });
    let mut field = unit(2);
    field.structure = true;
    field.power_field = Some(PowerField {
        cell_size: 32,
        rows: vec![3; 2],
    });
    let mut building = unit(3);
    building.structure = true;
    building.requires_power = true;
    let mut exempt = building.clone();
    exempt.id = UnitTypeId(4);
    exempt.requires_power = false;
    let mut world = definitions(
        vec![worker, field, building, exempt],
        vec![spawn(0, 1, 64), spawn(0, 2, 128), spawn(1, 2, 400)],
    );
    let check = |world: &World, kind, x| {
        world.build_rejection(
            PlayerId(0),
            EntityId(1),
            UnitTypeId(kind),
            Position { x, y: 64 },
        )
    };
    assert_eq!(check(&world, 3, 160), None);
    assert_eq!(check(&world, 3, 384), Some(Rejection::InvalidPlacement));
    assert_eq!(check(&world, 4, 384), None);
    let position = Position { x: 160, y: 64 };
    let command = Command {
        tick: world.tick(),
        player: PlayerId(0),
        sequence: 1,
        order: Order::Build {
            entity: EntityId(1),
            unit_type: UnitTypeId(3),
            position,
        },
    };
    // Server validation accepts the same position as the filtered client preview.
    let client = world
        .player_view(PlayerId(0))
        .unwrap()
        .into_world(&world)
        .unwrap();
    assert_eq!(check(&client, 3, 160), None);
    assert_eq!(world.step(&[command]).unwrap()[0].rejection, None);
}

#[test]
fn unpowered_production_pauses_until_an_owned_field_is_present() {
    let mut field = unit(1);
    field.structure = true;
    field.power_field = Some(PowerField {
        cell_size: 32,
        rows: vec![255; 5],
    });
    let mut producer = unit(2);
    producer.structure = true;
    producer.requires_power = true;
    producer.trains = vec![UnitTypeId(3)];
    let mut product = unit(3);
    product.build_ticks = 1;
    let mut powered = definitions(
        vec![field.clone(), producer.clone(), product.clone()],
        vec![spawn(0, 1, 64), spawn(0, 2, 128)],
    );
    assert!(powered.powered(&powered.state().entities[1]));
    issue(
        &mut powered,
        Order::Train {
            entity: EntityId(2),
            unit_type: UnitTypeId(3),
        },
    );
    assert_eq!(powered.state().entities.len(), 3);
    let hostile = definitions(
        vec![field, producer, product],
        vec![spawn(1, 1, 64), spawn(0, 2, 128)],
    );
    assert!(!hostile.powered(&hostile.state().entities[1]));
}

#[test]
fn portable_item_follows_the_worker_and_keeps_its_trigger_identity() {
    let mut worker = unit(1);
    worker.speed = 8;
    worker.worker = Some(WorkerStats {
        capacity: 8,
        harvest_amount: 8,
        harvest_ticks: 2,
        build_rate: 1,
        resource_kinds: vec!["ore".into()],
    });
    let mut item = unit(2);
    item.portable = true;
    item.blocks_movement = false;
    let mut world = definitions(vec![worker, item], vec![spawn(0, 1, 64), spawn(1, 2, 80)]);
    step(&mut world, 1);
    assert_eq!(world.state().entities[1].carried_by, Some(EntityId(1)));
    assert_eq!(world.state().entities[1].owner, PlayerId(0));
    issue(
        &mut world,
        Order::Move {
            entity: EntityId(1),
            target: Position { x: 200, y: 64 },
        },
    );
    step(&mut world, 30);
    assert_eq!(
        world.state().entities[1].position,
        world.state().entities[0].position
    );
    assert_eq!(world.state().entities[1].id, EntityId(2));
}

#[test]
fn independent_construction_releases_the_worker_and_consuming_construction_morphs_it() {
    for consumes in [false, true] {
        let mut worker = unit(1);
        worker.speed = 8;
        worker.builds = vec![UnitTypeId(2)];
        worker.worker = Some(WorkerStats {
            capacity: 8,
            harvest_amount: 8,
            harvest_ticks: 2,
            build_rate: 1,
            resource_kinds: vec!["ore".into()],
        });
        let mut building = unit(2);
        building.structure = true;
        building.build_ticks = 12;
        building.autonomous_construction = !consumes;
        building.consumes_builder = consumes;
        let mut world = definitions(vec![worker, building], vec![spawn(0, 1, 64)]);
        issue(
            &mut world,
            Order::Build {
                entity: EntityId(1),
                unit_type: UnitTypeId(2),
                position: Position { x: 240, y: 64 },
            },
        );
        step(&mut world, 8);
        let pending = world
            .state()
            .entities
            .iter()
            .find(|e| e.unit_type == UnitTypeId(2))
            .unwrap();
        assert!(world.construction_pending(pending));
        assert!(!world.entity_visible(PlayerId(0), pending.id));
        assert!(!world.entity_visible(PlayerId(1), pending.id));
        assert!(
            world
                .player_view(PlayerId(1))
                .unwrap()
                .entities
                .iter()
                .all(|e| match e {
                    ViewedEntity::Owned(e) => e.id != pending.id,
                    ViewedEntity::Visible(e) => e.id != pending.id,
                })
        );
        assert_eq!(pending.construction.as_ref().unwrap().remaining, 12);
        step(&mut world, 14);
        let foundation = world
            .state()
            .entities
            .iter()
            .find(|entity| entity.unit_type == UnitTypeId(2))
            .unwrap();
        assert!(foundation.construction.as_ref().unwrap().worker.is_none());
        if consumes {
            assert_eq!(foundation.id, EntityId(1));
            assert_eq!(world.state().entities.len(), 1);
        } else {
            issue(
                &mut world,
                Order::Move {
                    entity: EntityId(1),
                    target: Position { x: 200, y: 64 },
                },
            );
        }
        let snapshot = world.save_snapshot().unwrap();
        let mut restored = world.restore_snapshot(snapshot).unwrap();
        step(&mut world, 30);
        step(&mut restored, 30);
        assert_eq!(world.state_hash(), restored.state_hash());
        let completed = world
            .state()
            .entities
            .iter()
            .find(|entity| entity.unit_type == UnitTypeId(2))
            .unwrap();
        assert!(completed.construction.is_none());
        assert_eq!(completed.hp, 100);
        if !consumes {
            assert_eq!(world.state().entities[0].position.x, 200);
        }
    }
}
