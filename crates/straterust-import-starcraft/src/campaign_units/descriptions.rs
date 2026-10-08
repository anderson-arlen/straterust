//! Game explanations are package content, never native-client unit-ID checks.
use super::*;

pub(super) fn ability(id: u16) -> &'static str {
    match id {
        1 => {
            "Detonate an EMP shockwave at the target point, draining shields and energy in the affected area. Friendly units are also affected."
        }
        2 => {
            "Surround a unit with radiation that damages nearby organic units, including allies. Mechanical units can carry the radiation without being harmed by it."
        }
        3 => {
            "Temporarily disable a mechanical unit, preventing movement, attacks and abilities. Does not affect buildings or organic units."
        }
        4 => {
            "Charge and fire a powerful Yamato blast at one enemy unit or building. The Battlecruiser must hold position while charging; the projectile then travels to its target."
        }
        5 => {
            "Temporarily shield a friendly unit with a defensive matrix that absorbs incoming damage."
        }
        6 => {
            "Designate a ground target for a loaded Nuclear Silo. The Ghost must remain alive and keep targeting until the missile begins its descent. The strike consumes one missile and damages all units and buildings in its blast, including allies."
        }
        7 => {
            "Destroy a biological ground unit and create two short-lived Broodlings. Does not affect buildings, robotic units or flying units."
        }
        8 => {
            "Cover an area with Ensnare, slowing affected units and revealing concealed units for its duration."
        }
        9 => {
            "Attach a parasite to an enemy unit to share its sight. The target remains under enemy control."
        }
        10 => {
            "Infest a heavily damaged enemy Command Center, converting it into an Infested Command Center under your control."
        }
        11 => {
            "Create a Dark Swarm at the target point. Ground units beneath it are protected from ordinary ranged fire; melee and splash attacks can still harm them."
        }
        12 => {
            "Infect units and buildings in an area with Plague. It drains life over time without delivering the killing blow and does not drain shields."
        }
        13 => {
            "Consume a friendly Zerg unit to restore the Defiler's energy. The consumed unit is destroyed."
        }
        14 => {
            "Create a Psionic Storm that damages units in the target area over time, including friendly units. Storms do not stack their damage."
        }
        15 => {
            "Create two temporary copies of a unit. Hallucinations deal no damage and take increased damage; use them to draw enemy fire."
        }
        16 => {
            "Recall friendly units from the target area to the Arbiter's location after a short delay."
        }
        17 => {
            "Trap units in the target area in a Stasis Field. Affected units cannot act and cannot be damaged until the field expires."
        }
        18 => {
            "Merge two High Templar into one Archon. Both Templar are consumed by the transformation."
        }
        19 => {
            "Recharge a friendly Protoss unit's shields using the Shield Battery's energy. The unit must approach the battery."
        }
        20 => {
            "Create this Nydus Canal's paired exit on creep. Order eligible ground units into either entrance to emerge from the other."
        }
        _ => unreachable!("unmapped spell explanation"),
    }
}

pub(super) fn research(effect: &ResearchEffect, source: usize, technology: bool) -> String {
    use ResearchEffect::*;
    match effect {
        Ability { ability: id, .. } => format!("Unlocks this ability on the affected units. {}", ability(id.0)),
        VisionRange { amount, .. } => format!("Permanently increases sight range by {} tiles. This is a passive upgrade; no activation button is needed.", amount / 32),
        WeaponRange { amount, .. } => format!("Permanently increases weapon range by {} tile(s).", amount / 32),
        Armor { amount, .. } => format!("Permanently adds {amount} armor to the affected unit class, reducing incoming weapon damage."),
        ShieldArmor { amount, .. } => format!("Permanently adds {amount} armor to Protoss shields on units and buildings."),
        WeaponUpgrade { .. } | WeaponDamage { .. } => "Permanently increases weapon damage for the affected unit class. Each level adds the weapon's listed upgrade bonus.".into(),
        EnergyCapacity { amount, .. } => format!("Permanently increases maximum energy by {amount}. It does not instantly refill the unit's energy."),
        MovementSpeed { .. } => "Permanently increases movement speed for the affected units.".into(),
        AttackRate { .. } => "Permanently increases Zergling attack speed.".into(),
        ProductionCapacity { amount, .. } => format!("Increases the number of carried ammunition units by {amount}. Additional ammunition must still be produced."),
        Mode { .. } => "Enables Siege Mode on Siege Tanks. Siege Mode gives powerful long-range ground fire but prevents movement and has a minimum firing range.".into(),
        Transport { .. } => "Enables Overlords to carry friendly ground units. Order units into an Overlord to load; use Unload All to choose an unloading destination.".into(),
        Cloak { .. } if technology && source == 11 => "Enables eligible Zerg ground units to burrow and unburrow. Burrowed units cannot move or attack and require detection to be seen.".into(),
        Cloak { .. } => "Enables Cloak (C) on the affected units. Cloaking costs energy and drains it while active. Detectors reveal cloaked units; use Decloak (D) to stop the drain.".into(),
        Mines { .. } => "Enables Vultures to plant their limited supply of Spider Mines. Mines burrow, detect approaching ground enemies and detonate.".into(),
        Stim { .. } => "Enables Stim Packs on Marines and Firebats. Stimulation spends life to temporarily increase movement and attack speed.".into(),
    }
}

pub(super) fn refresh(files: &mut Files, rules: &Rules) -> Result<()> {
    let descriptions: BTreeMap<_, _> = MAPPING.iter().filter_map(|&(source, native)| {
        rules.units.iter().any(|u| u.id == UnitTypeId(native)).then_some(())?;
        let description = match source {
            14 => "Arm a Nuclear Silo with one missile. A Ghost designates its target using Nuclear Strike. Each launch consumes the missile.",
            73 => "Build an Interceptor for this Carrier. Interceptors launch automatically to attack and return to the Carrier between attacks.",
            85 => "Build a Scarab for this Reaver. Each ground attack consumes one Scarab.",
            106 => "Terran resource depot. Trains SCVs, accepts minerals and vespene gas, and supports a Comsat Station or Nuclear Silo addon. Can lift off.",
            107 => "Command Center addon. Spend its energy on Scanner Sweep to reveal terrain and detect concealed units in a target area.",
            108 => "Command Center addon. Builds and stores one nuclear missile for a Ghost to launch. Requires a Covert Ops.",
            109 => "Provides supply for Terran units, allowing more units to be trained.",
            110 => "Build on a vespene geyser to allow SCVs to harvest gas.",
            111 => "Trains Terran infantry. Can lift off.",
            112 => "Researches Terran infantry abilities, including Stim Packs and Marine weapon range.",
            113 => "Trains Terran ground vehicles. Supports a Machine Shop addon. Can lift off.",
            114 => "Trains Terran aircraft. Supports a Control Tower addon. Can lift off.",
            115 => "Starport addon. Enables Dropships and Science Vessels and researches Wraith cloaking and energy capacity.",
            116 => "Unlocks advanced Terran technology, supports Covert Ops or Physics Lab addons, and researches Science Vessel abilities. Can lift off.",
            117 => "Science Facility addon. Unlocks Ghost production and nuclear technology; researches Lockdown, personal cloaking, Ghost sight range and energy capacity.",
            118 => "Science Facility addon. Unlocks Battlecruiser production and researches the Yamato Gun and Battlecruiser energy capacity.",
            120 => "Factory addon. Unlocks Siege Tanks and researches Siege Mode, Spider Mines and Vulture speed.",
            122 => "Researches Terran infantry weapons and armor upgrades. Can lift off.",
            123 => "Unlocks Goliath production and researches Terran vehicle and ship weapons and armor.",
            124 => "Stationary air defense and detector. Reveals concealed enemies and attacks flying targets.",
            125 => "Shelters up to four infantry units, which can fire from inside with increased range. Click a passenger icon to unload that unit.",
            130 => "Infested Command Center. Produces Infested Terrans for explosive ground attacks.",
            131 => "Zerg resource depot. Accepts minerals and gas, generates larva and spreads creep. Can mutate into a Lair.",
            132 => "Upgraded Hatchery. Unlocks advanced Zerg technology while continuing to generate larva; can mutate into a Hive.",
            133 => "Final Hatchery evolution. Unlocks the most advanced Zerg technology and upgrades while generating larva.",
            134 => "Create a paired exit to transport Zerg ground units instantly between two Nydus Canal entrances.",
            135 => "Unlocks Hydralisk mutations and researches Hydralisk movement speed and weapon range.",
            136 => "Unlocks Defilers and researches their spells and energy capacity.",
            137 => "Upgraded Spire. Unlocks Guardian mutations and researches Zerg flyer weapons and armor.",
            138 => "Unlocks Queens and researches their spells and energy capacity.",
            139 => "Researches Zerg ground weapons and armor.",
            140 => "Unlocks Ultralisks.",
            141 => "Unlocks flying Zerg combat units and researches flyer weapons and armor. Can mutate into a Greater Spire.",
            142 => "Unlocks Zerglings and researches their movement and attack speed.",
            143 => "Spreads creep and can mutate into a Sunken Colony or Spore Colony.",
            144 => "Stationary Zerg air defense and detector. Reveals concealed enemies and attacks flying targets.",
            146 => "Stationary Zerg ground defense. Attacks ground enemies with subterranean spines.",
            149 => "Mutate on a vespene geyser to allow Drones to harvest gas.",
            154 => "Protoss resource depot. Trains Probes and accepts minerals and vespene gas.",
            155 => "Builds robotic Protoss units, including Shuttles, Reavers and Observers when their supporting structures are available. Requires Pylon power.",
            156 => "Provides Protoss supply and powers nearby structures. Powered coverage appears while placing a structure that needs it.",
            157 => "Warp onto a vespene geyser to allow Probes to harvest gas.",
            159 => "Unlocks Observers and researches their sight range and movement speed. Requires Pylon power.",
            160 => "Trains Protoss ground warriors. Requires Pylon power.",
            162 => "Stationary Protoss ground and air defense and detector. Requires Pylon power.",
            163 => "Unlocks advanced Protoss technology and researches Zealot movement speed. Requires Pylon power.",
            164 => "Unlocks advanced Protoss technology and researches air weapons, air armor and Dragoon range. Requires Pylon power.",
            165 => "Unlocks High Templar technology and researches Psionic Storm, Hallucination and Templar energy capacity. Requires Pylon power.",
            166 => "Researches Protoss ground weapons, ground armor and plasma shields. Requires Pylon power.",
            167 => "Trains Protoss aircraft. Requires Pylon power.",
            169 => "Unlocks Carriers and researches Carrier capacity and Scout improvements. Requires Pylon power.",
            170 => "Unlocks Arbiters and researches their spells and energy capacity. Requires Pylon power.",
            171 => "Unlocks Reavers and researches Reaver damage, Scarab capacity and Shuttle speed. Requires Pylon power.",
            172 => "Spend the Shield Battery's energy to recharge nearby friendly Protoss shields. Requires Pylon power.",
            _ => return None,
        };
        Some((UnitTypeId(native), description))
    }).collect();
    let mut presentation = std::str::from_utf8(&files["presentation.ron"])?.to_owned();
    set_map(
        &mut presentation,
        "unit_descriptions",
        &ron::ser::to_string(&descriptions)?,
    )?;
    files.insert("presentation.ron".into(), presentation.into_bytes());
    Ok(())
}
