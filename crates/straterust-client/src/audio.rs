//! Persistent music/voice/effects mixer. Simulation never observes its clock/device.
use rodio::{OutputStream, OutputStreamBuilder, Sink, Source};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};
pub use straterust_engine::media::AudioCue as Cue;
use straterust_engine::{
    media::{AudioClips, MediaPack, PcmClip},
    sim::{Entity, EntityId, PlayerId, Position, UnitOrder, UnitTypeId, World},
};

const MAX_EFFECTS: usize = 16;
struct Mixer {
    music: Sink,
    voice: Sink,
    mission: Sink,
    effects: Vec<Sink>,
    input: rodio::mixer::Mixer,
    music_gain: f32,
    sound_gain: f32,
    speech_gain: f32,
    // Keep the device alive; None is used only by the offline mixer tests.
    _stream: Option<OutputStream>,
}
impl Mixer {
    fn open() -> Option<Self> {
        match OutputStreamBuilder::open_default_stream() {
            Ok(mut stream) => {
                stream.log_on_drop(false);
                let music = Sink::connect_new(stream.mixer());
                music.set_volume(0.28);
                let voice = Sink::connect_new(stream.mixer());
                voice.set_volume(0.85);
                let mission = Sink::connect_new(stream.mixer());
                mission.set_volume(0.95);
                Some(Self {
                    music,
                    voice,
                    mission,
                    effects: Vec::new(),
                    input: stream.mixer().clone(),
                    music_gain: 1.0,
                    sound_gain: 1.0,
                    speech_gain: 1.0,
                    _stream: Some(stream),
                })
            }
            Err(error) => {
                log::warn!("optional audio output unavailable: {error}");
                None
            }
        }
    }
    fn clear_voice(&mut self) {
        // Sink::clear waits for the audio thread. Dropping/replacing a sink is
        // nonblocking, so an acknowledgement cannot stall rendering.
        self.voice = Sink::connect_new(&self.input);
        self.voice.set_volume(0.85 * self.speech_gain);
    }
    fn clear_mission(&mut self) {
        self.mission = Sink::connect_new(&self.input);
        self.mission.set_volume(0.95 * self.speech_gain);
    }
    fn clear_music(&mut self) {
        self.music = Sink::connect_new(&self.input);
        self.music.set_volume(0.28 * self.music_gain);
    }
}

#[derive(Clone)]
struct PcmSource {
    clip: Arc<PcmClip>,
    cursor: usize,
}
impl Iterator for PcmSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let value = self.clip.samples.get(self.cursor)?;
        self.cursor += 1;
        Some(f32::from(*value) / 32768.0)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.clip.samples.len() - self.cursor;
        (remaining, Some(remaining))
    }
}
impl Source for PcmSource {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.clip.samples.len() - self.cursor)
    }
    fn channels(&self) -> u16 {
        self.clip.channels
    }
    fn sample_rate(&self) -> u32 {
        self.clip.sample_rate
    }
    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_millis(self.clip.duration_ms()))
    }
}

#[derive(Clone, Copy)]
struct Observed {
    research: Option<straterust_engine::sim::ResearchId>,
    changing_mode: bool,
    cast: Option<(
        straterust_engine::sim::AbilityId,
        straterust_engine::sim::Tick,
    )>,
    owner: PlayerId,
    unit_type: UnitTypeId,
    position: Position,
    cooldown: u32,
    hp: u32,
    repair_progress: u64,
    repair_target: Option<EntityId>,
    cargo: u32,
    construction: Option<(u32, Option<EntityId>)>,
    mine_phase: Option<straterust_engine::sim::MinePhase>,
    garrisoned_in: Option<EntityId>,
    airborne: bool,
    cloaked: bool,
    flight_transition: u32,
}
impl Observed {
    fn from(world: &World, entity: &Entity) -> Self {
        Self {
            research: entity.research.as_ref().map(|job| job.id),
            cast: entity.last_cast.as_ref().map(|c| (c.ability, c.tick)),
            changing_mode: entity.mode_transition.is_some(),
            owner: entity.owner,
            unit_type: entity.unit_type,
            garrisoned_in: entity.garrisoned_in,
            airborne: entity.airborne,
            cloaked: entity.cloaked,
            flight_transition: entity.flight_transition,
            mine_phase: entity.mine_state.as_ref().map(|state| state.phase),
            position: entity.position,
            cooldown: entity.cooldown,
            hp: entity.hp,
            repair_progress: entity.repair_progress,
            repair_target: if let UnitOrder::Repair { target } = entity.order {
                Some(target)
            } else {
                None
            },
            cargo: entity.cargo.as_ref().map_or(0, |cargo| cargo.amount),
            construction: entity
                .construction
                .as_ref()
                .filter(|_| !world.construction_pending(entity))
                .map(|job| (job.remaining, job.worker)),
        }
    }
}

pub struct Audio {
    mixer: Option<Mixer>,
    clips: Vec<AudioClips>,
    music: Vec<Arc<PcmClip>>,
    mission_audio: Vec<Arc<PcmClip>>,
    mission_speaker: Option<UnitTypeId>,
    speech_muted: bool,
    music_index: usize,
    variants: BTreeMap<(Cue, Option<UnitTypeId>), usize>,
    last_event: BTreeMap<(Cue, Option<UnitTypeId>), Instant>,
    voice_type: Option<UnitTypeId>,
    voice_priority: u8,
    voice_started: Option<Instant>,
    pending_voice: Option<(Cue, Option<UnitTypeId>, Instant)>,
    previous: BTreeMap<EntityId, Observed>,
    previous_scans: Vec<straterust_engine::sim::Scan>,
    strike_events: std::collections::BTreeSet<(
        straterust_engine::sim::AbilityId,
        straterust_engine::sim::Tick,
        Option<straterust_engine::sim::StrikeStage>,
    )>,
    tick: u64,
    finished: bool,
    #[cfg(test)]
    pub(crate) events: Vec<(Cue, Option<UnitTypeId>)>,
}
impl Audio {
    pub fn new(enabled: bool) -> Self {
        Self {
            mixer: if enabled && !cfg!(test) {
                Mixer::open()
            } else {
                None
            },
            clips: Vec::new(),
            music: Vec::new(),
            mission_audio: Vec::new(),
            mission_speaker: None,
            speech_muted: false,
            music_index: 0,
            variants: BTreeMap::new(),
            last_event: BTreeMap::new(),
            voice_type: None,
            voice_priority: 0,
            voice_started: None,
            pending_voice: None,
            previous: BTreeMap::new(),
            previous_scans: Vec::new(),
            strike_events: Default::default(),
            tick: 0,
            finished: false,
            #[cfg(test)]
            events: Vec::new(),
        }
    }
    pub fn configure(&mut self, enabled: bool, music: u8, sound: u8, speech: u8) {
        if enabled && self.mixer.is_none() && !cfg!(test) {
            self.mixer = Mixer::open();
        }
        if let Some(mixer) = &mut self.mixer {
            let gain = |value: u8| {
                if enabled {
                    f32::from(value) / 100.0
                } else {
                    0.0
                }
            };
            mixer.music_gain = gain(music);
            mixer.sound_gain = gain(sound);
            mixer.speech_gain = gain(speech);
            mixer.music.set_volume(0.28 * mixer.music_gain);
            mixer.voice.set_volume(0.85 * mixer.speech_gain);
            mixer.mission.set_volume(0.95 * mixer.speech_gain);
            for effect in &mixer.effects {
                effect.set_volume(0.45 * mixer.sound_gain);
            }
        }
    }
    pub fn set_media(&mut self, media: Option<&MediaPack>) {
        if let Some(mixer) = &mut self.mixer {
            mixer.clear_voice();
            mixer.clear_mission();
            mixer.effects.clear();
        }
        self.clips = media.map_or_else(Vec::new, |media| media.audio.clone());
        self.mission_audio = media.map_or_else(Vec::new, |media| media.mission_audio.clone());
        self.mission_speaker = None;
        self.speech_muted = false;
        self.variants.clear();
        self.last_event.clear();
        self.voice_type = None;
        self.pending_voice = None;
        self.set_music(media.map_or(&[], |media| media.music.as_slice()));
    }
    /// Change a presentation playlist without restarting it during navigation.
    pub fn set_music(&mut self, music: &[Arc<PcmClip>]) {
        let same_music = self.music.len() == music.len()
            && self.music.iter().zip(music).all(|(old, new)| {
                old.channels == new.channels
                    && old.sample_rate == new.sample_rate
                    && old.samples == new.samples
            });
        if !same_music {
            if let Some(mixer) = &mut self.mixer {
                mixer.clear_music();
            }
            self.music = music.to_vec();
            self.music_index = 0;
        }
        self.update();
    }
    pub fn shutdown(&mut self) {
        self.mixer = None;
    }
    pub fn forget_entity(&mut self, entity: EntityId) {
        self.previous.remove(&entity);
    }
    pub fn is_speaking(&self, unit_type: UnitTypeId) -> bool {
        (self.mission_speaker == Some(unit_type)
            && self
                .mixer
                .as_ref()
                .is_some_and(|mixer| !mixer.mission.empty()))
            || (self.voice_type == Some(unit_type)
                && self
                    .mixer
                    .as_ref()
                    .is_some_and(|mixer| !mixer.voice.empty()))
    }
    pub fn play_mission(&mut self, sound: u16, speaker: Option<UnitTypeId>) {
        let Some(clip) = self.mission_audio.get(usize::from(sound)).cloned() else {
            return;
        };
        if let Some(mixer) = &mut self.mixer {
            if speaker.is_none() {
                mixer.effects.retain(|effect| !effect.empty());
                if mixer.effects.len() < MAX_EFFECTS {
                    let effect = Sink::connect_new(&mixer.input);
                    effect.set_volume(0.8 * mixer.sound_gain);
                    effect.append(PcmSource { clip, cursor: 0 });
                    mixer.effects.push(effect);
                }
                return;
            }
            self.mission_speaker = speaker;
            self.pending_voice = None;
            mixer.clear_voice();
            mixer.clear_mission();
            mixer.mission.append(PcmSource { clip, cursor: 0 });
        }
    }
    pub fn mute_unit_speech(&mut self, muted: bool) {
        self.speech_muted = muted;
        if muted {
            self.pending_voice = None;
            if let Some(mixer) = &mut self.mixer {
                mixer.clear_voice();
            }
        }
    }
    pub fn stop_mission(&mut self) {
        self.mission_speaker = None;
        self.speech_muted = false;
        if let Some(mixer) = &mut self.mixer {
            mixer.clear_mission();
        }
    }
    pub fn update(&mut self) {
        let Some(mixer) = &mut self.mixer else {
            return;
        };
        mixer.effects.retain(|effect| !effect.empty());
        if mixer.music.empty() && !self.music.is_empty() {
            mixer.music.append(PcmSource {
                clip: Arc::clone(&self.music[self.music_index]),
                cursor: 0,
            });
            self.music_index = (self.music_index + 1) % self.music.len();
        }
        if mixer.voice.empty() {
            self.voice_type = None;
            if let Some((cue, unit_type, queued)) = self.pending_voice.take()
                && queued.elapsed() <= Duration::from_secs(3)
            {
                self.event(cue, unit_type);
            }
        }
    }
    pub fn event(&mut self, cue: Cue, unit_type: Option<UnitTypeId>) {
        #[cfg(test)]
        {
            if self.events.len() == 256 {
                self.events.remove(0);
            }
            self.events.push((cue, unit_type));
        }
        let Some(mixer) = &mut self.mixer else {
            return;
        };
        let entry = self
            .clips
            .iter()
            .find(|entry| entry.cue == cue && entry.unit_type == unit_type)
            .or_else(|| {
                self.clips
                    .iter()
                    .find(|entry| entry.cue == cue && entry.unit_type.is_none())
            });
        let entry = entry.or_else(|| {
            if matches!(
                cue,
                Cue::AttackAir | Cue::ImpactAir | Cue::SelectConstruction
            ) {
                self.clips.iter().find(|entry| {
                    entry.cue
                        == match cue {
                            Cue::AttackAir => Cue::Attack,
                            Cue::ImpactAir => Cue::Impact,
                            _ => Cue::Select,
                        }
                        && entry.unit_type == unit_type
                })
            } else {
                None
            }
        });
        let voice = entry.is_some_and(|entry| entry.voice);
        if voice && (self.speech_muted || !mixer.mission.empty()) {
            return;
        }
        let now = Instant::now();
        let key = (cue, unit_type);
        let interval = match cue {
            Cue::Work => 700,
            Cue::Attack | Cue::AttackAir | Cue::Impact | Cue::ImpactAir => 40,
            Cue::Death => 30,
            _ => 120,
        };
        if self
            .last_event
            .get(&key)
            .is_some_and(|last| now.duration_since(*last) < Duration::from_millis(interval))
        {
            return;
        }
        let priority = if matches!(cue, Cue::Select | Cue::SelectConstruction | Cue::Order) {
            2
        } else {
            1
        };
        if voice && !mixer.voice.empty() {
            if priority < self.voice_priority || matches!(cue, Cue::ResearchComplete(_)) {
                self.pending_voice = Some((cue, unit_type, now));
                return;
            }
            if self
                .voice_started
                .is_some_and(|last| now.duration_since(last) < Duration::from_millis(120))
            {
                return;
            }
        }
        let clip = if let Some(entry) = entry {
            let next = self
                .variants
                .entry((entry.cue, entry.unit_type))
                .or_default();
            let clip = Arc::clone(&entry.variants[*next % entry.variants.len()]);
            *next = (*next + 1) % entry.variants.len();
            clip
        } else if matches!(
            cue,
            Cue::Select
                | Cue::SelectConstruction
                | Cue::Order
                | Cue::Error
                | Cue::Complete
                | Cue::Ready
        ) {
            Arc::new(tone(cue))
        } else {
            return;
        };
        self.last_event.insert(key, now);
        if voice {
            mixer.clear_voice();
            mixer.voice.append(PcmSource { clip, cursor: 0 });
            self.voice_type = unit_type;
            self.voice_priority = priority;
            self.voice_started = Some(now);
        } else {
            mixer.effects.retain(|effect| !effect.empty());
            if mixer.effects.len() >= MAX_EFFECTS {
                if matches!(cue, Cue::AbilityWarning(_)) {
                    mixer.effects.remove(0).stop();
                } else {
                    return;
                }
            }
            let sink = Sink::connect_new(&mixer.input);
            sink.set_volume(mixer.sound_gain * if cue == Cue::Work { 0.22 } else { 0.45 });
            sink.append(PcmSource { clip, cursor: 0 });
            mixer.effects.push(sink);
        }
    }
    /// Use the same flight clock as the visible missile, including skipped frames.
    pub fn projectile_impacts(
        &mut self,
        visuals: &crate::visual::Visuals,
        assets: Option<&straterust_engine::assets::AssetPack>,
        elapsed: Duration,
    ) {
        let Some(assets) = assets else { return };
        for shot in visuals.projectiles() {
            if assets
                .projectile_for(shot.unit_type, shot.targets_air)
                .is_some_and(|effect| shot.impacts_within(effect, elapsed))
            {
                self.event(
                    if shot.targets_air {
                        Cue::ImpactAir
                    } else {
                        Cue::Impact
                    },
                    Some(shot.unit_type),
                );
            }
        }
    }

    pub fn reset(&mut self, world: &World) {
        self.stop_mission();
        self.previous = world
            .state()
            .entities
            .iter()
            .map(|entity| (entity.id, Observed::from(world, entity)))
            .collect();
        self.strike_events.clear();
        self.previous_scans = world.state().scans.clone();
        self.tick = world.tick().0;
        self.finished = world.state().winner.is_some();
        self.pending_voice = None;
        self.voice_type = None;
        if let Some(mixer) = &mut self.mixer {
            mixer.clear_voice();
            mixer.effects.clear();
        }
    }
    /// Observe each completed tick once, including catch-up ticks. Cues follow
    /// actual work/combat/completion transitions, never rendering frame count.
    pub fn observe(&mut self, world: &World) {
        #[cfg(not(test))]
        if self.mixer.is_none() {
            return;
        }
        if world.tick().0 <= self.tick {
            return;
        }
        if self.finished {
            self.tick = world.tick().0;
            return;
        }
        let current: BTreeMap<_, _> = world
            .state()
            .entities
            .iter()
            .map(|entity| (entity.id, Observed::from(world, entity)))
            .collect();
        let mut events = Vec::new();
        for strike in world.strike_appearances(world.view_player()) {
            if strike.warning
                && self
                    .strike_events
                    .insert((strike.ability, strike.started, None))
            {
                events.push((Cue::AbilityWarning(strike.ability), None));
            }
            if let Some(stage) = strike.stage
                && self
                    .strike_events
                    .insert((strike.ability, strike.started, Some(stage)))
            {
                events.push((Cue::StrikeStage(strike.ability, stage), None));
            }
        }
        // Retain event keys across brief occlusion without unbounded growth.
        self.strike_events
            .retain(|(_, started, _)| world.tick().0.saturating_sub(started.0) < 100000);
        let mut matched_scans = vec![false; self.previous_scans.len()];
        for scan in &world.state().scans {
            if let Some(index) = self
                .previous_scans
                .iter()
                .enumerate()
                .position(|(index, old)| {
                    !matched_scans[index]
                        && old.owner == scan.owner
                        && old.position == scan.position
                        && old.radius == scan.radius
                        && (old.remaining == scan.remaining
                            || old.remaining == scan.remaining.saturating_add(1))
                })
            {
                matched_scans[index] = true;
            } else if scan.owner == world.view_player() {
                let unit = world.rules().units.iter().find(|unit| {
                    unit.scanner
                        .as_ref()
                        .is_some_and(|scanner| scanner.radius == scan.radius)
                });
                events.push((Cue::Scan, unit.map(|unit| unit.id)));
            }
        }
        let paused = world
            .state()
            .mission
            .as_ref()
            .is_some_and(|mission| mission.paused);
        for entity in &world.state().entities {
            let audible = world.entity_visible(world.view_player(), entity.id)
                || entity
                    .garrisoned_in
                    .is_some_and(|id| world.entity_visible(world.view_player(), id));
            if !audible {
                continue;
            }
            let Some(old) = self.previous.get(&entity.id) else {
                if entity.construction.is_some() {
                    events.push((Cue::Transform, Some(entity.unit_type)));
                }
                if entity.owner == world.view_player()
                    && entity.construction.is_none()
                    && world
                        .unit_type(entity.unit_type)
                        .is_some_and(|unit| unit.mine.is_none())
                {
                    events.push((Cue::Ready, Some(entity.unit_type)));
                }
                continue;
            };
            if !paused
                && entity.owner == world.view_player()
                && old.owner == entity.owner
                && let Some(research) = old.research
                && entity.research.as_ref().map(|job| job.id) != Some(research)
                && world.has_research(entity.owner, research)
            {
                events.push((Cue::ResearchComplete(research), None));
            }
            if !paused
                && entity.last_cast.as_ref().map(|c| (c.ability, c.tick)) != old.cast
                && let Some(cast) = &entity.last_cast
            {
                events.push((Cue::Ability(cast.ability), Some(entity.unit_type)));
            }
            if !paused && entity.mode_transition.is_some() && !old.changing_mode {
                events.push((Cue::ChangeMode, Some(entity.unit_type)));
            }
            if !paused
                && entity.construction.is_some()
                && (old.construction.is_none() || old.unit_type != entity.unit_type)
            {
                events.push((Cue::Transform, Some(entity.unit_type)));
            }
            if !paused
                && entity.owner == world.view_player()
                && old.unit_type != entity.unit_type
                && entity.production.is_empty()
                && entity.construction.is_none()
            {
                events.push((Cue::Ready, Some(entity.unit_type)));
            }
            if entity.owner == world.view_player()
                && old.owner != entity.owner
                && !events.iter().any(|(cue, _)| *cue == Cue::Capture)
            {
                events.push((Cue::Capture, None));
            }
            if !paused && entity.garrisoned_in != old.garrisoned_in {
                if let Some(container) = entity.garrisoned_in.and_then(|id| current.get(&id)) {
                    events.push((Cue::Load, Some(container.unit_type)));
                }
                if let Some(container) = old.garrisoned_in.and_then(|id| current.get(&id)) {
                    // Destruction and failed exits do not count as an unload.
                    events.push((Cue::Unload, Some(container.unit_type)));
                }
            }
            if !paused
                && entity.cloaked != old.cloaked
                && world
                    .unit_type(entity.unit_type)
                    .is_some_and(|unit| unit.cloak.is_some())
            {
                events.push((
                    if entity.cloaked {
                        Cue::Conceal
                    } else {
                        Cue::Reveal
                    },
                    Some(entity.unit_type),
                ));
            }
            if !paused
                && world
                    .unit_type(entity.unit_type)
                    .is_some_and(|unit| unit.flight.is_some())
            {
                if entity.airborne && !old.airborne {
                    events.push((Cue::Lift, Some(entity.unit_type)));
                }
                if old.flight_transition == 0
                    && entity.flight_transition > 0
                    && world.is_landing(entity)
                {
                    events.push((Cue::Land, Some(entity.unit_type)));
                }
            }
            if !paused
                && let Some(state) = &entity.mine_state
                && old.mine_phase != Some(state.phase)
            {
                use straterust_engine::sim::MinePhase;
                match state.phase {
                    MinePhase::Concealing | MinePhase::Emerging => {
                        events.push((Cue::Work, Some(entity.unit_type)))
                    }
                    MinePhase::Chasing => events.push((Cue::Attack, Some(entity.unit_type))),
                    _ => {}
                }
            }
            if entity.owner == world.view_player()
                && entity.construction.is_none()
                && let Some((_, worker)) = old.construction
            {
                let speaker = worker
                    .and_then(|id| self.previous.get(&id))
                    .map(|worker| worker.unit_type);
                events.push((Cue::Complete, speaker.or(Some(entity.unit_type))));
            }
            if !paused
                && entity.cooldown > 0
                && (entity.cooldown > old.cooldown || old.cooldown <= 1)
            {
                events.push((
                    if entity.last_attack_air {
                        Cue::AttackAir
                    } else {
                        Cue::Attack
                    },
                    Some(entity.unit_type),
                ));
            }
            let gathering = matches!(entity.order, UnitOrder::Gather { .. })
                && (entity.harvest_progress > 0
                    || entity.cargo.as_ref().map_or(0, |cargo| cargo.amount) > old.cargo);
            let building = if let UnitOrder::Build { building } = entity.order {
                self.previous
                    .get(&building)
                    .and_then(|old| old.construction)
                    .is_some_and(|(before, _)| {
                        current
                            .get(&building)
                            .is_some_and(|other| other.construction.map_or(0, |job| job.0) < before)
                    })
            } else {
                false
            };
            let repairing = if let UnitOrder::Repair { target } = entity.order {
                self.previous.get(&target).is_some_and(|before| {
                    current
                        .get(&target)
                        .is_some_and(|after| after.hp > before.hp)
                }) || (old.repair_target == Some(target)
                    && entity.repair_progress != old.repair_progress)
            } else {
                false
            };
            if !paused
                && entity.position == old.position
                && (gathering
                    || building
                    || repairing
                    || world
                        .appearance(entity.id)
                        .is_some_and(|a| a.work_heading.is_some()))
            {
                events.push((Cue::Work, Some(entity.unit_type)));
            }
        }
        if !paused {
            for impact in world.public_weapon_feedback().iter().filter(|e| e.impact) {
                events.push((
                    if impact.targets_air {
                        Cue::ImpactAir
                    } else {
                        Cue::Impact
                    },
                    Some(impact.weapon),
                ));
            }
            for impact in world.public_weapon_feedback().iter().filter(|e| !e.impact) {
                events.push((
                    if impact.targets_air {
                        Cue::AttackAir
                    } else {
                        Cue::Attack
                    },
                    Some(impact.weapon),
                ));
            }
            for shot in world.public_shots() {
                if current
                    .get(&shot.container)
                    .is_some_and(|e| e.owner != world.view_player())
                {
                    events.push((
                        if shot.targets_air {
                            Cue::AttackAir
                        } else {
                            Cue::Attack
                        },
                        Some(shot.weapon),
                    ));
                }
            }
        }
        for (id, old) in &self.previous {
            if !current.contains_key(id) && world.disclosed_death(*id) {
                // Notifications are coalesced by type. Do not announce stale
                // readiness after a unit of that type has died.
                if self
                    .pending_voice
                    .is_some_and(|(_, unit_type, _)| unit_type == Some(old.unit_type))
                {
                    self.pending_voice = None;
                }
                if world.visibility(world.view_player(), old.position)
                    == straterust_engine::sim::Visibility::Visible
                    && world
                        .unit_type(old.unit_type)
                        .is_some_and(|unit| !unit.revealer)
                {
                    events.push((Cue::Death, Some(old.unit_type)));
                }
            }
        }
        self.previous = current;
        self.previous_scans = world.state().scans.clone();
        self.tick = world.tick().0;
        self.finished = world.state().winner.is_some();
        for (cue, unit_type) in events {
            self.event(cue, unit_type);
        }
    }
}
fn tone(cue: Cue) -> PcmClip {
    let (frequency, count) = match cue {
        Cue::Select | Cue::SelectConstruction => (550.0, 420),
        Cue::Order => (880.0, 720),
        Cue::Error => (180.0, 960),
        _ => (1100.0, 1140),
    };
    let samples: Vec<_> = (0..count)
        .map(|index| {
            let phase = index as f64 / count as f64;
            let envelope = (phase * 20.0).min(1.0) * (1.0 - phase);
            ((index as f64 * frequency * std::f64::consts::TAU / 12000.0).sin() * envelope * 2500.0)
                as i16
        })
        .collect();
    PcmClip {
        channels: 1,
        sample_rate: 12000,
        samples: samples.into(),
    }
}

#[cfg(test)]
mod tests;
