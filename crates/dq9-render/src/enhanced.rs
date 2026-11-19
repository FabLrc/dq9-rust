//! The enhanced presentation's switches (`--fx`/`--no-fx`, the page's checkboxes), one boolean
//! per effect of the 16:9 view (`docs/app/screens.md`, § "Enhanced 16:9").
//!
//! The post pass reads the post effects as a bit mask (`post.wgsl`, [`PostBits`]); mipmapped
//! textures, the single-pixel edge accent and the HUD's glass panels are decided on the CPU and
//! are not part of that mask.

/// One switch per effect of the enhanced presentation. Every field is on in [`Self::DEFAULT`]
/// except [`Self::contact_shadows`], which is opt-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnhancedOptions {
    /// Mipmapped, linearly sampled textures instead of the DS's nearest filtering.
    pub trilinear: bool,
    /// FXAA-lite in the post pass.
    pub fxaa: bool,
    /// Screen-space contact shadows read from the depth buffer.
    pub contact_shadows: bool,
    /// Threshold bloom.
    pub bloom: bool,
    /// Filmic (ACES) tonemapping mixed with the DS colours.
    pub tonemapping: bool,
    /// Cool-shadow, warm-highlight colour grading with saturation and contrast.
    pub grading: bool,
    /// Vignette.
    pub vignette: bool,
    /// Dither against the 8-bit steps of the sky gradients.
    pub dithering: bool,
    /// Edge marking as a single physical pixel; off draws it at the DS's own width.
    pub edge_accent: bool,
    /// Translucent panels behind the HUD and the field's message window.
    pub glass: bool,
}

impl Default for EnhancedOptions {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl EnhancedOptions {
    /// The enhanced presentation as it starts: every effect except the contact shadows, which
    /// the original has no equivalent of (no post-process; its soft dark rim around the
    /// entities reads as a thick outline next to the DS's single-pixel edge marking). Opt in
    /// with `--fx contact-shadows` (Web: `?fx=contact-shadows` or the page's checkbox).
    pub const DEFAULT: Self = Self {
        contact_shadows: false,
        ..Self::ALL
    };

    /// Every effect on: what `--fx all` and `?fx=all` select.
    pub const ALL: Self = Self {
        trilinear: true,
        fxaa: true,
        contact_shadows: true,
        bloom: true,
        tonemapping: true,
        grading: true,
        vignette: true,
        dithering: true,
        edge_accent: true,
        glass: true,
    };

    /// Every effect off: the DS look of the field in the layout's frame.
    pub const NONE: Self = Self {
        trilinear: false,
        fxaa: false,
        contact_shadows: false,
        bloom: false,
        tonemapping: false,
        grading: false,
        vignette: false,
        dithering: false,
        edge_accent: false,
        glass: false,
    };

    /// The names `--fx`/`--no-fx` and `?fx=`/`?nofx=` accept, in the order of the fields.
    pub const NAMES: [&'static str; 10] = [
        "trilinear",
        "fxaa",
        "contact-shadows",
        "bloom",
        "tonemapping",
        "grading",
        "vignette",
        "dithering",
        "edge-accent",
        "glass",
    ];

    /// Whether the switch `name` is on, for the page's checkboxes.
    pub fn is_on(&self, name: &str) -> Option<bool> {
        Some(match name {
            "trilinear" => self.trilinear,
            "fxaa" => self.fxaa,
            "contact-shadows" => self.contact_shadows,
            "bloom" => self.bloom,
            "tonemapping" => self.tonemapping,
            "grading" => self.grading,
            "vignette" => self.vignette,
            "dithering" => self.dithering,
            "edge-accent" => self.edge_accent,
            "glass" => self.glass,
            _ => return None,
        })
    }

    /// The field `name` switches, for [`EnhancedOptions::set`].
    fn get(&mut self, name: &str) -> Option<&mut bool> {
        Some(match name {
            "trilinear" => &mut self.trilinear,
            "fxaa" => &mut self.fxaa,
            "contact-shadows" => &mut self.contact_shadows,
            "bloom" => &mut self.bloom,
            "tonemapping" => &mut self.tonemapping,
            "grading" => &mut self.grading,
            "vignette" => &mut self.vignette,
            "dithering" => &mut self.dithering,
            "edge-accent" => &mut self.edge_accent,
            "glass" => &mut self.glass,
            _ => return None,
        })
    }

    /// Switches one effect on (`on`) or off: a name, or `all` / `none` for every effect.
    pub fn set(&mut self, name: &str, on: bool) -> Result<(), String> {
        match name {
            "all" => *self = if on { Self::ALL } else { Self::NONE },
            "none" => *self = if on { Self::NONE } else { Self::ALL },
            _ => match self.get(name) {
                Some(flag) => *flag = on,
                None => {
                    return Err(format!(
                        "expected one of {}, all or none, not `{name}`",
                        Self::NAMES.join(", ")
                    ));
                }
            },
        }
        Ok(())
    }

    /// Applies a comma-separated list (empty parts ignored): the syntax of `--fx`,
    /// `--no-fx`, `?fx=` and `?nofx=`.
    pub fn apply_list(&mut self, list: &str, on: bool) -> Result<(), String> {
        for name in list.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            self.set(name, on)?;
        }
        Ok(())
    }

    /// The post pass's bit mask (the union of [`PostBits`] it switches on).
    pub fn post_mask(&self) -> u32 {
        let bits = [
            (self.fxaa, PostBits::FXAA),
            (self.contact_shadows, PostBits::CONTACT_SHADOWS),
            (self.bloom, PostBits::BLOOM),
            (self.tonemapping, PostBits::TONEMAPPING),
            (self.grading, PostBits::GRADING),
            (self.vignette, PostBits::VIGNETTE),
            (self.dithering, PostBits::DITHERING),
        ];
        bits.iter()
            .filter(|(on, _)| *on)
            .fold(0, |mask, (_, bit)| mask | bit.0)
    }

    /// Whether the post pass has anything to do: with every post effect off the field is drawn
    /// directly (the DS look).
    pub fn post_effects(&self) -> bool {
        self.post_mask() != 0
    }
}

/// Bits of the post pass's mask, as `post.wgsl` reads them. Each value is also declared in the
/// shader with the same name (`crates/dq9-render/tests/shaders.rs` checks the two agree).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostBits(pub u32);

impl PostBits {
    pub const FXAA: Self = Self(1);
    pub const CONTACT_SHADOWS: Self = Self(2);
    pub const BLOOM: Self = Self(4);
    pub const TONEMAPPING: Self = Self(8);
    pub const GRADING: Self = Self(16);
    pub const VIGNETTE: Self = Self(32);
    pub const DITHERING: Self = Self(64);

    /// The WGSL constant name of each bit, in the shader's spelling.
    pub const WGSL_NAMES: [(&'static str, Self); 7] = [
        ("FXAA", Self::FXAA),
        ("CONTACT_SHADOWS", Self::CONTACT_SHADOWS),
        ("BLOOM", Self::BLOOM),
        ("TONEMAPPING", Self::TONEMAPPING),
        ("GRADING", Self::GRADING),
        ("VIGNETTE", Self::VIGNETTE),
        ("DITHERING", Self::DITHERING),
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_has_every_effect_except_contact_shadows() {
        assert_eq!(EnhancedOptions::default(), EnhancedOptions::DEFAULT);
        let default = EnhancedOptions::DEFAULT;
        assert!(!default.contact_shadows);
        let all = EnhancedOptions::ALL;
        assert!(all.contact_shadows);
        assert_eq!(
            default,
            EnhancedOptions {
                contact_shadows: false,
                ..EnhancedOptions::ALL
            }
        );
    }

    #[test]
    fn all_selects_the_contact_shadows_and_none_is_everything_off() {
        let all = EnhancedOptions::ALL;
        assert!(all.contact_shadows);
        assert_eq!(
            EnhancedOptions::ALL.post_mask(),
            PostBits::FXAA.0
                | PostBits::CONTACT_SHADOWS.0
                | PostBits::BLOOM.0
                | PostBits::TONEMAPPING.0
                | PostBits::GRADING.0
                | PostBits::VIGNETTE.0
                | PostBits::DITHERING.0
        );
        assert_eq!(EnhancedOptions::NONE.post_mask(), 0);
        assert!(!EnhancedOptions::NONE.post_effects());
    }

    #[test]
    fn the_enhanced_default_skips_the_contact_shadows_alone() {
        let default = EnhancedOptions::default();
        let mask = default.post_mask();
        assert_eq!(mask & PostBits::CONTACT_SHADOWS.0, 0);
        assert_eq!(
            mask,
            PostBits::FXAA.0
                | PostBits::BLOOM.0
                | PostBits::TONEMAPPING.0
                | PostBits::GRADING.0
                | PostBits::VIGNETTE.0
                | PostBits::DITHERING.0
        );
        assert!(default.post_effects());
    }

    #[test]
    fn contact_shadows_are_opt_in_and_all_still_selects_them() {
        let mut options = EnhancedOptions::default();
        options.apply_list("contact-shadows", true).unwrap();
        assert_eq!(options, EnhancedOptions::ALL);
        let mut options = EnhancedOptions::default();
        options.set("all", true).unwrap();
        assert_eq!(options, EnhancedOptions::ALL);
        options.apply_list("contact-shadows", false).unwrap();
        assert_eq!(options, EnhancedOptions::DEFAULT);
    }

    #[test]
    fn lists_switch_effects_off_and_on() {
        let mut options = EnhancedOptions::ALL;
        options.apply_list("fxaa, bloom", false).unwrap();
        assert!(!options.fxaa);
        assert!(!options.bloom);
        assert!(options.trilinear);
        assert_eq!(options.post_mask() & PostBits::FXAA.0, 0);
        options.apply_list("bloom", true).unwrap();
        assert!(options.bloom);
        options.apply_list("none", true).unwrap();
        assert_eq!(options, EnhancedOptions::NONE);
        options.apply_list("all", true).unwrap();
        assert_eq!(options, EnhancedOptions::ALL);
        options.apply_list("all", false).unwrap();
        assert_eq!(options, EnhancedOptions::NONE);
        assert!(options.set("cinema", true).is_err());
        assert!(options.apply_list("fxaa,cinema", true).is_err());
    }

    #[test]
    fn the_names_are_the_fields_and_the_bits_are_distinct() {
        let mut options = EnhancedOptions::NONE;
        for name in EnhancedOptions::NAMES {
            options.set(name, true).unwrap();
        }
        assert_eq!(options, EnhancedOptions::ALL);
        let mut seen = 0;
        for (_, bit) in PostBits::WGSL_NAMES {
            assert_eq!(seen & bit.0, 0, "bit {} reused", bit.0);
            seen |= bit.0;
            assert!(bit.0.is_power_of_two());
        }
        assert_eq!(seen, EnhancedOptions::ALL.post_mask());
    }
}
