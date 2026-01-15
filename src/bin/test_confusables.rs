use confusables::Confusable;

fn are_nicks_confusable(nick1: &str, nick2: &str) -> bool {
    nick1.is_confusable_with(nick2) || nick2.is_confusable_with(nick1)
}

fn main() {
    let latin = "evan";
    let cyrillic = "еvan";

    println!("Testing confusables:");
    println!("  latin: {} (U+0065...)", latin);
    println!("  cyrillic: {} (U+0435...)", cyrillic);

    println!("\nDirect tests:");
    println!(
        "  latin.is_confusable_with(cyrillic): {}",
        latin.is_confusable_with(cyrillic)
    );
    println!(
        "  cyrillic.is_confusable_with(latin): {}",
        cyrillic.is_confusable_with(latin)
    );
    println!(
        "  are_nicks_confusable(latin, cyrillic): {}",
        are_nicks_confusable(latin, cyrillic)
    );

    println!(
        "\nlatin.detect_replace_confusable(): {}",
        latin.detect_replace_confusable()
    );
    println!(
        "cyrillic.detect_replace_confusable(): {}",
        cyrillic.detect_replace_confusable()
    );
}
