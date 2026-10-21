use super::*;
use crate::{
    assets::{AssetManifest, AssetPack, Image, ImageRef, encode_image},
    game::GameInfo,
    media::{AudioRef, MediaManifest, MediaPack, encode_wav},
    menus::{MenuManifest, MenuPack},
};

#[test]
fn maps_and_menus_resolve_shared_hashes_without_file_aliases_and_keep_native_media() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let one = root.join("maps/one");
    let two = root.join("maps/two");
    fs::create_dir_all(&one).unwrap();
    fs::create_dir_all(&two).unwrap();
    let image = Image {
        width: 2,
        height: 3,
        rgba: vec![127; 24],
    };
    let bytes = encode_image(&image).unwrap();
    let image_hash = blake3::hash(&bytes).to_hex().to_string();
    let wav = encode_wav(1, 12000, &[10, -10, 20, -20]).unwrap();
    let audio_hash = blake3::hash(&wav).to_hex().to_string();
    let input = root.join("input");
    let duplicate = root.join("independent-copy");
    fs::write(&input, &bytes).unwrap();
    fs::write(&duplicate, &bytes).unwrap();
    let archive = root.join("shared-assets.sras");
    let mut writer = Writer::create(&archive).unwrap();
    writer.copy(&input).unwrap();
    writer.copy(&duplicate).unwrap();
    fs::write(&input, &wav).unwrap();
    writer.copy(&input).unwrap();
    writer.finish().unwrap();
    assert_eq!(
        fs::metadata(&archive).unwrap().len(),
        HEADER + (bytes.len() + wav.len()) as u64 + 2 * ENTRY as u64
    );
    let info = GameInfo {
        schema_version: 1,
        version: "1.0".into(),
        importer: None,
        icon: None,
        asset_archive: Some("shared-assets.sras".into()),
    };
    fs::write(root.join("game.ron"), ron::ser::to_string(&info).unwrap()).unwrap();
    for (directory, name) in [(&one, "one.srim"), (&two, "different-name.srim")] {
        let image_ref = ImageRef {
            file: name.into(),
            blake3: image_hash.clone(),
        };
        let encoded = ron::ser::to_string(&image_ref).unwrap();
        let assets: AssetManifest = ron::from_str(&format!("(schema_version:1,terrain:{encoded},unit_type:1,unit_name:\"Test\",frame_ms:100,anchor:(0,0),frames:[{encoded}])")).unwrap();
        fs::write(
            directory.join("assets.ron"),
            ron::ser::to_string(&assets).unwrap(),
        )
        .unwrap();
        let mut media: MediaManifest = ron::from_str("(schema_version:1)").unwrap();
        media.music.push(AudioRef {
            file: "music.wav".into(),
            blake3: audio_hash.clone(),
        });
        fs::write(
            directory.join("media.ron"),
            ron::ser::to_string(&media).unwrap(),
        )
        .unwrap();
        let mut menu = MenuManifest::basic("Test", false);
        menu.background = Some(image_ref);
        menu.music = media.music.clone();
        fs::write(
            directory.join("menus.ron"),
            ron::ser::to_string(&menu).unwrap(),
        )
        .unwrap();
        assert!(!directory.join(name).exists());
        let loaded = AssetPack::load(directory).unwrap().unwrap();
        assert_eq!(loaded.terrain, image);
        assert_eq!(loaded.frames[0], image);
        assert_eq!(
            MediaPack::load(directory).unwrap().unwrap().music[0]
                .samples
                .as_ref(),
            &[10, -10, 20, -20]
        );
        assert_eq!(
            MenuPack::load(directory).unwrap().unwrap().images[name],
            image
        );
    }
    let reader = Reader::open(&one).unwrap();
    assert!(reader.read("../escape", &image_hash, 100).is_err());
    assert!(reader.read("one.srim", &image_hash, 1).is_err());
    let outside = tempfile::tempdir().unwrap();
    assert!(
        reader
            .for_directory(outside.path())
            .unwrap()
            .read("one.srim", &image_hash, 100)
            .is_err()
    );
    let original = fs::read(&archive).unwrap();
    let mut corrupt = original.clone();
    corrupt[HEADER as usize] ^= 1;
    fs::write(&archive, corrupt).unwrap();
    assert!(
        reader
            .read("one.srim", &image_hash, 100)
            .unwrap_err()
            .to_string()
            .contains("hash mismatch")
    );
    let mut corrupt = original.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    fs::write(&archive, corrupt).unwrap();
    assert!(Reader::open(&one).is_err());
    let mut corrupt = original.clone();
    corrupt[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
    fs::write(&archive, corrupt).unwrap();
    assert!(Reader::open(&one).is_err());
    fs::write(&archive, &original[..original.len() - 1]).unwrap();
    assert!(Reader::open(&one).is_err());
}
