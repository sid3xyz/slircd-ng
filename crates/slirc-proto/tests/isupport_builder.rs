use slirc_proto::isupport::{ChanModesBuilder, TargMaxBuilder};

#[test]
fn test_chanmodes_builder() {
    let modes = ChanModesBuilder::new()
        .list_modes("beI")
        .param_always("k")
        .param_set("l")
        .no_param("imnst");

    assert_eq!(modes.build(), "beI,k,l,imnst");
}

#[test]
#[should_panic(expected = "Duplicate channel mode character 'k' found in CHANMODES")]
fn test_chanmodes_duplicate_cross_category() {
    ChanModesBuilder::new().param_always("k").no_param("k"); // Should panic
}

#[test]
#[should_panic(expected = "Duplicate channel mode character 'a' found in input string")]
fn test_chanmodes_duplicate_same_string() {
    ChanModesBuilder::new().list_modes("aa"); // Should panic
}

#[test]
fn test_targmax_builder() {
    let targmax = TargMaxBuilder::new()
        .add("JOIN", 10)
        .add_unlimited("PRIVMSG");

    // Order is preserved
    assert_eq!(targmax.build(), "JOIN:10,PRIVMSG:");
}
