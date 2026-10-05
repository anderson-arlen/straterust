use super::*;

pub(crate) fn unit_id(source: u16) -> Result<UnitTypeId> {
    if let Some(id) = crate::campaign_units::native_id(source) {
        return Ok(id);
    }
    UNIT_IDS
        .iter()
        .find(|(id, _)| *id == source)
        .map(|(_, id)| UnitTypeId(*id))
        .with_context(|| format!("unsupported campaign unit {source}"))
}
pub(crate) fn player_id(source: u32) -> Result<PlayerId> {
    PLAYER_IDS
        .iter()
        .find(|(id, _)| u32::from(*id) == source)
        .map(|(_, id)| PlayerId(*id))
        .with_context(|| format!("unsupported campaign player {source}"))
}
pub(crate) fn players(source: u32) -> Result<Vec<PlayerId>> {
    match source {
        13 | 18 => Ok(vec![PlayerId(0)]),
        17 => Ok(vec![PlayerId(0), PlayerId(1), PlayerId(2), PlayerId(3)]),
        19 => Ok(vec![PlayerId(1)]),
        _ => Ok(vec![player_id(source)?]),
    }
}
pub(crate) fn portrait(source: u16) -> Result<UnitTypeId> {
    if source == 27 {
        return Ok(UnitTypeId(1001));
    }
    if matches!(source, 23 | 29) {
        Ok(UnitTypeId(1000))
    } else {
        unit_id(source)
    }
}
pub(crate) fn unit_match(source: u16) -> Result<MissionUnits> {
    Ok(match source {
        229 => MissionUnits::Any,
        231 => MissionUnits::Structures,
        _ => MissionUnits::Type(unit_id(source)?),
    })
}
pub(crate) fn comparison(value: u8) -> Result<MissionComparison> {
    Ok(match value {
        0 => MissionComparison::AtLeast,
        1 => MissionComparison::AtMost,
        10 => MissionComparison::Exactly,
        _ => bail!("unsupported trigger comparison {value}"),
    })
}
pub(crate) fn duration(action: &SourceAction) -> Result<u32> {
    Ok(match action.modifier {
        7 => action.second,
        8 => action
            .time
            .checked_add(action.second)
            .context("transmission duration overflow")?,
        9 => action.time.saturating_sub(action.second),
        _ => bail!("unsupported transmission modifier {}", action.modifier),
    })
}

pub(crate) struct References {
    pub(super) texts: Vec<String>,
    pub(super) text_ids: BTreeMap<u32, u16>,
    pub(super) sound_ids: BTreeMap<u32, u16>,
}
impl References {
    pub(crate) fn collect(
        triggers: &[SourceTrigger],
        briefing: &[SourceTrigger],
        strings: &[String],
    ) -> Result<Self> {
        let mut text_set = BTreeSet::new();
        let mut sounds = BTreeSet::new();
        for action in triggers.iter().chain(briefing).flat_map(|t| &t.actions) {
            for id in [action.text, action.sound]
                .into_iter()
                .filter(|id| *id != 0)
            {
                ensure!((id as usize) < strings.len(), "trigger string outside STR");
            }
            if action.text != 0 {
                text_set.insert(action.text);
            }
            if action.sound != 0 {
                sounds.insert(action.sound);
            }
        }
        ensure!(
            text_set.len() <= 256 && sounds.len() <= 128,
            "too many campaign references"
        );
        let texts = text_set
            .iter()
            .map(|id| strings[*id as usize].clone())
            .collect();
        let text_ids = text_set
            .into_iter()
            .enumerate()
            .map(|(i, id)| (id, i as u16))
            .collect();
        let sound_ids = sounds
            .into_iter()
            .enumerate()
            .map(|(i, id)| (id, i as u16))
            .collect();
        Ok(Self {
            texts,
            text_ids,
            sound_ids,
        })
    }
    pub(crate) fn text(&self, id: u32) -> Result<u16> {
        self.text_ids
            .get(&id)
            .copied()
            .context("missing mission text")
    }
    pub(crate) fn sound(&self, id: u32) -> Result<u16> {
        self.sound_ids
            .get(&id)
            .copied()
            .context("missing mission sound")
    }
    pub(crate) fn extract_audio<R: std::io::Read + std::io::Seek>(
        &mut self,
        installer: &mut Archive<R>,
        strings: &[String],
        files: &mut Files,
        members: &mut Vec<MemberReport>,
        race: &str,
        mission_number: u8,
    ) -> Result<()> {
        let mut media: MediaManifest = ron::de::from_bytes(&files["media.ron"])?;
        ensure!(
            media.mission_audio.is_empty(),
            "mission sounds already assigned"
        );
        let mut total = 0;
        for (&source, &native) in &self.sound_ids {
            let path = &strings[source as usize];
            ensure!(
                path.to_ascii_lowercase().starts_with("staredit\\wav\\")
                    && !path.contains("..")
                    && !path.contains('/')
                    && path.len() < 128,
                "invalid campaign WAV member"
            );
            let full = format!("campaign\\{race}\\{race}{mission_number:02}\\{path}");
            let bytes = member(
                installer,
                &full,
                16 * 1024 * 1024,
                "install",
                "original mission/briefing speech",
                members,
            )?;
            let pcm =
                decode_wav(&bytes).with_context(|| format!("unsupported mission WAV {full}"))?;
            ensure!(pcm.duration_ms() <= 120_000, "mission WAV exceeds2minutes");
            total += pcm.samples.len() * 2;
            ensure!(total <= 64 * 1024 * 1024, "mission audio exceeds64MiB");
            let normalized = encode_wav(pcm.channels, pcm.sample_rate, &pcm.samples)?;
            let reference = AudioRef {
                file: format!("mission-sound-{native:03}.wav"),
                blake3: blake3::hash(&normalized).to_hex().to_string(),
            };
            files.insert(reference.file.clone(), normalized);
            media.mission_audio.push(reference);
        }
        media.mission_texts = self.texts.clone();
        files.insert("media.ron".into(), ron_bytes(&media)?);
        Ok(())
    }
}
pub(crate) fn translate_mission(
    triggers: &[SourceTrigger],
    locations: &BTreeMap<u16, MissionLocation>,
    refs: &References,
) -> Result<Mission> {
    let indices: BTreeMap<_, _> = locations
        .keys()
        .enumerate()
        .map(|(i, id)| (*id, i as u16))
        .collect();
    let location = |id: u32| -> Result<u16> {
        indices
            .get(&u16::try_from(id)?)
            .copied()
            .with_context(|| format!("missing campaign location {id}"))
    };
    let mut native = Vec::new();
    for source in triggers {
        let mut conditions = Vec::new();
        for c in &source.conditions {
            conditions.push(match c.kind {
                1 => MissionCondition::Countdown {
                    comparison: comparison(c.comparison)?,
                    milliseconds: c.amount.checked_mul(1000).context("countdown overflow")?,
                },
                2 | 3 => MissionCondition::Count {
                    players: players(c.player)?,
                    units: unit_match(c.unit)?,
                    location: if c.kind == 3 {
                        Some(location(c.location)?)
                    } else {
                        None
                    },
                    comparison: comparison(c.comparison)?,
                    amount: c.amount,
                },
                11 => {
                    ensure!(matches!(c.comparison, 2 | 3), "invalid switch comparison");
                    MissionCondition::Switch {
                        index: u16::from(c.switch),
                        set: c.comparison == 2,
                    }
                }
                _ => bail!("unsupported campaign condition {}", c.kind),
            });
        }
        let mut actions = Vec::new();
        for a in &source.actions {
            let action = match a.kind {
                1 => MissionAction::Victory,
                2 => MissionAction::Defeat,
                4 => MissionAction::Wait {
                    milliseconds: a.time,
                },
                5 => MissionAction::Pause,
                7 => MissionAction::Transmission {
                    text: refs.text(a.text)?,
                    sound: if a.sound == 0 {
                        None
                    } else {
                        Some(refs.sound(a.sound)?)
                    },
                    portrait: portrait(a.unit)?,
                    location: location(a.location)?,
                    milliseconds: duration(a)?,
                },
                8 => MissionAction::Sound {
                    sound: refs.sound(a.sound)?,
                },
                9 => MissionAction::Text {
                    text: refs.text(a.text)?,
                },
                10 => MissionAction::CenterView {
                    location: location(a.location)?,
                },
                11 => {
                    ensure!(
                        a.second == 0 && a.modifier == 0,
                        "unsupported legacy create properties"
                    );
                    MissionAction::Create {
                        properties: Default::default(),
                        player: player_id(a.player)?,
                        unit_type: unit_id(a.unit)?,
                        location: location(a.location)?,
                    }
                }
                12 => MissionAction::Objectives {
                    text: refs.text(a.text)?,
                },
                13 => {
                    ensure!(
                        matches!(a.modifier, 4 | 5) && a.second < 256,
                        "unsupported switch action"
                    );
                    MissionAction::SetSwitch {
                        index: a.second as u16,
                        set: a.modifier == 4,
                    }
                }
                23 => {
                    ensure!(a.modifier == 0, "unsupported legacy kill count");
                    MissionAction::Kill {
                        players: players(a.player)?,
                        units: unit_match(a.unit)?,
                        location: location(a.location)?,
                    }
                }
                26 => {
                    ensure!(
                        a.modifier == 7 && a.unit <= 2,
                        "unsupported resource action"
                    );
                    MissionAction::SetResources {
                        players: players(a.player)?,
                        resources: match a.unit {
                            0 => vec![ResourceAmount {
                                kind: "minerals".into(),
                                amount: a.second,
                            }],
                            1 => vec![ResourceAmount {
                                kind: "gas".into(),
                                amount: a.second,
                            }],
                            _ => vec![
                                ResourceAmount {
                                    kind: "minerals".into(),
                                    amount: a.second,
                                },
                                ResourceAmount {
                                    kind: "gas".into(),
                                    amount: a.second,
                                },
                            ],
                        },
                    }
                }
                30 | 31 => MissionAction::Speech {
                    muted: a.kind == 30,
                },
                38 => MissionAction::MoveLocation {
                    location: location(a.second)?,
                    players: players(a.player)?,
                    units: unit_match(a.unit)?,
                    search_location: location(a.location)?,
                },
                43 => {
                    ensure!(
                        matches!(a.modifier, 4 | 5),
                        "unsupported invincibility toggle"
                    );
                    MissionAction::Invincibility {
                        players: players(a.player)?,
                        units: unit_match(a.unit)?,
                        location: location(a.location)?,
                        enabled: a.modifier == 4,
                    }
                }
                _ => bail!("unsupported campaign action {}", a.kind),
            };
            actions.push(action);
        }
        native.push(MissionTrigger {
            conditions,
            actions,
        });
    }
    Ok(Mission {
        schema_version: 1,
        player: PlayerId(0),
        poll_ticks: 31,
        wait_step_ms: 42,
        locations: locations.values().copied().collect(),
        triggers: native,
        rescuable_players: vec![PlayerId(2), PlayerId(3)],
        rescuers: vec![PlayerId(0)],
        alliances: vec![
            [PlayerId(0), PlayerId(2)],
            [PlayerId(0), PlayerId(3)],
            [PlayerId(1), PlayerId(2)],
            [PlayerId(1), PlayerId(3)],
            [PlayerId(2), PlayerId(3)],
        ],
    })
}
pub(crate) fn write_presentation(
    files: &mut Files,
    briefing: &[SourceTrigger],
    refs: &References,
) -> Result<()> {
    use straterust_engine::media::BriefingAction;
    let mut media: MediaManifest = ron::de::from_bytes(&files["media.ron"])?;
    let mut actions = Vec::new();
    for a in briefing.iter().flat_map(|t| &t.actions) {
        let action = match a.kind {
            1 => BriefingAction::Wait {
                milliseconds: a.time,
            },
            2 => BriefingAction::Sound {
                sound: refs.sound(a.sound)?,
            },
            3 => BriefingAction::Text {
                text: refs.text(a.text)?,
                milliseconds: a.time,
            },
            4 => BriefingAction::Objectives {
                text: refs.text(a.text)?,
            },
            5 => {
                ensure!(a.player < 4, "invalid briefing portrait slot");
                BriefingAction::ShowPortrait {
                    slot: a.player as u8,
                    portrait: portrait(a.unit)?,
                }
            }
            6 => {
                ensure!(a.player < 4, "invalid briefing portrait slot");
                BriefingAction::HidePortrait {
                    slot: a.player as u8,
                }
            }
            8 => {
                ensure!(a.player < 4, "invalid briefing transmission slot");
                BriefingAction::Transmission {
                    slot: a.player as u8,
                    text: refs.text(a.text)?,
                    sound: if a.sound == 0 {
                        None
                    } else {
                        Some(refs.sound(a.sound)?)
                    },
                    milliseconds: duration(a)?,
                }
            }
            _ => bail!("unsupported briefing action {}", a.kind),
        };
        actions.push(action);
    }
    media.briefing = actions;
    media.validate()?;
    files.insert("media.ron".into(), ron_bytes(&media)?);
    Ok(())
}
