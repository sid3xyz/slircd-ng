use slirc_proto::Command;

#[test]
fn test_sjoin_roundtrip() {
    let users = vec![
        ("@".to_string(), "001AAAAAA".to_string()),
        ("".to_string(), "001AAAAAB".to_string()),
        ("+".to_string(), "001AAAAAC".to_string()),
    ];
    let cmd = Command::SJOIN(
        1234567890,
        "#test".to_string(),
        "+nt".to_string(),
        vec!["key".to_string()],
        users,
    );

    let serialized = cmd.to_string();
    assert_eq!(
        serialized,
        "SJOIN 1234567890 #test +nt key :@001AAAAAA 001AAAAAB +001AAAAAC"
    );

    // Note: The IRC parser strips the leading ':' from the trailing parameter.
    // So we pass the user list without the leading colon here.
    let parsed = Command::new(
        "SJOIN",
        vec![
            "1234567890",
            "#test",
            "+nt",
            "key",
            "@001AAAAAA 001AAAAAB +001AAAAAC",
        ],
    )
    .unwrap();
    assert_eq!(cmd, parsed);
}

#[test]
fn test_tmode_roundtrip() {
    let cmd = Command::TMODE(
        1234567890,
        "#test".to_string(),
        "+o".to_string(),
        vec!["001AAAAAA".to_string()],
    );

    let serialized = cmd.to_string();
    assert_eq!(serialized, "TMODE 1234567890 #test +o 001AAAAAA");

    let parsed = Command::new("TMODE", vec!["1234567890", "#test", "+o", "001AAAAAA"]).unwrap();
    assert_eq!(cmd, parsed);
}
