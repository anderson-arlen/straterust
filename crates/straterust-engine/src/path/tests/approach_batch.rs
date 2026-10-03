use super::*;

#[test]
fn batched_entrances_preserve_first_reachable_path_after_disconnected_candidates() {
    let mut map = map(16, 12);
    for y in 0..12 {
        block(&mut map, 8, y);
    }
    let start = Position { x: 12, y: 12 };
    let targets = [
        Position { x: 100, y: 20 },
        Position { x: 108, y: 72 },
        Position { x: 44, y: 76 },
    ];
    let footprint = Footprint {
        width: 4,
        height: 4,
    };
    for class in [MovementClass::Ground, MovementClass::Air] {
        let expected = targets.iter().find_map(|&target| {
            find_path(&map, footprint, class, start, target, &[]).map(|path| (target, path))
        });
        assert_eq!(
            find_path_to_any(&map, footprint, class, start, &targets, &[]),
            expected
        );
    }
    assert!(
        find_path_to_any(
            &map,
            footprint,
            MovementClass::Ground,
            start,
            &targets[..2],
            &[]
        )
        .is_none()
    );
}
