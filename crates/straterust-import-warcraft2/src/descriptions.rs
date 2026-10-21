//! Role descriptions shared by fresh imports and installed-data upgrades.
pub(super) fn unit(source: usize) -> Option<&'static str> {
    Some(match source {
        0 | 1 | 16 | 17 | 54 | 55 => "Melee infantry. Attacks ground units and buildings.",
        2 | 3 => {
            "Worker. Mines gold, chops lumber, constructs buildings and repairs friendly structures."
        }
        4 | 5 => {
            "Long-range siege engine. Bombards ground targets; cannot fire at enemies inside its minimum range."
        }
        6 | 7 => {
            "Fast, durable melee cavalry. Attacks ground targets and can be upgraded at the Church or Altar of Storms."
        }
        8 | 9 | 18 | 19 | 20 | 53 => {
            "Ranged infantry. Attacks ground and air targets; benefits from Lumber Mill upgrades."
        }
        10 | 21 => {
            "Human spellcaster. Uses mana to cast spells; additional spells are researched at the Mage Tower."
        }
        11 | 24 | 51 => {
            "Undead spellcaster. Uses mana to cast spells; additional spells are researched at the Temple of the Damned."
        }
        12 | 50 | 52 => {
            "Upgraded Knight. Combines melee combat with spells researched at the Church, including healing."
        }
        13 => {
            "Upgraded Ogre. Combines melee combat with spells researched at the Altar of Storms, including Bloodlust."
        }
        14 | 15 => {
            "Demolition unit. Detonates to destroy nearby ground targets, sacrificing itself in the explosion."
        }
        22 | 35 | 42 | 43 => {
            "Flying attacker. Breathes fire at ground and air targets and crosses terrain freely."
        }
        23 => "Flying scout. Provides vision across otherwise impassable terrain.",
        25 => "Summoned eye. Explores terrain from the air for a limited time.",
        26 | 27 => {
            "Oil tanker. Builds platforms on oil fields, gathers oil and returns it to a Shipyard or Oil Refinery."
        }
        28 | 29 => {
            "Transport ship. Carries ground units across water; unload passengers on a reachable shore."
        }
        30 | 31 | 36 | 37 => "Light warship. Fires cannons at coastal and naval targets.",
        32 | 33 => {
            "Heavy warship. Fires powerful broadside cannon rounds at coastal and naval targets."
        }
        38 | 39 => {
            "Submerged warship. Attacks naval targets and remains hidden from enemies without detection."
        }
        40 | 41 => {
            "Fast flying scout. Reveals terrain and detects submerged submarines; cannot attack."
        }
        45 => "Wildlife. Roams the map and does not fight for either side.",
        58 | 59 => {
            "Provides food capacity for training more units. Build additional farms before reaching the food limit."
        }
        60 | 61 => {
            "Trains infantry, ranged troops, cavalry and siege engines as their required buildings become available."
        }
        62 => "Upgrades Knights to Paladins and researches their spells. Requires a Castle.",
        63 => "Upgrades Ogres to Ogre-Mages and researches their spells. Requires a Fortress.",
        64 | 65 => {
            "Scout tower. Provides vision and can be upgraded to a Guard Tower or Cannon Tower."
        }
        66 => "Enables Knight training and the Castle upgrade. Requires a Keep or Castle.",
        67 => "Enables Ogre training and the Fortress upgrade. Requires a Stronghold or Fortress.",
        68 => {
            "Trains Flying Machines, Demolition Squads and enables submarines. Requires a Keep or Castle."
        }
        69 => {
            "Trains Zeppelins, Goblin Sappers and enables Giant Turtles. Requires a Stronghold or Fortress."
        }
        70 => "Trains Gryphon Riders. Requires a Castle.",
        71 => "Trains Dragons. Requires a Fortress.",
        72 | 73 => {
            "Builds tankers, transports and warships, and receives oil deliveries. Requires a Lumber Mill and a suitable shoreline."
        }
        74 | 75 => {
            "Main resource depot. Trains workers, receives gold and lumber, and upgrades to unlock advanced buildings."
        }
        76 | 77 => {
            "Receives lumber, improves lumber deliveries and researches ranged-unit upgrades. Enables Shipyards and Guard Towers."
        }
        78 | 79 => {
            "Enables transports and heavy warships, and researches naval weapons and armor. Requires a Shipyard."
        }
        80 => "Trains Mages and researches their spells. Requires a Castle.",
        81 => "Trains Death Knights and researches their spells. Requires a Fortress.",
        82 | 83 => {
            "Researches melee weapons, armor and siege-engine damage. Enables cavalry and Cannon Towers."
        }
        84 | 85 => {
            "Receives tanker deliveries and increases oil collected per trip. Requires a Shipyard and a suitable shoreline."
        }
        86 | 87 => {
            "Extracts oil from the field beneath it. Tankers collect oil here and deliver it to a Shipyard or Oil Refinery."
        }
        88 | 89 => {
            "Upgraded main resource depot. Trains workers, improves gold deliveries and unlocks advanced buildings."
        }
        90 | 91 => {
            "Fully upgraded main resource depot. Trains workers, improves gold deliveries and unlocks the most advanced buildings."
        }
        92 => {
            "Gold deposit. Workers enter the mine to gather gold and return it to a main resource depot."
        }
        96 | 97 => {
            "Defensive tower. Fires ranged shots at ground and air enemies. Requires a Lumber Mill."
        }
        98 | 99 => {
            "Defensive tower. Fires powerful cannon rounds at ground enemies. Requires a Blacksmith."
        }
        999..=1087 => "Defensive wall. Blocks ground movement and can be destroyed by attacks.",
        _ => return None,
    })
}
