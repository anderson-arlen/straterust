use super::*;

#[test]
#[cfg(unix)]
fn repeated_image_and_audio_references_use_one_checked_payload_but_verify_each_digest() {
    let temporary = tempfile::tempdir().unwrap();
    let one = temporary.path().join("one");
    let two = temporary.path().join("two");
    fs::create_dir(&one).unwrap();
    fs::create_dir(&two).unwrap();
    let image = crate::assets::encode_image(&crate::assets::Image {
        width: 2,
        height: 3,
        rgba: vec![127; 24],
    })
    .unwrap();
    let wav = crate::media::encode_wav(1, 12000, &[10, -10, 20, -20]).unwrap();
    fs::write(one.join("image.srim"), &image).unwrap();
    fs::write(one.join("audio.wav"), &wav).unwrap();
    fs::hard_link(one.join("image.srim"), two.join("image.srim")).unwrap();
    fs::hard_link(one.join("audio.wav"), two.join("audio.wav")).unwrap();
    let mut image_ref = ImageRef {
        file: "image.srim".into(),
        blake3: blake3::hash(&image).to_hex().to_string(),
    };
    let mut audio_ref = AudioRef {
        file: "audio.wav".into(),
        blake3: blake3::hash(&wav).to_hex().to_string(),
    };
    let mut validation = Validation::default();
    for _ in 0..32 {
        for root in [&one, &two] {
            assert_eq!(validation.image(root, &image_ref).unwrap().rgba_bytes, 24);
            assert_eq!(validation.audio(root, &audio_ref).unwrap().pcm_bytes, 8);
        }
    }
    assert_eq!(validation.images.len(), 1);
    assert_eq!(validation.audio.len(), 1);
    image_ref.blake3 = "0".repeat(64);
    audio_ref.blake3 = "0".repeat(64);
    assert!(
        validation
            .image(&two, &image_ref)
            .unwrap_err()
            .to_string()
            .contains("hash mismatch")
    );
    assert!(
        validation
            .audio(&two, &audio_ref)
            .unwrap_err()
            .to_string()
            .contains("hash mismatch")
    );
    // An independently corrupted file with the same declared hash cannot reuse
    // the checked data of another file.
    fs::remove_file(two.join("image.srim")).unwrap();
    let mut corrupt = image.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    fs::write(two.join("image.srim"), &corrupt).unwrap();
    image_ref.blake3 = blake3::hash(&image).to_hex().to_string();
    assert!(validation.image(&two, &image_ref).is_err());
}

#[test]
fn snapshot_validation_preserves_native_canvas_hash_and_audio_checks() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let image = crate::assets::encode_image(&crate::assets::Image {
        width: 2,
        height: 3,
        rgba: vec![127; 24],
    })
    .unwrap();
    fs::write(root.join("image.srim"), &image).unwrap();
    let reference = ImageRef {
        file: "image.srim".into(),
        blake3: blake3::hash(&image).to_hex().to_string(),
    };
    let encoded = ron::ser::to_string(&reference).unwrap();
    let mut assets: AssetManifest = ron::from_str(&format!("(schema_version:1,terrain:{encoded},unit_type:1,unit_name:\"Test\",frame_ms:100,anchor:(0,0),frames:[{encoded}])")).unwrap();
    let publish = |assets: &AssetManifest| {
        fs::write(
            root.join("assets.ron"),
            ron::ser::to_string(assets).unwrap(),
        )
        .unwrap()
    };
    publish(&assets);
    let wav = crate::media::encode_wav(1, 12000, &[10, -10]).unwrap();
    fs::write(root.join("audio.wav"), &wav).unwrap();
    let mut media: MediaManifest = ron::from_str("(schema_version:1)").unwrap();
    media.music.push(AudioRef {
        file: "audio.wav".into(),
        blake3: blake3::hash(&wav).to_hex().to_string(),
    });
    fs::write(root.join("media.ron"), ron::ser::to_string(&media).unwrap()).unwrap();
    Validation::default().directory(root).unwrap();
    assets.anchor = [10000, 10000];
    publish(&assets);
    assert!(
        Validation::default()
            .directory(root)
            .unwrap_err()
            .to_string()
            .contains("anchor")
    );
    assets.anchor = [0, 0];
    assets.frames[0].blake3 = "0".repeat(64);
    publish(&assets);
    assert!(
        Validation::default()
            .directory(root)
            .unwrap_err()
            .to_string()
            .contains("hash mismatch")
    );
    media.music[0].blake3 = "0".repeat(64);
    fs::write(root.join("media.ron"), ron::ser::to_string(&media).unwrap()).unwrap();
    assets.frames[0].blake3 = reference.blake3;
    publish(&assets);
    assert!(
        Validation::default()
            .directory(root)
            .unwrap_err()
            .to_string()
            .contains("hash mismatch")
    );
}
