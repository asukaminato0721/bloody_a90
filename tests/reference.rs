use bloody_a90::macros::{self, Repeat};

fn decode_hex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn prior_python_compiler_vectors() {
    for line in include_str!("fixtures/macro-reference.tsv").lines() {
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 6);
        let repeat = match fields[1] {
            "0" => Repeat::Once,
            "1" => Repeat::Held,
            "2" => Repeat::Toggle,
            _ => unreachable!(),
        };
        let program = macros::compile(
            &fields[2].replace("\\n", "\n"),
            &fields[3].replace("\\n", "\n"),
            repeat,
        )
        .unwrap();
        assert_eq!(
            macros::bytes(&program.press),
            decode_hex(fields[4]),
            "{} press",
            fields[0]
        );
        assert_eq!(
            macros::bytes(&program.release),
            decode_hex(fields[5]),
            "{} release",
            fields[0]
        );
    }
}
