use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

pub(crate) const FAMILY: &str = "JetBrains Mono";

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

pub(crate) fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let mut database = fontdb::Database::new();
        database.load_font_data(super::nerd_font::FONT_DATA.to_vec());
        database.load_system_fonts();

        let preferred = preferred_families();
        let candidates = std::array::from_fn(|index| {
            let bold = index & 1 != 0;
            let italic = index & 2 != 0;
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
    })
}

fn style_index(bold: bool, italic: bool) -> usize {
    usize::from(bold) | (usize::from(italic) << 1)
}

fn preferred_families() -> Vec<String> {
    let configured = std::env::var("SHELL_USE_RECORDING_FONT_FAMILIES")
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
    configured
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
) -> (usize, usize, u16, usize) {
    let family = preferred
        .iter()
        .position(|preferred| {
            face.families
                .iter()
                .any(|(family, _)| family.eq_ignore_ascii_case(preferred))
        })
        .unwrap_or(if face.monospaced { 1_000 } else { 2_000 });
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
        style,
        face.weight.0.abs_diff(desired_weight),
        usize::from(!face.monospaced),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_contains_the_bundled_nerd_face() {
        let catalog = catalog();
        assert!(!catalog.nerd_faces.is_empty());
    }
}
