//! Source cargo sizes and the Starport/attached Control Tower transport path.
use super::*;

pub(crate) fn apply_transport_rules(
    archive: &mut Archive<std::io::Cursor<Vec<u8>>>,
    rules: &mut Rules,
) -> Result<()> {
    let units = archive.read_file("arr\\units.dat", 19192)?;
    for &(source, native) in MAPPING {
        if let Some(unit) = rules.units.iter_mut().find(|u| u.id == UnitTypeId(native)) {
            unit.cargo_size = units[0x4210 + usize::from(source)].clamp(1, 8);
        }
    }
    if !rules.units.iter().any(|u| u.id == UnitTypeId(53)) {
        return Ok(());
    }
    let passengers = MAPPING
        .iter()
        .filter_map(|&(source, native)| {
            rules
                .units
                .iter()
                .find(|u| {
                    u.id == UnitTypeId(native)
                        && !u.structure
                        && u.speed > 0
                        && !u.revealer
                        && u.mine.is_none()
                        && u.movement_class == MovementClass::Ground
                        && units[0x4210 + usize::from(source)] < 255
                })
                .map(|u| u.id)
        })
        .collect::<Vec<_>>();
    let prerequisites = [UnitTypeId(33), UnitTypeId(34)]
        .into_iter()
        .filter(|id| rules.units.iter().any(|u| u.id == *id))
        .collect::<Vec<_>>();
    for unit in &mut rules.units {
        match unit.id.0 {
            2 if !unit.repairs.contains(&UnitTypeId(53)) => unit.repairs.push(UnitTypeId(53)),
            33 if !unit.trains.contains(&UnitTypeId(53)) => unit.trains.push(UnitTypeId(53)),
            53 => {
                unit.prerequisites = prerequisites.clone();
                unit.garrison = Some(GarrisonStats {
                    boarding_range: 1,
                    capacity: units[0x42f4 + 11],
                    passengers: passengers.clone(),
                    attackers: vec![],
                    range_bonus: 0,
                    unload_ticks: 15,
                });
            }
            _ => {}
        }
    }
    Ok(())
}
