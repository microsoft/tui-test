use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use super::style::FontFamilies;

pub(crate) const FAMILY: &str = "JetBrains Mono";

#[cfg(feature = "recording-font-jetbrains-mono")]
const REGULAR_FONTS: &[&[u8]] = &[include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/jetbrains-mono/JetBrainsMono-Regular.ttf"
))];

#[cfg(feature = "recording-font-jetbrains-mono-styles")]
const STYLED_FONTS: &[&[u8]] = &[
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/jetbrains-mono/JetBrainsMono-Bold.ttf"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/jetbrains-mono/JetBrainsMono-Italic.ttf"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/jetbrains-mono/JetBrainsMono-BoldItalic.ttf"
    )),
];

#[cfg(feature = "recording-font-jetbrains-mono-full")]
const FULL_FAMILY_FONTS: &[&[u8]] = &[
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/jetbrains-mono/JetBrainsMono-Thin.ttf"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/jetbrains-mono/JetBrainsMono-ThinItalic.ttf"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/jetbrains-mono/JetBrainsMono-ExtraLight.ttf"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/jetbrains-mono/JetBrainsMono-ExtraLightItalic.ttf"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/jetbrains-mono/JetBrainsMono-Light.ttf"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/jetbrains-mono/JetBrainsMono-LightItalic.ttf"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/jetbrains-mono/JetBrainsMono-Medium.ttf"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/jetbrains-mono/JetBrainsMono-MediumItalic.ttf"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/jetbrains-mono/JetBrainsMono-SemiBold.ttf"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/jetbrains-mono/JetBrainsMono-SemiBoldItalic.ttf"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/jetbrains-mono/JetBrainsMono-ExtraBold.ttf"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/jetbrains-mono/JetBrainsMono-ExtraBoldItalic.ttf"
    )),
];

pub(crate) struct Catalog {
    pub database: Arc<fontdb::Database>,
    candidates: [Vec<fontdb::ID>; 4],
    nerd_faces: Vec<fontdb::ID>,
}

impl Catalog {
    pub fn candidates(&self, bold: bool, italic: bool, character: char) -> Vec<fontdb::ID> {
        let mut output = Vec::new();
        let mut seen = HashSet::new();
        if super::nerd_font::is_private_use(character) {
            output.extend(
                self.nerd_faces
                    .iter()
                    .copied()
                    .filter(|id| seen.insert(*id)),
            );
        }
        output.extend(
            self.candidates[style_index(bold, italic)]
                .iter()
                .copied()
                .filter(|id| seen.insert(*id)),
        );
        output
    }
}

/// The bundled, nerd and system faces. Scanning the system is the expensive
/// part and does not depend on the style, so it happens once for the process;
/// a style naming its own font files gets a clone of this with those files
/// added rather than paying for the scan again.
fn base_database() -> &'static fontdb::Database {
    static BASE: OnceLock<fontdb::Database> = OnceLock::new();
    BASE.get_or_init(|| {
        let mut database = fontdb::Database::new();
        load_bundled_fonts(&mut database);
        database.load_font_data(super::nerd_font::FONT_DATA.to_vec());
        database.load_system_fonts();
        database
    })
}

/// The catalog to draw a style with. Which faces win depends on the families
/// that style names, so the ordering cannot be computed once for the process.
/// Keyed by the style's fonts: a run uses one, or one per profile.
pub(crate) fn catalog_for(font: &FontFamilies) -> Arc<Catalog> {
    static CATALOGS: OnceLock<Mutex<HashMap<FontFamilies, Arc<Catalog>>>> = OnceLock::new();
    let mut catalogs = CATALOGS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("font catalog cache");
    if let Some(existing) = catalogs.get(font) {
        return Arc::clone(existing);
    }
    // Built while holding the lock. Ranking every face is slow enough that
    // letting concurrent callers each build their own copy costs far more
    // than making them wait for the one that will be cached anyway.
    let catalog = Arc::new(build_catalog(font));
    catalogs.insert(font.clone(), Arc::clone(&catalog));
    catalog
}

fn build_catalog(font: &FontFamilies) -> Catalog {
    let mut database = base_database().clone();
    for path in &font.files {
        // A path that names nothing is not an error, for the same reason a
        // family no face provides is not: the style falls back.
        if path.is_dir() {
            database.load_fonts_dir(path);
        } else {
            let _ = database.load_font_file(path);
        }
    }

    let candidates = std::array::from_fn(|index| {
        let bold = index & 1 != 0;
        let italic = index & 2 != 0;
        let preferred = preferred_families(font, bold, italic);
        let mut faces = database.faces().collect::<Vec<_>>();
        faces.sort_by(|left, right| {
            face_score(left, &preferred, bold, italic)
                .partial_cmp(&face_score(right, &preferred, bold, italic))
                .unwrap_or(Ordering::Equal)
        });
        faces.into_iter().map(|face| face.id).collect()
    });
    let nerd_faces = database
        .faces()
        .filter(|face| {
            face.families
                .iter()
                .any(|(family, _)| family.contains("Nerd Font"))
        })
        .map(|face| face.id)
        .collect();
    Catalog {
        database: Arc::new(database),
        candidates,
        nerd_faces,
    }
}

fn load_bundled_fonts(database: &mut fontdb::Database) {
    #[cfg(feature = "recording-font-jetbrains-mono")]
    load_font_data(database, REGULAR_FONTS);
    #[cfg(feature = "recording-font-jetbrains-mono-styles")]
    load_font_data(database, STYLED_FONTS);
    #[cfg(feature = "recording-font-jetbrains-mono-full")]
    load_font_data(database, FULL_FAMILY_FONTS);
    #[cfg(not(feature = "recording-font-jetbrains-mono"))]
    let _ = database;
}

#[cfg(feature = "recording-font-jetbrains-mono")]
fn load_font_data(database: &mut fontdb::Database, fonts: &[&[u8]]) {
    for font in fonts {
        database.load_font_data(font.to_vec());
    }
}

fn style_index(bold: bool, italic: bool) -> usize {
    usize::from(bold) | (usize::from(italic) << 1)
}

fn preferred_families(font: &FontFamilies, bold: bool, italic: bool) -> Vec<String> {
    // A style that names its own family outranks the environment variable,
    // which exists for reaching the catalog when no config can. A style that
    // names none leaves the variable exactly as authoritative as it was.
    let default_family = FontFamilies::default().family;
    // A family is a CSS font stack, because that is what the SVG path passes
    // straight into font-family. Split it so face selection sees the same
    // preference order the SVG does rather than one unmatchable string.
    let named = [font.resolve(bold, italic), font.family.as_str()]
        .into_iter()
        .filter(|family| *family != default_family)
        .flat_map(|family| family.split(','))
        .map(|family| family.trim().trim_matches(['\'', '"']).to_string())
        .filter(|family| !family.is_empty() && family != "monospace")
        .collect::<Vec<_>>();
    let configured = std::env::var("TUI_TEST_RECORDING_FONT_FAMILIES")
        .ok()
        .into_iter()
        .flat_map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        });
    named
        .into_iter()
        .chain(configured)
        .chain(
            [
                FAMILY,
                "Cascadia Mono",
                "Cascadia Code",
                "Consolas",
                "Menlo",
                "DejaVu Sans Mono",
                "Noto Sans Mono",
                "Segoe UI Emoji",
                "Segoe UI Symbol",
                "Noto Sans CJK SC",
                "Microsoft YaHei UI",
                "Yu Gothic UI",
                "Malgun Gothic",
                "PingFang SC",
                "Apple Color Emoji",
                "Noto Color Emoji",
            ]
            .into_iter()
            .map(str::to_string),
        )
        .fold(Vec::new(), |mut families, family| {
            if !families
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(&family))
            {
                families.push(family);
            }
            families
        })
}

fn face_score(
    face: &fontdb::FaceInfo,
    preferred: &[String],
    bold: bool,
    italic: bool,
) -> (usize, usize, usize, u16, usize) {
    let family = preferred
        .iter()
        .position(|preferred| {
            face.families
                .iter()
                .any(|(family, _)| family.eq_ignore_ascii_case(preferred))
        })
        .unwrap_or(if face.monospaced { 1_000 } else { 2_000 });
    let exact_jetbrains_style = usize::from(!is_exact_jetbrains_style(face, bold, italic));
    let wants_italic = italic;
    let is_italic = face.style != fontdb::Style::Normal;
    let style = usize::from(wants_italic != is_italic);
    let desired_weight = if bold {
        fontdb::Weight::BOLD.0
    } else {
        fontdb::Weight::NORMAL.0
    };
    (
        family,
        exact_jetbrains_style,
        style,
        face.weight.0.abs_diff(desired_weight),
        usize::from(!face.monospaced),
    )
}

fn is_exact_jetbrains_style(face: &fontdb::FaceInfo, bold: bool, italic: bool) -> bool {
    if !face
        .families
        .iter()
        .any(|(family, _)| family.eq_ignore_ascii_case(FAMILY))
    {
        return false;
    }
    let expected = match (bold, italic) {
        (false, false) => "JetBrainsMono-Regular",
        (true, false) => "JetBrainsMono-Bold",
        (false, true) => "JetBrainsMono-Italic",
        (true, true) => "JetBrainsMono-BoldItalic",
    };
    face.post_script_name.eq_ignore_ascii_case(expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_contains_the_bundled_nerd_face() {
        let catalog = catalog_for(&FontFamilies::default());
        assert!(!catalog.nerd_faces.is_empty());
    }

    #[test]
    fn selected_font_tier_has_the_expected_face_count() {
        let expected = if cfg!(feature = "recording-font-jetbrains-mono-full") {
            16
        } else if cfg!(feature = "recording-font-jetbrains-mono-styles") {
            4
        } else if cfg!(feature = "recording-font-jetbrains-mono") {
            1
        } else {
            0
        };
        assert_eq!(bundled_font_count(), expected);
    }

    #[cfg(feature = "recording-font-jetbrains-mono")]
    #[test]
    fn bundled_regular_font_is_the_full_official_face() {
        let face = ttf_parser::Face::parse(REGULAR_FONTS[0], 0).unwrap();
        assert!(face.number_of_glyphs() >= 1_700);
    }

    #[cfg(feature = "recording-font-jetbrains-mono-styles")]
    #[test]
    fn styled_tier_contains_bold_italic_and_bold_italic_faces() {
        let metadata = font_metadata(STYLED_FONTS);
        assert!(metadata.contains("JetBrainsMono-Bold"));
        assert!(metadata.contains("JetBrainsMono-Italic"));
        assert!(metadata.contains("JetBrainsMono-BoldItalic"));
    }

    #[cfg(feature = "recording-font-jetbrains-mono-full")]
    #[test]
    fn full_tier_contains_every_remaining_static_family_face() {
        let metadata = font_metadata(FULL_FAMILY_FONTS);
        assert_eq!(metadata.len(), 12);
        for face in [
            "JetBrainsMono-Thin",
            "JetBrainsMono-ThinItalic",
            "JetBrainsMono-ExtraLight",
            "JetBrainsMono-ExtraLightItalic",
            "JetBrainsMono-Light",
            "JetBrainsMono-LightItalic",
            "JetBrainsMono-Medium",
            "JetBrainsMono-MediumItalic",
            "JetBrainsMono-SemiBold",
            "JetBrainsMono-SemiBoldItalic",
            "JetBrainsMono-ExtraBold",
            "JetBrainsMono-ExtraBoldItalic",
        ] {
            assert!(metadata.contains(face));
        }
    }

    #[cfg(feature = "recording-font-jetbrains-mono")]
    #[test]
    fn bundled_font_includes_its_ofl_license_and_source() {
        let license = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/jetbrains-mono/OFL.txt"
        ));
        let source = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/jetbrains-mono/SOURCE"
        ));
        assert!(license.contains("SIL OPEN FONT LICENSE"));
        assert!(source.contains("JetBrains/JetBrainsMono"));
        assert!(source.contains("full-glyph"));
    }

    fn bundled_font_count() -> usize {
        let count = 0;
        #[cfg(feature = "recording-font-jetbrains-mono")]
        let count = count + REGULAR_FONTS.len();
        #[cfg(feature = "recording-font-jetbrains-mono-styles")]
        let count = count + STYLED_FONTS.len();
        #[cfg(feature = "recording-font-jetbrains-mono-full")]
        let count = count + FULL_FAMILY_FONTS.len();
        count
    }

    #[cfg(feature = "recording-font-jetbrains-mono-styles")]
    fn font_metadata(fonts: &[&[u8]]) -> HashSet<String> {
        let mut database = fontdb::Database::new();
        load_font_data(&mut database, fonts);
        database
            .faces()
            .map(|face| face.post_script_name.clone())
            .collect()
    }

    /// The raster path used to pick faces with no reference to the style, so a
    /// screenshot and a recording of the same session disagreed about the font.
    #[test]
    fn a_configured_family_outranks_the_environment_and_the_defaults() {
        let font = FontFamilies {
            family: "Berkeley Mono".into(),
            bold: Some("Berkeley Mono Bold".into()),
            ..FontFamilies::default()
        };

        let plain = preferred_families(&font, false, false);
        assert_eq!(plain.first().map(String::as_str), Some("Berkeley Mono"));

        // A family is a CSS font stack, so face selection has to read it the
        // same way the SVG does rather than as one unmatchable string.
        let stack = preferred_families(
            &FontFamilies {
                family: "'Berkeley Mono', Menlo, monospace".into(),
                ..FontFamilies::default()
            },
            false,
            false,
        );
        assert_eq!(
            &stack[..2],
            &["Berkeley Mono".to_string(), "Menlo".to_string()],
            "quotes and spacing are stripped and the generic name dropped"
        );

        let bold = preferred_families(&font, true, false);
        assert_eq!(
            bold.first().map(String::as_str),
            Some("Berkeley Mono Bold"),
            "a bold run prefers the family named for it"
        );
        assert_eq!(
            bold.get(1).map(String::as_str),
            Some("Berkeley Mono"),
            "and falls back to the base family before anything built in"
        );

        // A style naming nothing must leave the existing order alone, which is
        // what keeps TUI_TEST_RECORDING_FONT_FAMILIES authoritative.
        let default = preferred_families(&FontFamilies::default(), false, false);
        assert_eq!(default.first().map(String::as_str), Some(FAMILY));
    }

    #[test]
    fn catalogs_are_cached_per_style_and_differ_between_them() {
        let one = catalog_for(&FontFamilies::default());
        let again = catalog_for(&FontFamilies::default());
        assert!(
            Arc::ptr_eq(&one, &again),
            "the same fonts reuse the catalog rather than rescanning"
        );

        let other = catalog_for(&FontFamilies {
            family: "Berkeley Mono".into(),
            ..FontFamilies::default()
        });
        assert!(
            !Arc::ptr_eq(&one, &other),
            "and different fonts get their own"
        );
    }
}
