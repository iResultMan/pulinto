//! The active variant is process-wide state, so this lives in a test binary of
//! its own: a unit test flipping it would change what every other test in the
//! same process reads.

use pulinto_core::layout::{convert, guess_layout, use_variants, Layout};

#[test]
fn the_active_variant_decides_which_table_by_id_returns() {
    assert_eq!(Layout::by_id("ru").unwrap().variant, "");

    use_variants([("us", "mac"), ("ru", "mac")]);
    let us = Layout::by_id("us").unwrap();
    let ru = Layout::by_id("ru").unwrap();
    assert_eq!(ru.variant, "mac");
    assert_eq!(ru.id, "ru", "the identifier stays the language's");
    // `us(mac)` has no table of its own: nothing to switch.
    assert_eq!(us.variant, "");
    assert_eq!(convert("ghbdtn&", us, ru), "привет.");
    assert_eq!(guess_layout("привет").unwrap().variant, "mac");

    // A variant we know nothing about leaves the standard table in place.
    use_variants([("ru", "dvorak-or-whatever")]);
    assert_eq!(Layout::by_id("ru").unwrap().variant, "");

    use_variants([]);
    assert_eq!(convert("ghbdtn&", us, Layout::by_id("ru").unwrap()), "привет?");
}
