//! Colour, done in a fixed order.
//!
//! Transcribed from someone who grades for a living, and the useful part isn't
//! any single adjustment — it's that **the order is fixed**. The reason people
//! flounder learning colour is that every tool is available at once and
//! nothing tells you which to reach for. A node tree you always build the same
//! way removes that entirely.
//!
//! Five nodes, always the same five, always in this order. Anything you can't
//! fix inside them wasn't a colour problem.

use serde::{Deserialize, Serialize};

/// One node in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Node {
    /// Colour space transform in. Turns flat log footage into something with
    /// normal contrast before you touch anything.
    TransformIn,
    /// Get the brightness right, watching the waveform rather than the image.
    Exposure,
    /// Fix the colour cast, using skin as the reference.
    Balance,
    Contrast,
    Saturation,
    /// Colour space transform out, to whatever you're delivering.
    TransformOut,
}

impl Node {
    pub fn what(&self) -> &'static str {
        match self {
            Node::TransformIn => "turn the log footage into something normal to look at",
            Node::Exposure => "get the brightness right",
            Node::Balance => "take the colour cast out",
            Node::Contrast => "add the contrast back",
            Node::Saturation => "bring the colour up",
            Node::TransformOut => "convert to what you're delivering",
        }
    }

    /// What to look at while doing it. Every one of these is a scope rather
    /// than the picture, which is the habit that separates grading from
    /// fiddling.
    pub fn watch(&self) -> &'static str {
        match self {
            Node::TransformIn => "nothing — it's a conversion, not a choice",
            Node::Exposure => "the waveform, not the image",
            Node::Balance => "the vectorscope, with the skin tone line showing",
            Node::Contrast => "the waveform, so you don't crush the blacks or clip the whites",
            Node::Saturation => "the image, but change it in the skin tones only",
            Node::TransformOut => "nothing — it's a conversion",
        }
    }

    /// The specific thing people get wrong here.
    pub fn the_mistake(&self) -> Option<&'static str> {
        match self {
            Node::TransformIn => Some("grading the log footage directly, which fights you the whole way"),
            Node::Exposure => Some("judging brightness by eye on an uncalibrated screen"),
            Node::Balance => Some(
                "balancing to a white object rather than to skin — skin is what the eye judges \
                 everything else against",
            ),
            Node::Contrast => Some("crushing the blacks, which is the single commonest thing that makes footage look cheap"),
            Node::Saturation => Some(
                "raising saturation globally, which turns skin orange before anything else gets \
                 more colourful",
            ),
            Node::TransformOut => None,
        }
    }
}

/// The tree, always the same.
pub fn tree() -> Vec<Node> {
    vec![
        Node::TransformIn,
        Node::Exposure,
        Node::Balance,
        Node::Contrast,
        Node::Saturation,
        Node::TransformOut,
    ]
}

/// Set the project up before touching a clip.
///
/// Getting these wrong makes every later step fight you, and they're set once
/// and forgotten.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectSetup {
    /// Colour science.
    pub science: &'static str,
    /// The space you work in.
    pub timeline_space: &'static str,
    /// The space you deliver in.
    pub output_space: &'static str,
    /// The neutral point contrast pivots around.
    ///
    /// In a wide working space this is not 0.5 — using the default is why
    /// contrast pushes everything dark.
    pub contrast_pivot: f32,
}

pub fn recommended_setup() -> ProjectSetup {
    ProjectSetup {
        science: "DaVinci YRGB Color Managed",
        timeline_space: "DaVinci Wide Gamut / Intermediate",
        output_space: "Rec.709 Gamma 2.4",
        // The number from the video, and the one nobody mentions.
        contrast_pivot: 0.336,
    }
}

/// What Atlas checks about a grade.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Measured {
    /// Fraction of the frame at pure black.
    pub crushed_black: f32,
    /// At pure white.
    pub clipped_white: f32,
    /// Where the skin cluster sits on the vectorscope, as degrees off the
    /// skin tone line. Positive is toward red.
    pub skin_off_line: f32,
    pub saturation: f32,
    /// Saturation was raised globally rather than in skin tones.
    pub raised_globally: bool,
}

/// Notes on a grade, in the order of the tree so they're fixable in one pass.
pub fn check(m: &Measured) -> Vec<(Node, String)> {
    let mut out = Vec::new();

    if m.crushed_black > 0.02 {
        out.push((
            Node::Contrast,
            format!(
                "{:.0}% of the frame is at pure black. Lift the black point — contrast comes \
                 from the curve, and crushing is what makes footage look cheap.",
                m.crushed_black * 100.0
            ),
        ));
    }
    if m.clipped_white > 0.01 {
        out.push((
            Node::Contrast,
            format!("{:.0}% is clipped white and there's no detail in it.", m.clipped_white * 100.0),
        ));
    }
    // The one that matters most and is invisible without the scope.
    if m.skin_off_line.abs() > 6.0 {
        out.push((
            Node::Balance,
            format!(
                "skin is {:.0}° off the line — {} on the vectorscope. Move the global colour \
                 wheel until the cluster sits on it.",
                m.skin_off_line,
                if m.skin_off_line > 0.0 { "too red" } else { "too green" }
            ),
        ));
    }
    if m.raised_globally && m.saturation > 1.1 {
        out.push((
            Node::Saturation,
            "saturation is up globally, which turns skin orange before anything else gets more \
             colourful. Use colour slice and bring it up in the skin tones only."
                .into(),
        ));
    }

    // Ordered by the tree, so fixing them is one pass down the nodes rather
    // than jumping around.
    let order = tree();
    out.sort_by_key(|(n, _)| order.iter().position(|x| x == n).unwrap_or(9));
    out
}

/// What Atlas says.
pub fn spoken(notes: &[(Node, String)]) -> String {
    match notes.first() {
        None => "Grade looks clean.".into(),
        Some((node, why)) => {
            let mut s = format!("On the {} node: {why}", node_name(*node));
            if notes.len() > 1 {
                s.push_str(&format!(" {} more, further down the tree.", notes.len() - 1));
            }
            s
        }
    }
}

fn node_name(n: Node) -> &'static str {
    match n {
        Node::TransformIn => "transform in",
        Node::Exposure => "exposure",
        Node::Balance => "balance",
        Node::Contrast => "contrast",
        Node::Saturation => "saturation",
        Node::TransformOut => "transform out",
    }
}

/// Why a fixed order rather than a set of tips.
pub const WHY_A_FIXED_TREE: &str =
    "The reason colour is hard to learn isn't that any one adjustment is difficult — it's that \
     every tool is available at once and nothing tells you which to reach for. The same five \
     nodes in the same order every time removes that, and anything you can't fix inside them \
     wasn't a colour problem.";

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GradingConfig {
    pub enabled: bool,
    /// How far skin can sit off the line before it's worth saying.
    pub skin_tolerance_degrees: f32,
    /// Fraction of frame at pure black before it counts as crushed.
    pub crush_limit: f32,
}

impl Default for GradingConfig {
    fn default() -> Self {
        GradingConfig { enabled: false, skin_tolerance_degrees: 6.0, crush_limit: 0.02 }
    }
}
