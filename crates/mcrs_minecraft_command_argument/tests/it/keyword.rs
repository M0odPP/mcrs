use crate::{OP, read};
use mcrs_command::Span;
use mcrs_command::SyntaxError;
use mcrs_minecraft_command::ResourceError;
use mcrs_minecraft_command_argument::{
    EntityAnchorArgument, GameModeArgument, HeightmapArgument, KeywordError,
    ResourceLocationArgument, SwingAnimationArgument, SwingAnimationName,
};
use mcrs_minecraft_game_mode::GameMode;
use mcrs_minecraft_resource::ResourceLocation;

#[test]
fn identifiers_default_their_namespace() {
    let id = |namespace, path| ResourceLocation::new(namespace, path).unwrap();
    let cases = [
        ("stone", Ok(id("minecraft", "stone")), 5),
        ("Stone", Ok(id("minecraft", "")), 0),
        (
            "a:b:c",
            Err(SyntaxError::argument(ResourceError::IdInvalid, 0)),
            0,
        ),
        (":x", Ok(id("minecraft", "x")), 2),
        ("stone[x]", Ok(id("minecraft", "stone")), 5),
    ];
    for (input, expected, cursor) in cases {
        assert_eq!(
            read(&ResourceLocationArgument, input, &OP),
            (expected, cursor),
            "{input}"
        );
    }
}

#[test]
fn keywords_fail_at_their_own_positions() {
    assert_eq!(
        read(&GameModeArgument, "creative", &OP).0,
        Ok(GameMode::Creative)
    );
    assert_eq!(
        read(&GameModeArgument, "Creative", &OP).0,
        Err(SyntaxError::argument(
            KeywordError::GameModeInvalid(Span::new(0, 8)),
            8
        ))
    );
    assert_eq!(
        read(&GameModeArgument, "creativ", &OP).0,
        Err(SyntaxError::argument(
            KeywordError::GameModeInvalid(Span::new(0, 7)),
            7
        ))
    );
    assert_eq!(
        read(&EntityAnchorArgument, "foo", &OP),
        (
            Err(SyntaxError::argument(
                KeywordError::AnchorInvalid(Span::new(0, 3)),
                0
            )),
            0
        )
    );
}

#[test]
fn heightmap_reads_only_the_lower_case_kept_types() {
    use mcrs_minecraft_core::HeightmapName;
    assert_eq!(
        read(&HeightmapArgument, "ocean_floor", &OP).0,
        Ok(HeightmapName::OceanFloor)
    );
    for input in ["WORLD_SURFACE", "world_surface_wg", "1", ""] {
        let end = input.len();
        assert_eq!(
            read(&HeightmapArgument, input, &OP),
            (
                Err(SyntaxError::argument(
                    KeywordError::EnumInvalid(Span::new(0, end)),
                    end
                )),
                end
            ),
            "{input}"
        );
    }
}

#[test]
fn swing_animation_accepts_only_lowercase_none_whack_stab_and_suggests_case_insensitively() {
    use mcrs_command::SuggestionBuilder;
    use mcrs_minecraft_command::error_message;
    use mcrs_minecraft_text::Text;

    for (input, expected) in [
        ("none", SwingAnimationName::None),
        ("whack", SwingAnimationName::Whack),
        ("stab", SwingAnimationName::Stab),
    ] {
        assert_eq!(
            read(&SwingAnimationArgument, input, &OP),
            (Ok(expected), input.len())
        );
    }
    for bad in ["WHACK", "bad_anim"] {
        let end = bad.len();
        let (err, cursor) = read(&SwingAnimationArgument, bad, &OP);
        assert_eq!(
            err,
            Err(SyntaxError::argument(
                KeywordError::SwingAnimationInvalid(Span::new(0, end)),
                end
            ))
        );
        assert_eq!(cursor, end);
    }
    let (err, cursor) = read(&SwingAnimationArgument, "@", &OP);
    let err = err.unwrap_err();
    assert_eq!(cursor, 0);
    assert_eq!(
        error_message("@", &err),
        Text::translate("argument.swing_animation.invalid", vec!["".into()])
    );

    let suggest = |remaining: &str| -> Vec<String> {
        let mut out = Vec::new();
        let mut builder = SuggestionBuilder::<()>::new(remaining, 0, &mut out);
        SwingAnimationArgument::suggest(&mut builder);
        out.into_iter()
            .map(|suggestion| {
                let text: String = suggestion.text.chars().collect();
                text
            })
            .collect()
    };
    assert_eq!(suggest(""), vec!["none", "stab", "whack"]);
    assert_eq!(suggest("s"), vec!["stab"]);
    assert_eq!(suggest("S"), vec!["stab"]);
}
