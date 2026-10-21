use super::*;

#[test]
#[ignore = "requires STRATERUST_GAME_ROOT; native eight-player overloaded-host delivery"]
fn retail_overloaded_host_yields_to_updates() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(std::env::var_os("STRATERUST_GAME_ROOT").unwrap());
    let catalog = Catalog::load(&root)?;
    let map_index = catalog
        .maps
        .iter()
        .position(|m| m.package.contains("bac6eef937ed93cf"))
        .unwrap();
    let mut settings = catalog.defaults(map_index);
    settings.random_locations = false;
    for (index, slot) in settings.slots.iter_mut().enumerate() {
        slot.kind = if index == 0 {
            SlotKind::Open
        } else {
            SlotKind::Computer
        };
        slot.faction = Some(index % 2);
    }
    let world = catalog
        .prepare(&root, map_index, &settings, &[0], 42)?
        .world;
    let definitions = world.snapshot();
    let map = net::MapTransfer::of(&world)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    // Deliberately ask for more work than the native eight-player world can
    // sustain. Delivery must yield between expensive steps instead of waiting
    // behind a permanent eight-tick catch-up batch.
    let thread = std::thread::spawn(move || {
        net::run_host_configured(
            listener,
            world,
            42,
            stopped,
            None,
            map,
            vec![PlayerId(0)],
            1,
            None,
        )
    });
    let result = (|| -> anyhow::Result<Vec<u64>> {
        let (mut client, initial) = net::RemoteClient::connect(address, &definitions, PlayerId(0))?;
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut previous = initial.sequence;
        let mut gaps = Vec::new();
        while gaps.len() < 100 && Instant::now() < deadline {
            if let Some(net::ServerMessage::Update(update)) = client.poll()? {
                gaps.push(update.sequence - previous);
                previous = update.sequence;
                client.acknowledge(previous)?;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        client.leave();
        Ok(gaps)
    })();
    stop.store(true, Ordering::Release);
    let _ = thread.join().unwrap();
    let mut gaps = result?;
    assert!(gaps.len() >= 50, "host stopped delivering updates");
    gaps.sort_unstable();
    let median = gaps[gaps.len() / 2];
    eprintln!(
        "Overloaded host: {} updates, median ticks between deliveries={median}",
        gaps.len()
    );
    assert!(
        median <= 4,
        "updates withheld behind catch-up batches: {gaps:?}"
    );
    Ok(())
}
use straterust_engine::session::ServerSession;

#[test]
#[ignore = "measures native snapshot cost; requires STRATERUST_GAME_ROOT"]
fn original_snapshot_delivery_cost() -> anyhow::Result<()> {
    let directory = std::path::PathBuf::from(
        std::env::var_os("STRATERUST_GAME_ROOT").expect("set native game directory"),
    );
    let catalog = Catalog::load(&directory)?;
    let mut indices = vec![0];
    if let Some(i) = catalog.maps.iter().position(|map| map.players == 2) {
        indices.push(i);
    }
    if let Some((i, _)) = catalog
        .maps
        .iter()
        .enumerate()
        .max_by_key(|(_, map)| map.size)
    {
        indices.push(i);
    }
    indices.sort_unstable();
    indices.dedup();
    for index in indices {
        for fog in [true, false] {
            let mut settings = catalog.defaults(index);
            settings.fog = fog;
            settings.random_locations = false;
            for (i, slot) in settings.slots.iter_mut().enumerate() {
                slot.kind = if i < 2 {
                    SlotKind::Open
                } else {
                    SlotKind::Closed
                };
            }
            let prepared = catalog.prepare(&directory, index, &settings, &[0, 1], 42)?;
            let mut session =
                ServerSession::new(prepared.world, 42, vec![PlayerId(0), PlayerId(1)])?;
            session.advance(&[])?;
            let update = session.update(PlayerId(0), &[])?;
            let resources = update.view.resources.len();
            let entities = update.view.entities.len();
            let message = net::ServerMessage::Update(update);
            let start = Instant::now();
            let mut bytes = String::new();
            for _ in 0..20 {
                bytes = ron::ser::to_string(&message)?;
            }
            let encoding = start.elapsed() / 20;
            let start = Instant::now();
            for _ in 0..20 {
                let decoded: net::ServerMessage = ron::de::from_str(&bytes)?;
                std::hint::black_box(decoded);
            }
            let decoding = start.elapsed() / 20;
            let update = match message {
                net::ServerMessage::Update(update) => update,
                _ => unreachable!(),
            };
            let start = Instant::now();
            for _ in 0..20 {
                std::hint::black_box(update.view.clone().into_world(session.world())?);
            }
            let application = start.elapsed() / 20;
            println!(
                "map={:?} fog={fog} entities={entities} visible_resources={resources} bytes={} tick_ms={} encode={encoding:?} decode={decoding:?} apply={application:?}",
                catalog.maps[index].title,
                bytes.len(),
                session.world().rules().tick_ms
            );
            let full_wire = net::encoded_size(&net::ServerMessage::Update(update.clone()))?;
            let mut sender = net::replication::Sender::new(update.clone());
            session.advance(&[])?;
            let quiet = session.update(PlayerId(0), &[])?;
            sender.observe(quiet.clone())?;
            let packet = sender.packet().unwrap();
            let changed_resources = packet.resources.len();
            let quiet_message = net::ServerMessage::Delta(packet);
            let quiet_ron = ron::ser::to_string(&quiet_message)?.len();
            let start = Instant::now();
            let mut quiet_wire = 0;
            for _ in 0..20 {
                quiet_wire = net::encoded_size(&quiet_message)?;
            }
            let delta_encoding = start.elapsed() / 20;
            sender.acknowledge(quiet.sequence)?;
            let mut harvest = quiet.clone();
            harvest.sequence += 1;
            harvest.view.tick.0 += 1;
            if let Some(resource) = harvest.view.resources.first_mut() {
                resource.amount = resource.amount.saturating_sub(1);
            }
            sender.observe(harvest)?;
            let harvest_wire =
                net::encoded_size(&net::ServerMessage::Delta(sender.packet().unwrap()))?;
            println!(
                "full_wire={full_wire} quiet_delta_ron={quiet_ron} quiet_wire={quiet_wire} quiet_changed_resources={changed_resources} one_resource_wire={harvest_wire} delta_encode={delta_encoding:?}"
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "profiles sustained native mining; requires STRATERUST_GAME_ROOT"]
fn mining_delivery_cost() -> anyhow::Result<()> {
    let directory = std::path::PathBuf::from(
        std::env::var_os("STRATERUST_GAME_ROOT").expect("set native game directory"),
    );
    let catalog = Catalog::load(&directory)?;
    let index = catalog
        .maps
        .iter()
        .position(|map| map.players == 2)
        .unwrap();
    for speed_name in ["Normal", "Fastest"] {
        let speed_index = catalog
            .speeds
            .iter()
            .position(|s| s.name == speed_name)
            .unwrap();
        let tick_ms = catalog.speeds[speed_index].tick_ms;
        for fog in [true, false] {
            for ack_lag in [0, 3] {
                let mut settings = catalog.defaults(index);
                settings.fog = fog;
                settings.speed = speed_index;
                settings.map_units = true;
                settings.random_locations = false;
                for (i, slot) in settings.slots.iter_mut().enumerate() {
                    slot.kind = if i < 2 {
                        SlotKind::Open
                    } else {
                        SlotKind::Closed
                    };
                    slot.faction = Some(0);
                }
                let prepared = catalog.prepare(&directory, index, &settings, &[0, 1], 42)?;
                // Ordinary Warcraft II starts contain only a worker. Add a completed
                // dropoff so this measures repeated mining trips, not one load then idle.
                let mut map = prepared.world.map().clone();
                let start = map
                    .spawns
                    .iter()
                    .find(|s| s.owner == PlayerId(1))
                    .unwrap()
                    .position;
                let hall = prepared
                    .world
                    .rules()
                    .units
                    .iter()
                    .find(|u| u.structure && u.dropoff.iter().any(|kind| kind == "gold"))
                    .expect("gold dropoff")
                    .id;
                map.spawns.push(Spawn {
                    owner: PlayerId(1),
                    unit_type: hall,
                    position: Position {
                        x: start.x - 160,
                        y: start.y,
                    },
                    ..Default::default()
                });
                let world = World::new(prepared.world.rules().clone(), map, 42)?;
                let mut session = ServerSession::new(world, 42, vec![PlayerId(0), PlayerId(1)])?;
                let world = session.world();
                let worker = world
                    .state()
                    .entities
                    .iter()
                    .find(|entity| {
                        entity.owner == PlayerId(1)
                            && world.unit_type(entity.unit_type).unwrap().worker.is_some()
                    })
                    .expect("starting worker");
                let mine = world
                    .state()
                    .resources
                    .iter()
                    .filter(|node| node.kind == "gold")
                    .min_by_key(|node| {
                        let dx = i64::from(node.position.x - worker.position.x);
                        let dy = i64::from(node.position.y - worker.position.y);
                        dx * dx + dy * dy
                    })
                    .expect("gold mine");
                let (worker_id, mine_id, original_gold) = (worker.id, mine.id, mine.amount);
                let command = Command {
                    tick: world.tick(),
                    player: PlayerId(1),
                    sequence: 1,
                    order: Order::Gather {
                        entity: worker_id,
                        resource: mine_id,
                    },
                };
                let initial = session.update(PlayerId(1), &[])?;
                let definitions = initial.view.clone().into_world(session.world())?;
                let mut sender = net::replication::Sender::new(initial.clone());
                let mut receiver = net::replication::Receiver::new(&initial);
                let mut next_send = net::clock::UPDATE_MS;
                let mut packets = 0;
                let mut wire = 0;
                let mut raw = 0;
                let mut records = BTreeMap::<EntityId, usize>::new();
                let mut resource_records = 0;
                let mut header_frames = 0;
                let mut maximum = 0;
                let mut maximum_packet = String::new();
                let mut projection = Duration::ZERO;
                let mut reconstruction = Duration::ZERO;
                let mut generation = Duration::ZERO;
                const TICKS: u64 = 1000;
                for tick in 0..TICKS {
                    let start = Instant::now();
                    let outcomes = session.advance(if tick == 0 {
                        std::slice::from_ref(&command)
                    } else {
                        &[]
                    })?;
                    assert!(
                        outcomes.iter().all(|outcome| outcome.rejection.is_none()),
                        "{outcomes:?}"
                    );
                    let mut update = session.update(PlayerId(1), &outcomes)?;
                    update.time = Some(net::clock::FrameTime {
                        captured_ms: (tick + 1) * u64::from(tick_ms),
                        tick_ms,
                    });
                    sender.observe(update)?;
                    generation += start.elapsed();
                    let elapsed = (tick + 1) * u64::from(tick_ms);
                    if elapsed < next_send {
                        continue;
                    }
                    next_send += net::clock::UPDATE_MS;
                    packets += 1;
                    let packet = sender.packet().unwrap();
                    for (id, _) in &packet.entities {
                        *records.entry(*id).or_default() += 1;
                    }
                    resource_records += packet.resources.len();
                    let message = net::ServerMessage::Delta(packet.clone());
                    let ron = ron::ser::to_string(&message)?;
                    header_frames += usize::from(!ron.contains("header:None"));
                    raw += ron.len();
                    let bytes = net::encoded_size(&message)?;
                    wire += bytes;
                    if bytes > maximum {
                        maximum = bytes;
                        maximum_packet = ron;
                    }
                    let start = Instant::now();
                    let update = receiver.apply(packet)?.unwrap();
                    reconstruction += start.elapsed();
                    let start = Instant::now();
                    std::hint::black_box(update.view.into_world(&definitions)?);
                    projection += start.elapsed();
                    if update.sequence > ack_lag {
                        sender.acknowledge(update.sequence - ack_lag)?;
                    }
                }
                let remaining = session
                    .world()
                    .state()
                    .resources
                    .iter()
                    .find(|node| node.id == mine_id)
                    .unwrap()
                    .amount;
                assert!(
                    original_gold - remaining > 100,
                    "worker must complete repeated mining trips"
                );
                let names: Vec<_> = records
                    .into_iter()
                    .map(|(id, count)| {
                        let name = session
                            .world()
                            .state()
                            .entities
                            .iter()
                            .find(|e| e.id == id)
                            .map(|e| e.unit_type);
                        (id, name, count)
                    })
                    .collect();
                println!(
                    "mining map={:?} fog={fog} ack_lag={ack_lag} wire={wire} raw={raw} max_packet={maximum} header_frames={header_frames} resource_records={resource_records} gold_mined={} changed_entities={names:?} generate_per_tick={:?} reconstruct_per_tick={:?} project_per_tick={:?}",
                    catalog.maps[index].title,
                    original_gold - remaining,
                    generation / TICKS as u32,
                    reconstruction / TICKS as u32,
                    projection / TICKS as u32
                );
                println!(
                    "speed={speed_name} packets={packets} down_Bps={:.0}",
                    wire as f64 * 1000. / (TICKS as f64 * f64::from(tick_ms))
                );
                std::fs::write("/tmp/stratarust-network-packet.ron", maximum_packet)?;
            }
        }
    }
    Ok(())
}
