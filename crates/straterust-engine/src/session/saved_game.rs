//! Local save format: integrity is checked on original bytes before defaults or
//! migrations change the typed state. Deterministic replay hashes stay strict.
use super::*;
use crate::sim::SaveDefinitions;
use ron::value::RawValue;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedGame {
    pub checkpoint: SessionSnapshot,
    pub definitions: Option<SaveDefinitions>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u32,
    checksum: String,
    // Keep the payload opaque during envelope parsing. RawValue's untyped
    // validation repeatedly walks large fog grids and is extremely expensive.
    data: String,
}

impl SavedGame {
    pub fn capture(server: &ServerSession) -> Result<Self> {
        Ok(Self {
            checkpoint: server.save_snapshot()?,
            definitions: Some(SaveDefinitions::of(server.world())),
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        // Do not legitimize an already damaged checkpoint by wrapping it.
        self.checkpoint.world.verify_checksum()?;
        let data = ron::ser::to_string(self)?;
        let envelope = Envelope {
            version: 2,
            checksum: blake3::hash(data.as_bytes()).to_hex().to_string(),
            data,
        };
        Ok(ron::ser::to_string(&envelope)?.into_bytes())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        #[derive(Deserialize)]
        struct Version {
            version: u32,
        }
        // Original writer output is canonical compact RON. Avoid traversing
        // an entire legacy state merely to read its leading version number.
        let version = if bytes.starts_with(b"(version:1,world:") {
            1
        } else {
            ron::de::from_bytes::<Version>(bytes)?.version
        };
        match version {
            2 => {
                let envelope: Envelope = ron::de::from_bytes(bytes)?;
                ensure!(
                    envelope.checksum == blake3::hash(envelope.data.as_bytes()).to_hex().as_str(),
                    "saved game checksum mismatch"
                );
                let mut saved: Self = ron::from_str(&envelope.data)?;
                saved.checkpoint.world.refresh_checksum()?;
                Ok(saved)
            }
            1 => {
                // Original saves contain a plain SessionSnapshot and checksum
                // of the serialized state. Hash that raw state, not a newer
                // struct reserialized with newly introduced default fields.
                #[derive(Deserialize)]
                struct LegacyWorld<'a> {
                    #[serde(borrow)]
                    state: &'a RawValue,
                    checksum: String,
                }
                #[derive(Deserialize)]
                struct Legacy<'a> {
                    #[serde(borrow)]
                    world: LegacyWorld<'a>,
                }
                let mut checkpoint: SessionSnapshot = ron::de::from_bytes(bytes)?;
                let text = std::str::from_utf8(bytes)?;
                let prefix = format!(
                    "(version:{},world:(version:{},identity:{},state:",
                    checkpoint.version,
                    checkpoint.world.version,
                    ron::ser::to_string(&checkpoint.world.identity)?
                );
                let suffix = format!(
                    ",state_hash:{},checksum:{}),seed:",
                    ron::ser::to_string(&checkpoint.world.state_hash)?,
                    ron::ser::to_string(&checkpoint.world.checksum)?
                );
                let canonical = text
                    .strip_prefix(&prefix)
                    .and_then(|rest| rest.rfind(&suffix).map(|end| &rest[..end]));
                // Typed parsing above validates the full document. Canonical
                // field framing identifies the original state bytes without
                // reparsing its grids. Retain RawValue for noncanonical RON.
                let legacy;
                let state = if let Some(state) = canonical {
                    state
                } else {
                    legacy = ron::de::from_bytes::<Legacy<'_>>(bytes)?;
                    ensure!(
                        legacy.world.checksum == checkpoint.world.checksum,
                        "saved game checksum mismatch"
                    );
                    legacy.world.state.get_ron()
                };
                ensure!(
                    checkpoint.world.checksum == blake3::hash(state.as_bytes()).to_hex().as_str(),
                    "saved game checksum mismatch"
                );
                checkpoint.world.refresh_checksum()?;
                Ok(Self {
                    checkpoint,
                    definitions: None,
                })
            }
            version => anyhow::bail!("unsupported saved game format {version}"),
        }
    }
}
