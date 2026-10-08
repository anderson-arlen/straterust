use super::*;

fn table() -> Vec<u8> {
    let count = 700_u16;
    let mut bytes = vec![0; 2 + usize::from(count) * 2];
    bytes[..2].copy_from_slice(&count.to_le_bytes());
    for index in 1..=usize::from(count) {
        let offset = u16::try_from(bytes.len()).unwrap();
        bytes[index * 2..index * 2 + 2].copy_from_slice(&offset.to_le_bytes());
        bytes.extend_from_slice(match index {
            586 => b"k\x01Train \x03K\x01night\0",
            592 => b"j\x01Train \x03J\x01ourneyman\0",
            463 => b"b\x02Upgrade \x03B\x01lades\0",
            688 | 693 => b"\x1b\0",
            _ => b"q\0",
        });
    }
    bytes
}

#[test]
fn hotkeys_refresh_all_categories_from_source_strings_without_removing_other_controls() {
    let mut files = Files::from([("presentation.ron".into(), br#"(
        schema_version:1, train_keys:{1:"V",999:"Q"},
        research_keys:{3:"W",999:"P"}, command_keys:{"cancel":"X","custom":"Z"},
        command_buttons:{"cloak.23.off":(slot:7,key:"D",label:"Decloak",tip:"Deactivate",icon:"command.decloak")},
    )"#.to_vec())]);
    apply(&mut files, &table()).unwrap();
    let keys: Keys = ron::de::from_bytes(&files["presentation.ron"]).unwrap();
    assert_eq!(keys.train_keys[&UnitTypeId(1)], "K");
    assert_eq!(keys.train_keys[&UnitTypeId(2)], "J");
    assert_eq!(keys.train_keys[&UnitTypeId(999)], "Q");
    assert_eq!(keys.research_keys[&ResearchId(1)], "B");
    assert_eq!(keys.research_keys[&ResearchId(3)], "Q");
    assert_eq!(keys.research_keys[&ResearchId(999)], "P");
    assert_eq!(keys.command_keys["cancel"], "Esc");
    assert_eq!(keys.command_keys["back"], "Esc");
    assert_eq!(keys.command_keys["build.16"], "Q");
    assert_eq!(keys.command_keys["custom"], "Z");
    assert!(
        std::str::from_utf8(&files["presentation.ron"])
            .unwrap()
            .contains("key:\"D\"")
    );
    let once = files.clone();
    apply(&mut files, &table()).unwrap();
    assert_eq!(files, once);
}

#[test]
fn hotkeys_reject_invalid_references_and_offsets_before_publishing() {
    let valid = table();
    assert!(hotkey(&valid, 0).is_err());
    assert!(hotkey(&valid, 701).is_err());
    let mut invalid = valid.clone();
    invalid[1172..1174].copy_from_slice(&1_u16.to_le_bytes());
    assert!(hotkey(&invalid, 586).is_err());
    let offset = usize::from(u16::from_le_bytes(valid[1172..1174].try_into().unwrap()));
    let mut invalid = valid.clone();
    invalid[offset] = b'1';
    let mut files = Files::from([("presentation.ron".into(), b"(schema_version:1,)".to_vec())]);
    let before = files.clone();
    assert!(apply(&mut files, &invalid).is_err());
    assert_eq!(files, before);
    assert!(hotkey(&valid[..valid.len() - 1], 700).is_err());
}

#[test]
#[ignore = "requires privately extracted original executable/stat_txt.tbl"]
fn hotkeys_match_original_retail_button_data() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/source/starcraft");
    let table = std::fs::read(root.join("rez_stat_txt.tbl")).unwrap();
    let executable = std::fs::read(root.join("files_starcraft.exe")).unwrap();
    let mut files = Files::from([("presentation.ron".into(), b"(schema_version:1,)".to_vec())]);
    apply(&mut files, &table).unwrap();
    let keys: Keys = ron::de::from_bytes(&files["presentation.ron"]).unwrap();
    for (id, expected) in [
        (1, "M"),
        (2, "S"),
        (11, "F"),
        (19, "G"),
        (20, "V"),
        (21, "G"),
        (22, "T"),
        (23, "W"),
        (53, "D"),
        (6, "Z"),
        (7, "H"),
        (27, "D"),
        (28, "O"),
        (29, "M"),
    ] {
        assert_eq!(keys.train_keys[&UnitTypeId(id)], expected);
    }
    for (id, expected) in [
        (1, "W"),
        (2, "A"),
        (3, "U"),
        (4, "T"),
        (5, "C"),
        (6, "A"),
        (7, "I"),
        (8, "M"),
    ] {
        assert_eq!(keys.research_keys[&ResearchId(id)], expected);
    }
    for (name, expected) in [
        ("move", "M"),
        ("stop", "S"),
        ("attack", "A"),
        ("patrol", "P"),
        ("hold", "H"),
        ("lift", "L"),
        ("land", "L"),
        ("rally", "R"),
        ("gather", "G"),
        ("repair", "R"),
        ("build", "B"),
        ("advanced-build", "V"),
        ("unload", "U"),
        ("back", "Esc"),
        ("cancel", "Esc"),
        ("stim", "T"),
        ("mine", "I"),
        ("scan", "S"),
    ] {
        assert_eq!(keys.command_keys[name], expected);
    }
    // The executable uses these table IDs for the actual worker build rows.
    for (start, length) in [(0xe4f18, 8), (0xe4fd0, 4)] {
        for row in 0..length {
            let offset = start + row * 20;
            let source =
                u16::from_le_bytes(executable[offset + 14..offset + 16].try_into().unwrap());
            let string =
                u16::from_le_bytes(executable[offset + 16..offset + 18].try_into().unwrap());
            assert!(
                BUILD
                    .iter()
                    .any(|&(id, _, _, label)| id == source && label == u32::from(string))
            );
        }
    }
    // Parent-building addon positions must not collide with research rows.
    for parent in [106, 113, 114, 116] {
        let header = 0xe5cf0 + parent * 12;
        let count = u32::from_le_bytes(executable[header..header + 4].try_into().unwrap());
        let pointer = u32::from_le_bytes(executable[header + 4..header + 8].try_into().unwrap());
        for index in 0..count as usize {
            let row = &executable[pointer as usize - 0x402200 + index * 20..][..20];
            if u32::from_le_bytes(row[8..12].try_into().unwrap()) == 0x473130 {
                let source = u16::from_le_bytes(row[14..16].try_into().unwrap());
                let slot = u16::from_le_bytes(row[..2].try_into().unwrap()) as u8 - 1;
                let string = u16::from_le_bytes(row[16..18].try_into().unwrap());
                assert!(ADDON.contains(&(source, slot, u32::from(string))));
            }
        }
    }
    assert_eq!(hotkey(&table, 372).unwrap(), "U");
    assert_eq!(hotkey(&table, 373).unwrap(), "U");
    assert_eq!(hotkey(&table, 344).unwrap(), "C");
    assert_eq!(hotkey(&table, 345).unwrap(), "C");
}
