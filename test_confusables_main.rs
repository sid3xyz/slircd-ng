use confusables::Confusable;

fn main() {
    let latin = "evan";
    let cyrillic = "еvan";
    
    println!("Latin 'evan': {}", latin);
    println!("Cyrillic 'еvan' (with U+0435): {}", cyrillic);
    
    println!("\nConfusables methods:");
    println!("latin.is_confusable_with(cyrillic): {}", latin.is_confusable_with(cyrillic));
    println!("cyrillic.is_confusable_with(latin): {}", cyrillic.is_confusable_with(latin));
    
    println!("\nlatin.detect_replace_confusable(): {}", latin.detect_replace_confusable());
    println!("cyrillic.detect_replace_confusable(): {}", cyrillic.detect_replace_confusable());
    
    println!("\nlatin.contains_confusable(): {}", latin.contains_confusable());
    println!("cyrillic.contains_confusable(): {}", cyrillic.contains_confusable());
}
