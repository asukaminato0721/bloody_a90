use bloody_a90::gestures::direction4;
use std::{
    fs::File,
    io::{BufRead, BufReader},
};

fn check(row: &str) -> u32 {
    let fields: Vec<_> = row.split_whitespace().collect();
    assert_eq!(fields.len(), 5, "{row}");
    let control: u32 = fields[0].parse().unwrap();
    assert!(matches!(control, 0x027f | 0x037f));
    let x = fields[1].parse().unwrap();
    let y = fields[2].parse().unwrap();
    let expected: u8 = fields[3].parse().unwrap();
    let angle = f64::from_bits(u64::from_str_radix(fields[4], 16).unwrap());
    assert!(angle.is_finite());
    assert_eq!(
        direction4(x, y),
        expected,
        "native reference {row}; angle={angle:.17}"
    );
    control
}

#[test]
fn native_direction_samples_include_diagonal_boundaries() {
    let mut count = 0;
    for row in include_str!("fixtures/gesture-directions.tsv").lines() {
        if row.starts_with('#') || row.starts_with("control") || row.is_empty() {
            continue;
        }
        assert_eq!(check(row), 0x037f);
        count += 1;
    }
    assert!(count >= 250);
}

#[test]
#[ignore = "requires A90_DIRECTION_REFERENCE from the original pure-math probe"]
fn full_original_direction_reference() {
    let path = std::env::var_os("A90_DIRECTION_REFERENCE").expect("set A90_DIRECTION_REFERENCE");
    let mut counts = [0usize; 2];
    for row in BufReader::new(File::open(path).unwrap()).lines() {
        let row = row.unwrap();
        if row.starts_with("control") {
            continue;
        }
        counts[usize::from(check(&row) == 0x027f)] += 1;
    }
    assert_eq!(counts, [1_181_184, 1_181_184]);
}
