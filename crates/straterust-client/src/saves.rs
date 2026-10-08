//! Local files contain server checkpoints. Small headers list slots without
//! decoding authoritative state into the window's presentation world.
use super::*;
use serde::{Deserialize, Serialize};
use std::io::Read;
use straterust_engine::session::SavedGame;

const MAGIC: &[u8; 8] = b"SRSAVE01";
const MAX_HEADER: usize = 1024 * 1024;
const MAX_SAVE: u64 = 128 * 1024 * 1024;
pub(super) const SLOT_COUNT: usize = 7;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CampaignSave {
    root: PathBuf,
    id: String,
    index: usize,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SaveHeader {
    pub game: PathBuf,
    pub package: PathBuf,
    pub title: String,
    pub tick: u64,
    campaign: Option<CampaignSave>,
    camera: Camera,
    selected: BTreeSet<EntityId>,
    resource: Option<ResourceId>,
    groups: [BTreeSet<EntityId>; 10],
    mission: Option<mission::MissionUi>,
    animation: Duration,
    paused: bool,
    sequence: u64,
    result: Option<straterust_engine::session::MatchResult>,
}

impl SaveHeader {
    pub fn capture(app: &App, game: &Path) -> Result<Self> {
        ensure!(
            app.network.is_none() && app.playback_end.is_none(),
            "save is available in local games only"
        );
        let package = app
            .package_directory
            .clone()
            .context("session has no installed package")?;
        Ok(Self {
            game: game.canonicalize()?,
            package,
            title: app.campaign.as_ref().map_or_else(
                || app.world.rules().id.clone(),
                |c| c.manifest.missions[c.index].title.clone(),
            ),
            tick: app.world.tick().0,
            campaign: app.campaign.as_ref().map(|c| CampaignSave {
                root: c.root.clone(),
                id: c.manifest.id.clone(),
                index: c.index,
            }),
            camera: app.camera,
            selected: app.selected.clone(),
            resource: app.selected_resource,
            groups: app.groups.clone(),
            mission: app.mission_ui.clone(),
            animation: app.animation_elapsed,
            paused: app.paused,
            sequence: app.sequence,
            result: app.match_result.clone(),
        })
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.game.is_absolute()
                && self.package.is_absolute()
                && self.package.starts_with(&self.game),
            "saved package is outside its installed game"
        );
        ensure!(
            self.title.len() <= 512
                && self.camera.x.is_finite()
                && self.camera.y.is_finite()
                && self.camera.zoom.is_finite()
                && (0.25..=4.0).contains(&self.camera.zoom),
            "invalid saved presentation"
        );
        ensure!(
            self.selected.len() <= SELECTION_LIMIT
                && self.groups.iter().all(|g| g.len() <= SELECTION_LIMIT),
            "invalid saved selection"
        );
        Ok(())
    }

    pub fn apply(self, app: &mut App) -> Result<()> {
        self.validate()?;
        ensure!(
            self.tick == app.world.tick().0,
            "save header and checkpoint ticks differ"
        );
        if let Some(c) = self.campaign {
            let root = c
                .root
                .canonicalize()
                .context("saved campaign is no longer installed")?;
            let manifest = Campaign::load(&root)?;
            ensure!(
                root.starts_with(&self.game)
                    && manifest.id == c.id
                    && c.index < manifest.missions.len()
                    && root
                        .join(&manifest.missions[c.index].package)
                        .canonicalize()?
                        == self.package,
                "saved campaign no longer matches its mission"
            );
            app.campaign = Some(CampaignSession {
                root,
                manifest,
                index: c.index,
            });
        }
        app.camera = self.camera;
        app.camera.clamp_to_map(
            [app.world.map().width, app.world.map().height],
            [f64::from(app.config.width), f64::from(app.config.height)],
        );
        let owned = |id: &EntityId| {
            app.world
                .state()
                .entities
                .iter()
                .any(|e| e.id == *id && e.owner == app.world.view_player())
        };
        app.selected = self.selected.into_iter().filter(owned).collect();
        app.groups = self.groups.map(|g| g.into_iter().filter(owned).collect());
        app.selected_resource = self
            .resource
            .filter(|id| app.world.state().resources.iter().any(|r| r.id == *id));
        app.mission_ui = self.mission;
        app.animation_elapsed = self.animation;
        app.paused = self.paused;
        app.sequence = self.sequence;
        app.match_result = self.result;
        app.status = format!("Loaded {}", self.title);
        Ok(())
    }
}

pub(super) fn slot_path(directory: &Path, slot: usize) -> Result<PathBuf> {
    ensure!(slot < SLOT_COUNT, "invalid save slot");
    Ok(directory.join(format!("slot-{}.srsave", slot + 1)))
}

pub(super) fn labels(directory: &Path, game: &Path) -> Vec<String> {
    (0..SLOT_COUNT)
        .map(|slot| {
            let path = slot_path(directory, slot).unwrap();
            match read_header(&path) {
                Ok(header) if header.package.starts_with(game) => {
                    format!("{}. {} (tick {})", slot + 1, header.title, header.tick)
                }
                Ok(_) => format!("{}. Saved game from another package", slot + 1),
                Err(_) if path.exists() => format!("{}. Unreadable saved game", slot + 1),
                Err(_) => format!("{}. Empty slot", slot + 1),
            }
        })
        .collect()
}

fn header_from(file: &mut File) -> Result<SaveHeader> {
    ensure!(
        file.metadata()?.len() <= MAX_SAVE,
        "save exceeds size limit"
    );
    let mut prefix = [0; 12];
    file.read_exact(&mut prefix)
        .context("truncated save header")?;
    ensure!(&prefix[..8] == MAGIC, "unsupported save format");
    let length = u32::from_le_bytes(prefix[8..].try_into()?) as usize;
    ensure!(length <= MAX_HEADER, "save header exceeds size limit");
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes)?;
    let header: SaveHeader = ron::de::from_bytes(&bytes).context("invalid save header")?;
    header.validate()?;
    Ok(header)
}

pub(super) fn read_header(path: &Path) -> Result<SaveHeader> {
    header_from(&mut File::open(path).context("cannot open saved game")?)
}

pub(super) fn read(path: &Path) -> Result<(SaveHeader, SavedGame)> {
    let mut file = File::open(path)?;
    let header = header_from(&mut file)?;
    let mut bytes = Vec::new();
    file.take(MAX_SAVE + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_SAVE, "save exceeds size limit");
    let saved = SavedGame::decode(&bytes).context("invalid saved checkpoint")?;
    let snapshot = &saved.checkpoint;
    ensure!(
        snapshot.participants == [PlayerId(0)] && header.tick == snapshot.world.state.tick.0,
        "save is not a matching local session"
    );
    let accepted_sequence = snapshot
        .world
        .state
        .last_sequences
        .first()
        .copied()
        .unwrap_or(0)
        .max(
            snapshot
                .pending
                .iter()
                .map(|c| c.sequence)
                .max()
                .unwrap_or(0),
        );
    ensure!(
        header.sequence >= accepted_sequence,
        "saved command sequence is stale"
    );
    Ok((header, saved))
}

pub(super) fn write(path: &Path, header: &SaveHeader, saved: &SavedGame) -> Result<()> {
    header.validate()?;
    let snapshot = &saved.checkpoint;
    ensure!(
        header.tick == snapshot.world.state.tick.0,
        "save must capture a settled tick"
    );
    let metadata = ron::ser::to_string(header)?.into_bytes();
    let data = saved.encode()?;
    ensure!(
        metadata.len() <= MAX_HEADER && (12 + metadata.len() + data.len()) as u64 <= MAX_SAVE,
        "save exceeds size limit"
    );
    std::fs::create_dir_all(path.parent().context("save has no directory")?)?;
    // A leftover from a crashed process must not prevent later saves. The
    // worker serializes writes; separate game processes use separate files.
    let temporary = path.with_extension(format!("srsave.{}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)
        .context("cannot create save temporary file")?;
    let result = (|| -> Result<()> {
        file.write_all(MAGIC)?;
        file.write_all(&(metadata.len() as u32).to_le_bytes())?;
        file.write_all(&metadata)?;
        file.write_all(&data)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path).context("cannot publish saved game")
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests;
