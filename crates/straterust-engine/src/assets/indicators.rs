//! Optional selection and pointer artwork, independent of gameplay geometry.
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitIndicator {
    pub unit_type: UnitTypeId,
    pub circle: u8,
    pub circle_y: i16,
    pub bar_y: i16,
    pub bar_width: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CursorManifest {
    pub key: String,
    /// Vertical frame atlas, with one fixed canvas/anchor for every frame.
    pub image: ImageRef,
    pub frames: u16,
    pub frame_ms: u32,
    pub anchor: [u16; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndicatorsManifest {
    /// Each atlas has three rows: friendly, neutral and enemy circle palettes.
    pub circles: Vec<ImageRef>,
    pub units: Vec<UnitIndicator>,
    pub health_colors: [u32; 19],
    pub cursors: Vec<CursorManifest>,
}

impl IndicatorsManifest {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            !self.circles.is_empty() && self.circles.len() <= 20,
            "invalid selection circles"
        );
        ensure!(self.units.len() <= 256, "too many unit indicators");
        let mut units = BTreeSet::new();
        for entry in &self.units {
            ensure!(
                units.insert(entry.unit_type)
                    && usize::from(entry.circle) < self.circles.len()
                    && entry.circle_y.unsigned_abs() <= 256
                    && entry.bar_y.unsigned_abs() <= 512
                    && (19..=256).contains(&entry.bar_width),
                "invalid unit selection indicator"
            );
        }
        ensure!(
            self.health_colors.iter().all(|color| *color <= 0xffffff),
            "invalid health bar palette"
        );
        ensure!(self.cursors.len() <= 16, "too many cursors");
        let mut keys = BTreeSet::new();
        for cursor in &self.cursors {
            ensure!(
                keys.insert(&cursor.key)
                    && !cursor.key.is_empty()
                    && cursor.key.len() <= 32
                    && cursor
                        .key
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
                    && (1..=32).contains(&cursor.frames)
                    && (1..=1000).contains(&cursor.frame_ms),
                "invalid cursor"
            );
            validate_reference(&cursor.image)?;
        }
        for circle in &self.circles {
            validate_reference(circle)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct IndicatorsPack {
    pub manifest: IndicatorsManifest,
    pub circles: Vec<Image>,
    pub cursors: Vec<Image>,
}

impl IndicatorsPack {
    pub(super) fn load(
        manifest: &IndicatorsManifest,
        load: &mut impl FnMut(&ImageRef) -> Result<Image>,
    ) -> Result<Self> {
        let circles = manifest
            .circles
            .iter()
            .map(&mut *load)
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            circles.iter().all(|image| image.height.is_multiple_of(3)),
            "invalid circle atlas"
        );
        let cursors = manifest
            .cursors
            .iter()
            .map(|cursor| {
                let image = load(&cursor.image)?;
                ensure!(
                    image.height.is_multiple_of(u32::from(cursor.frames))
                        && u32::from(cursor.anchor[0]) < image.width
                        && u32::from(cursor.anchor[1]) < image.height / u32::from(cursor.frames),
                    "invalid cursor atlas/anchor"
                );
                Ok(image)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            manifest: manifest.clone(),
            circles,
            cursors,
        })
    }
}
