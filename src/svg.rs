//! The process as a BPMN diagram, in SVG.
//!
//! A process is a tree of sequence, parallel, choice and nature nodes over
//! tasks, and its diagram is planar by construction: a sequence lays its two
//! regions left to right on one line, and a gateway node opens a split, lays
//! its two branches one above the other and closes them with a join. Every
//! element carries the identifier of its node, so that a strategy can later be
//! shown on the drawing by lighting the branches it takes.
//!
//! The layout is one recursion. Every node owns a box whose flow line runs
//! through its vertical middle: a task is a box of fixed height, a sequence is
//! its regions side by side with their middles aligned, a gateway node is a
//! diamond, the two branches stacked, a diamond. Because every box is centred
//! on its flow line, the connectors of a split leave its top and bottom
//! vertices and never cross anything, and the whole drawing is planar without
//! any search.

use std::collections::HashMap;

use crate::num::decimal;
use crate::tree::{Kind, Tree};

/// The width of a task box follows its text, its height its lines.
const TASK_MIN_W: f64 = 104.0;
/// The gateway diamond, vertex to vertex.
const GW: f64 = 36.0;
/// Between two elements on one line, and between a diamond and its branches.
const GAP: f64 = 30.0;
/// Between the two branches of a gateway.
const VGAP: f64 = 20.0;
/// The radius of the start and end events.
const EVENT: f64 = 11.0;
const PAD: f64 = 18.0;
/// In the detailed drawing, the margin a region keeps around its content,
/// inside its dashed box, wide enough for the identifier in its corner.
const REGION_M: f64 = 14.0;
/// What the drawing shows of a task beyond its name and duration.
#[derive(Clone, Debug, Default)]
pub struct Style {
    /// Every region in a dashed box with its identifier, and the identifier
    /// among the data of every task.
    pub detailed: bool,
    /// The impacts of a task as a tooltip on its box rather than as a line
    /// in it, which keeps a box narrow when the impacts are many.
    pub tooltips: bool,
    /// The impacts of a task as bars in its box, these components in this
    /// order, every bar against the greatest value of its impact over the
    /// tasks of the process; the values go to the tooltip.
    pub bars: Option<Vec<usize>>,
    /// The duration of a task as a clock in the corner of its box, the disc
    /// filled by the duration against the longest of the process, whole for
    /// the longest; the number goes to the tooltip.
    pub clocks: bool,
    /// The names beside what is drawn: the name of its impact at the left of
    /// every bar, and the duration under the clock.
    pub names: bool,
    /// The identifier every node shows, by identifier, when the process is
    /// an assembly of saved processes: `7-3` for the node 3 of the process
    /// the leaf 7 refers to. The bare identifiers otherwise.
    pub labels: Option<Vec<String>>,
    /// The saved processes spliced into the drawing, each collapsed into one
    /// box or expanded inside a bold one.
    pub subs: Vec<SubBox>,
}

/// One saved process in the drawing.
#[derive(Clone, Debug)]
pub struct SubBox {
    /// The identifier of its root in the assembled tree.
    pub root: u32,
    /// The identifier it shows, that of the leaf referring to it.
    pub label: String,
    /// The name it is saved under.
    pub name: String,
    /// The name it is shown under.
    pub shown: String,
    pub expanded: bool,
}
/// The square with the plus or the minus on the border of a sub-process.
const TOGGLE: f64 = 14.0;
const CLOCK_R: f64 = 7.0;
/// The column the clock takes at the right of a box, and its height with
/// the number under it.
const CLOCK_COL: f64 = 2.0 * (CLOCK_R + 6.0);
const CLOCK_H: f64 = 2.0 * CLOCK_R + 6.0;
const CLOCK_H_NAMED: f64 = CLOCK_H + 12.0;
/// The bars: the length of the longest, the height of a row, the bar in it.
const BAR_LEN: f64 = 60.0;
const ROW_H: f64 = 10.0;
const BAR_T: f64 = 7.0;

/// The colour of a bar is its heat: the value against the greatest value of
/// its impact over the process, from a cold blue through yellow to a fire
/// red at the maximum. The impact a bar stands for is its place, the bars
/// keeping the alphabetical order of the names in every box.
fn heat(t: f64) -> String {
    let t = t.clamp(0.0, 1.0);
    let (a, b, u) = if t < 0.5 {
        ((0x3b, 0x82, 0xf6), (0xf5, 0xc5, 0x42), t * 2.0)
    } else {
        ((0xf5, 0xc5, 0x42), (0xe8, 0x23, 0x2a), (t - 0.5) * 2.0)
    };
    let mix = |x: u8, y: u8| -> u8 { (x as f64 + (y as f64 - x as f64) * u).round() as u8 };
    format!("#{:02x}{:02x}{:02x}", mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}
const LEGEND_H: f64 = 22.0;

struct Sizes {
    w: Vec<f64>,
    h: Vec<f64>,
    style: Style,
    /// The greatest value of every impact over the tasks, for the bars.
    maxes: Vec<f64>,
    /// The longest duration over the tasks, for the clocks.
    longest: i16,
    /// The sub-process rooted at a node, by the identifier of the root.
    subs: HashMap<u32, usize>,
}

impl Sizes {
    fn detailed(&self) -> bool {
        self.style.detailed
    }
    fn sub(&self, id: u32) -> Option<&SubBox> {
        self.subs.get(&id).map(|&i| &self.style.subs[i])
    }
    /// A collapsed sub-process: drawn as one box, its tree left out.
    fn collapsed(&self, id: u32) -> bool {
        matches!(self.sub(id), Some(b) if !b.expanded)
    }
    /// An expanded sub-process: its tree inside a bold box.
    fn expanded(&self, id: u32) -> bool {
        matches!(self.sub(id), Some(b) if b.expanded)
    }
    /// The identifier a node shows.
    fn label(&self, id: u32) -> String {
        match &self.style.labels {
            Some(l) => match l.get(id as usize) {
                Some(s) if !s.is_empty() => s.clone(),
                _ => id.to_string(),
            },
            None => id.to_string(),
        }
    }
}

/// The lines of the box of a collapsed sub-process: the name it is shown
/// under, the saved process it refers to, and its identifier in the
/// detailed drawing.
fn sub_lines(s: &Sizes, id: u32) -> Vec<(&'static str, String)> {
    let b = s.sub(id).expect("a collapsed sub-process");
    let mut lines = vec![("name", b.shown.clone()), ("detail", format!("@{}", b.name))];
    if s.style.detailed {
        // the identifier of the leaf that refers to it, not of its root
        lines.push(("detail id", format!("id {}", b.label)));
    }
    lines
}

/// The lines of a task box, the name first and in a larger type: the
/// identifier in the detailed drawing, the duration, the impacts unless they
/// are a tooltip.
fn task_box_lines(tree: &Tree, id: u32, s: &Sizes) -> (Vec<(&'static str, String)>, String) {
    let (name, duration, impacts) = task_lines(tree, id);
    let mut lines: Vec<(&'static str, String)> = vec![("name", name)];
    if s.style.detailed {
        lines.push(("detail id", format!("id {}", s.label(id))));
    }
    if !s.style.clocks {
        lines.push(("detail", duration.clone()));
    }
    if !s.style.tooltips && s.style.bars.is_none() {
        lines.push(("detail", impacts.clone()));
    }
    // the tooltip carries what the box does not show
    let mut tip = Vec::new();
    if s.style.clocks {
        tip.push(duration);
    }
    if s.style.tooltips || s.style.bars.is_some() {
        tip.push(if impacts.is_empty() { "no impact".to_string() } else { impacts });
    }
    (lines, tip.join(" · "))
}

/// The height of a task box of `n` lines, with its rows of bars when it
/// has them, and never shorter than its clock column.
fn task_h(n: usize, s: &Sizes) -> f64 {
    let rows = match &s.style.bars {
        Some(b) if !b.is_empty() => b.len() as f64 * ROW_H + 6.0,
        _ => 0.0,
    };
    let h = 16.0 + 14.0 * n as f64 + rows;
    let clock = if !s.style.clocks {
        0.0
    } else if s.style.names {
        CLOCK_H_NAMED + 10.0
    } else {
        CLOCK_H + 8.0
    };
    h.max(clock)
}

/// The width the widest name at the left of the bars takes, when the names
/// are shown.
fn label_w(tree: &Tree, s: &Sizes) -> f64 {
    match (&s.style.bars, s.style.names) {
        (Some(b), true) if !b.is_empty() => {
            let names = impact_names(tree);
            b.iter().map(|&c| text_w(&names[c], 9.5)).fold(0.0, f64::max) + 6.0
        }
        _ => 0.0,
    }
}

/// The width the rows of bars of a task need, name and bar.
fn bars_w(tree: &Tree, s: &Sizes) -> f64 {
    match &s.style.bars {
        Some(b) if !b.is_empty() => label_w(tree, s) + BAR_LEN + 24.0,
        _ => 0.0,
    }
}

/// The impacts in alphabetical order of their names, as the page shows them.
pub fn alphabetical(tree: &Tree) -> Vec<usize> {
    let names = impact_names(tree);
    let mut order: Vec<usize> = (0..tree.k).collect();
    order.sort_by(|&a, &b| names[a].cmp(&names[b]));
    order
}

/// The names of the impacts, an index where the process gives none.
pub fn impact_names(tree: &Tree) -> Vec<String> {
    (0..tree.k)
        .map(|j| match tree.meta.impact_names.get(j) {
            Some(n) if !n.is_empty() => n.clone(),
            _ => format!("impact {j}"),
        })
        .collect()
}

/// How far inside its box the first and last element of a node sit: the
/// margin of the bold box of an expanded sub-process, of the dashed box of
/// a region in the detailed drawing, nothing otherwise and nothing for a
/// task or a collapsed sub-process, which have no box around them.
fn inset(tree: &Tree, id: u32, s: &Sizes) -> f64 {
    if s.collapsed(id) {
        0.0
    } else if s.expanded(id) || (s.detailed() && tree.node(id).kind != Kind::Task) {
        REGION_M
    } else {
        0.0
    }
}

/// The three lines of a task box: the name, the duration, the impacts that
/// are not zero, named as the instance names them.
fn task_lines(tree: &Tree, id: u32) -> (String, String, String) {
    let name = tree
        .task_names
        .get(id as usize)
        .cloned()
        .unwrap_or_default();
    let duration = format!("duration {}", tree.node(id).duration);
    let names = &tree.meta.impact_names;
    // the impacts in alphabetical order of their names, as the page shows
    // them everywhere
    let mut parts: Vec<(String, String)> = Vec::new();
    for (j, r) in tree.impact_exact(id).iter().enumerate() {
        if r.numer().sign() == num_bigint::Sign::NoSign {
            continue;
        }
        let label = names
            .get(j)
            .cloned()
            .unwrap_or_else(|| format!("impact {j}"));
        parts.push((label, decimal(r, 6)));
    }
    parts.sort_by(|a, b| a.0.cmp(&b.0));
    let line: Vec<String> = parts.into_iter().map(|(l, v)| format!("{l} {v}")).collect();
    (name, duration, line.join(" · "))
}

/// An estimate of the width of a text at a font size, enough to size a box.
fn text_w(s: &str, px: f64) -> f64 {
    s.chars().count() as f64 * px * 0.56
}

/// The room a nature node leaves between its split and its branches: the
/// gap, or enough for the longer of its two probability labels to sit on the
/// connector without touching the branch.
fn lead(tree: &Tree, id: u32) -> f64 {
    if tree.node(id).kind != Kind::Nature {
        return GAP;
    }
    let p = tree.prob_exact(id);
    let q = num_rational::BigRational::from_integer(1.into()) - p;
    let widest = text_w(&decimal(p, 6), 11.0).max(text_w(&decimal(&q, 6), 11.0));
    GAP.max(widest + 16.0)
}

fn measure(tree: &Tree, id: u32, s: &mut Sizes) {
    let n = tree.node(id);
    if s.collapsed(id) {
        // one box, like a task, its tree left out
        let lines = sub_lines(s, id);
        let text = lines
            .iter()
            .map(|(class, text)| text_w(text, if *class == "name" { 13.0 } else { 11.0 }))
            .fold(TASK_MIN_W, f64::max)
            + 24.0;
        s.w[id as usize] = text;
        s.h[id as usize] = 16.0 + 14.0 * lines.len() as f64 + 6.0;
        return;
    }
    // the margin of the box around the node, the same on every side
    let m = inset(tree, id, s);
    let (w, h) = match n.kind {
        Kind::Task => {
            let (lines, _) = task_box_lines(tree, id, s);
            let text = lines
                .iter()
                .map(|(class, text)| text_w(text, if *class == "name" { 13.0 } else { 11.0 }))
                .fold(TASK_MIN_W, f64::max)
                + 24.0;
            let clock = if s.style.clocks { CLOCK_COL } else { 0.0 };
            (text.max(bars_w(tree, s)) + clock, task_h(lines.len(), s))
        }
        Kind::Sequence => {
            measure(tree, n.low, s);
            measure(tree, n.high, s);
            (
                s.w[n.low as usize] + GAP + s.w[n.high as usize],
                s.h[n.low as usize].max(s.h[n.high as usize]),
            )
        }
        Kind::Parallel | Kind::Choice | Kind::Nature => {
            measure(tree, n.low, s);
            measure(tree, n.high, s);
            (
                GW + lead(tree, id) + s.w[n.low as usize].max(s.w[n.high as usize]) + GAP + GW,
                s.h[n.low as usize] + VGAP + s.h[n.high as usize],
            )
        }
    };
    s.w[id as usize] = w + 2.0 * m;
    s.h[id as usize] = h + 2.0 * m;
}

fn xml(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c => o.push(c),
        }
    }
    o
}

/// A sequence flow: a polyline through the points, an arrowhead at its end.
fn flow(out: &mut String, from: &str, to: &str, points: &[(f64, f64)]) {
    let mut d = String::new();
    for (i, (x, y)) in points.iter().enumerate() {
        d.push_str(if i == 0 { "M" } else { " L" });
        d.push_str(&format!(" {x:.1} {y:.1}"));
    }
    out.push_str(&format!(
        "<path class=\"flow\" data-from=\"{from}\" data-to=\"{to}\" d=\"{d}\" marker-end=\"url(#arrow)\"/>\n"
    ));
}

/// A gateway diamond centred at `(cx, cy)`, with the mark of its kind.
fn diamond(out: &mut String, id: u32, role: &str, kind: Kind, cx: f64, cy: f64) {
    let r = GW / 2.0;
    out.push_str(&format!(
        "<g class=\"gateway {}\" data-id=\"{id}\" data-role=\"{role}\">\n<polygon points=\"{:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1}\"/>\n",
        match kind {
            Kind::Parallel => "parallel",
            Kind::Choice => "choice",
            _ => "nature",
        },
        cx, cy - r, cx + r, cy, cx, cy + r, cx - r, cy
    ));
    let m = 7.0;
    match kind {
        Kind::Parallel => out.push_str(&format!(
            "<path class=\"mark\" d=\"M {:.1} {:.1} L {:.1} {:.1} M {:.1} {:.1} L {:.1} {:.1}\"/>\n",
            cx - m, cy, cx + m, cy, cx, cy - m, cx, cy + m
        )),
        Kind::Choice => out.push_str(&format!(
            "<path class=\"mark\" d=\"M {:.1} {:.1} L {:.1} {:.1} M {:.1} {:.1} L {:.1} {:.1}\"/>\n",
            cx - m * 0.75, cy - m * 0.75, cx + m * 0.75, cy + m * 0.75,
            cx - m * 0.75, cy + m * 0.75, cx + m * 0.75, cy - m * 0.75
        )),
        _ => out.push_str(&format!(
            "<circle class=\"mark\" cx=\"{cx:.1}\" cy=\"{cy:.1}\" r=\"{:.1}\"/>\n<circle class=\"dot\" cx=\"{cx:.1}\" cy=\"{cy:.1}\" r=\"2.2\"/>\n",
            m - 0.5
        )),
    }
    out.push_str("</g>\n");
}

/// The dashed box of a region in the detailed drawing, its identifier in
/// the corner.
fn region_box(out: &mut String, id: u32, label: &str, x: f64, y: f64, w: f64, h: f64) {
    out.push_str(&format!(
        "<g class=\"region\" data-id=\"{id}\">\n<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{w:.1}\" height=\"{h:.1}\" rx=\"10\" ry=\"10\"/>\n\
         <text class=\"rid\" x=\"{:.1}\" y=\"{:.1}\">{}</text>\n</g>\n",
        x + 7.0, y + 11.0, xml(label)
    ));
}

/// The square on the bottom border of a sub-process, with a plus when the
/// process is collapsed and a minus when it is expanded: the page toggles
/// the one for the other on a click.
fn toggle(out: &mut String, label: &str, cx: f64, cy: f64, expanded: bool) {
    let r = TOGGLE / 2.0;
    out.push_str(&format!(
        "<g class=\"toggle\" data-toggle=\"{}\"><title>{}</title>\n<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{TOGGLE}\" height=\"{TOGGLE}\" rx=\"2\"/>\n\
         <text x=\"{cx:.1}\" y=\"{:.1}\">{}</text>\n</g>\n",
        xml(label),
        if expanded { "collapse" } else { "expand" },
        cx - r, cy - r, cy + 4.5,
        if expanded { "\u{2212}" } else { "+" }
    ));
}

/// A collapsed sub-process: one box with the name it is shown under, the
/// saved process it refers to, and the plus on its border.
fn draw_collapsed(id: u32, x: f64, y: f64, s: &Sizes, out: &mut String) {
    let b = s.sub(id).expect("a collapsed sub-process");
    let (w, h) = (s.w[id as usize], s.h[id as usize]);
    out.push_str(&format!(
        "<g class=\"sub collapsed\" data-id=\"{id}\" data-sub=\"{}\">\n<title>sub-process @{}</title>\n\
         <rect class=\"box\" x=\"{x:.1}\" y=\"{y:.1}\" width=\"{w:.1}\" height=\"{h:.1}\" rx=\"9\" ry=\"9\"/>\n",
        xml(&b.label), xml(&b.name)
    ));
    for (j, (class, text)) in sub_lines(s, id).iter().enumerate() {
        out.push_str(&format!(
            "<text class=\"{class}\" x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\">{}</text>\n",
            x + w / 2.0,
            y + 20.0 + 15.0 * j as f64,
            xml(text)
        ));
    }
    toggle(out, &b.label, x + w / 2.0, y + h, false);
    out.push_str("</g>\n");
}

/// The bold box of an expanded sub-process, its identifier and its names in
/// the corner, the minus on its border; its tree is drawn inside.
fn sub_box(out: &mut String, id: u32, b: &SubBox, x: f64, y: f64, w: f64, h: f64) {
    out.push_str(&format!(
        "<g class=\"sub expanded\" data-id=\"{id}\" data-sub=\"{}\">\n\
         <rect class=\"box\" x=\"{x:.1}\" y=\"{y:.1}\" width=\"{w:.1}\" height=\"{h:.1}\" rx=\"10\" ry=\"10\"/>\n\
         <text class=\"rid\" x=\"{:.1}\" y=\"{:.1}\">{} · {} @{}</text>\n",
        xml(&b.label), x + 7.0, y + 11.0, xml(&b.label), xml(&b.shown), xml(&b.name)
    ));
    toggle(out, &b.label, x + w / 2.0, y + h, true);
    out.push_str("</g>\n");
}

/// Draws the node in its box, top-left at `(x, y)`, flow line at mid height.
/// In the detailed drawing the box of a region is its dashed box, and the
/// content sits inside the margin.
fn draw(tree: &Tree, id: u32, x: f64, y: f64, s: &Sizes, out: &mut String) {
    let n = tree.node(id);
    if s.collapsed(id) {
        draw_collapsed(id, x, y, s, out);
        return;
    }
    let (w, h) = (s.w[id as usize], s.h[id as usize]);
    let ay = y + h / 2.0;
    let m = inset(tree, id, s);
    if let Some(b) = s.sub(id) {
        // expanded: the bold box takes the place of the dashed one
        sub_box(out, id, b, x, y, w, h);
    } else if n.kind != Kind::Task && s.detailed() {
        region_box(out, id, &s.label(id), x, y, w, h);
    }
    // the content of a region, inside its margin; the flow line is unmoved,
    // the margin being the same above and below
    let x = x + m;
    let y = y + m;
    let (w, h) = (w - 2.0 * m, h - 2.0 * m);
    match n.kind {
        Kind::Task => {
            let (lines, tip) = task_box_lines(tree, id, s);
            out.push_str(&format!("<g class=\"task\" data-id=\"{id}\">\n"));
            if !tip.is_empty() {
                out.push_str(&format!("<title>{}</title>\n", xml(&tip)));
            }
            out.push_str(&format!(
                "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{w:.1}\" height=\"{h:.1}\" rx=\"9\" ry=\"9\"/>\n"
            ));
            for (j, (class, text)) in lines.iter().enumerate() {
                out.push_str(&format!(
                    "<text class=\"{class}\" x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\">{}</text>\n",
                    x + w / 2.0,
                    y + 20.0 + 15.0 * j as f64,
                    xml(text)
                ));
            }
            if s.style.clocks {
                // the clock in the top right corner: a disc filled clockwise
                // from twelve by the duration against the longest, and the
                // number under it when the names are shown
                let (cx, cy) = (x + w - CLOCK_R - 6.0, y + CLOCK_R + 6.0);
                let f = if s.longest > 0 {
                    (n.duration as f64 / s.longest as f64).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                out.push_str(&format!(
                    "<circle class=\"clock\" cx=\"{cx:.1}\" cy=\"{cy:.1}\" r=\"{CLOCK_R}\"/>\n"
                ));
                if f >= 1.0 {
                    out.push_str(&format!(
                        "<circle class=\"hand\" cx=\"{cx:.1}\" cy=\"{cy:.1}\" r=\"{CLOCK_R}\"/>\n"
                    ));
                } else if f > 0.0 {
                    let a = f * std::f64::consts::TAU;
                    let (ex, ey) = (cx + CLOCK_R * a.sin(), cy - CLOCK_R * a.cos());
                    out.push_str(&format!(
                        "<path class=\"hand\" d=\"M {cx:.1} {cy:.1} L {cx:.1} {:.1} A {CLOCK_R} {CLOCK_R} 0 {} 1 {ex:.1} {ey:.1} Z\"/>\n",
                        cy - CLOCK_R,
                        if f > 0.5 { 1 } else { 0 }
                    ));
                }
                if s.style.names {
                    out.push_str(&format!(
                        "<text class=\"small\" x=\"{cx:.1}\" y=\"{:.1}\" text-anchor=\"middle\">{}</text>\n",
                        cy + CLOCK_R + 11.0,
                        n.duration
                    ));
                }
            }
            if let Some(bars) = &s.style.bars {
                // one row per impact under the lines: the name at the left
                // when the names are shown, then the bar, as long and as hot
                // as the value against the greatest of its impact
                let names = impact_names(tree);
                let lw = label_w(tree, s);
                let clock = if s.style.clocks { CLOCK_COL } else { 0.0 };
                let x0 = x + 12.0;
                let bx = x0 + lw;
                let top = y + 16.0 + 14.0 * lines.len() as f64 - 4.0;
                let values = tree.impact(id);
                let _ = clock;
                for (r, &c) in bars.iter().enumerate() {
                    let ry = top + r as f64 * ROW_H;
                    if s.style.names {
                        out.push_str(&format!(
                            "<text class=\"small\" x=\"{x0:.1}\" y=\"{:.1}\">{}</text>\n",
                            ry + BAR_T - 0.5,
                            xml(&names[c])
                        ));
                    }
                    let v = values.get(c).copied().unwrap_or(0.0);
                    let t = if s.maxes[c] > 0.0 { v / s.maxes[c] } else { 0.0 };
                    out.push_str(&format!(
                        "<line class=\"base\" x1=\"{bx:.1}\" y1=\"{:.1}\" x2=\"{bx:.1}\" y2=\"{:.1}\"/>\n",
                        ry, ry + BAR_T
                    ));
                    if t > 0.0 {
                        out.push_str(&format!(
                            "<rect class=\"bar\" x=\"{bx:.1}\" y=\"{ry:.1}\" width=\"{:.1}\" height=\"{BAR_T}\" style=\"fill:{}\"/>\n",
                            t * BAR_LEN,
                            heat(t)
                        ));
                    }
                }
            }
            out.push_str("</g>\n");
        }
        Kind::Sequence => {
            let (wl, hl) = (s.w[n.low as usize], s.h[n.low as usize]);
            let hh = s.h[n.high as usize];
            let (il, ih) = (inset(tree, n.low, s), inset(tree, n.high, s));
            draw(tree, n.low, x, ay - hl / 2.0, s, out);
            flow(
                out,
                &n.low.to_string(),
                &n.high.to_string(),
                &[(x + wl - il, ay), (x + wl + GAP + ih, ay)],
            );
            draw(tree, n.high, x + wl + GAP, ay - hh / 2.0, s, out);
        }
        Kind::Parallel | Kind::Choice | Kind::Nature => {
            let (wl, hl) = (s.w[n.low as usize], s.h[n.low as usize]);
            let (wh, hh) = (s.w[n.high as usize], s.h[n.high as usize]);
            let (il, ih) = (inset(tree, n.low, s), inset(tree, n.high, s));
            let cx = x + GW / 2.0; // the split
            let x1 = x + GW + lead(tree, id); // where both branches start
            let jx = x1 + wl.max(wh) + GAP + GW / 2.0; // the join
            let ayl = y + hl / 2.0;
            let ayh = y + hl + VGAP + hh / 2.0;
            let r = GW / 2.0;
            let split = format!("{id}:split");
            let join = format!("{id}:join");
            diamond(out, id, "split", n.kind, cx, ay);
            flow(out, &split, &n.low.to_string(), &[(cx, ay - r), (cx, ayl), (x1 + il, ayl)]);
            flow(out, &split, &n.high.to_string(), &[(cx, ay + r), (cx, ayh), (x1 + ih, ayh)]);
            if n.kind == Kind::Nature {
                // each probability sits on the connector of its branch, just
                // above the horizontal run that leads into it
                let p = tree.prob_exact(id);
                let q = num_rational::BigRational::from_integer(1.into()) - p;
                out.push_str(&format!(
                    "<text class=\"prob\" x=\"{:.1}\" y=\"{:.1}\">{}</text>\n<text class=\"prob\" x=\"{:.1}\" y=\"{:.1}\">{}</text>\n",
                    cx + 6.0, ayl - 5.0, decimal(p, 6),
                    cx + 6.0, ayh - 5.0, decimal(&q, 6)
                ));
            }
            draw(tree, n.low, x1, y, s, out);
            draw(tree, n.high, x1, y + hl + VGAP, s, out);
            flow(out, &n.low.to_string(), &join, &[(x1 + wl - il, ayl), (jx, ayl), (jx, ay - r)]);
            flow(out, &n.high.to_string(), &join, &[(x1 + wh - ih, ayh), (jx, ayh), (jx, ay + r)]);
            diamond(out, id, "join", n.kind, jx, ay);
        }
    }
}

/// The diagram of the whole process, start event to end event. `detailed`
/// wraps every region in a dashed box with its identifier and writes the
/// identifier among the data of every task; `tooltips` moves the impacts of
/// every task from a line in its box to a tooltip on it.
pub fn render(tree: &Tree, style: &Style) -> String {
    let n = tree.nodes.len();
    let mut maxes = vec![0.0f64; tree.k];
    for &t in &tree.tasks {
        for (j, v) in tree.impact(t).iter().enumerate() {
            if *v > maxes[j] {
                maxes[j] = *v;
            }
        }
    }
    let longest = tree.tasks.iter().map(|&t| tree.node(t).duration).max().unwrap_or(0);
    let subs: HashMap<u32, usize> = style.subs.iter().enumerate().map(|(i, b)| (b.root, i)).collect();
    let mut s = Sizes {
        w: vec![0.0; n],
        h: vec![0.0; n],
        style: style.clone(),
        maxes,
        longest,
        subs,
    };
    measure(tree, tree.root, &mut s);
    let (w, h) = (s.w[tree.root as usize], s.h[tree.root as usize]);
    // the legend of the bars takes a band above the drawing
    let legend_h = match &style.bars {
        Some(b) if !b.is_empty() => LEGEND_H,
        _ => 0.0,
    };
    let x0 = PAD + 2.0 * EVENT + GAP;
    let y0 = PAD + legend_h;
    let ay = y0 + h / 2.0;
    let total_w = x0 + w + GAP + 2.0 * EVENT + PAD;
    let total_h = h + 2.0 * PAD + legend_h;
    let mut out = String::new();
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{total_w:.0}\" height=\"{total_h:.0}\" \
         viewBox=\"0 0 {total_w:.0} {total_h:.0}\" font-family=\"-apple-system, BlinkMacSystemFont, Segoe UI, system-ui, sans-serif\">\n\
         <defs><marker id=\"arrow\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"7\" markerHeight=\"7\" orient=\"auto-start-reverse\">\
         <path d=\"M 0 0 L 10 5 L 0 10 z\" fill=\"#0b0b0b\"/></marker></defs>\n\
         <style>\
         .task rect{{fill:#fff;stroke:#0b0b0b;stroke-width:1.3}}\
         .task .name{{font-size:13px;font-weight:600;fill:#0b0b0b}}\
         .task .detail{{font-size:11px;fill:#6e6e6e}}\
         .gateway polygon{{fill:#fff;stroke:#0b0b0b;stroke-width:1.3}}\
         .gateway .mark{{fill:none;stroke:#0b0b0b;stroke-width:1.6}}\
         .gateway .dot{{fill:#0b0b0b}}\
         .flow{{fill:none;stroke:#0b0b0b;stroke-width:1.3}}\
         .event{{fill:#fff;stroke:#0b0b0b;stroke-width:1.3}}\
         .event.end{{stroke-width:3.2}}\
         .prob{{font-size:11px;fill:#6e6e6e}}\
         .region rect{{fill:none;stroke:#9c9a95;stroke-width:1;stroke-dasharray:5 4}}\
         .region .rid{{font-size:10.5px;fill:#6e6e6e;font-family:ui-monospace,Menlo,monospace}}\
         .task .id{{font-family:ui-monospace,Menlo,monospace}}\
         .task .base{{stroke:#c9c8c3;stroke-width:1}}\
         .task .bar{{stroke:none}}\
         .task .small{{font-size:9.5px;fill:#6e6e6e}}\
         .task .clock{{fill:#fff;stroke:#b9b7b1;stroke-width:1}}\
         .task .hand{{fill:#2a78d6;stroke:none}}\
         .legend text{{font-size:11px;fill:#0b0b0b}}\
         .sub.collapsed .box{{fill:#fff;stroke:#0b0b0b;stroke-width:1.3}}\
         .sub.expanded .box{{fill:none;stroke:#0b0b0b;stroke-width:2.2}}\
         .sub .name{{font-size:13px;font-weight:600;fill:#0b0b0b}}\
         .sub .detail{{font-size:11px;fill:#6e6e6e}}\
         .sub .rid{{font-size:10.5px;fill:#0b0b0b;font-weight:600;font-family:ui-monospace,Menlo,monospace}}\
         .toggle rect{{fill:#fff;stroke:#0b0b0b;stroke-width:1.2;cursor:pointer}}\
         .toggle text{{font-size:12px;font-weight:600;fill:#0b0b0b;text-anchor:middle;pointer-events:none}}\
         </style>\n"
    ));
    if let Some(bars) = &style.bars {
        if !bars.is_empty() {
            // the legend: the bars left to right by name, and the heat from
            // nothing to the greatest value of the impact
            let names = impact_names(tree);
            let listed: Vec<String> = bars.iter().map(|&c| xml(&names[c])).collect();
            let text = if style.names {
                "bars".to_string()
            } else {
                format!("bars, top to bottom: {}", listed.join(" · "))
            };
            out.push_str(&format!(
                "<g class=\"legend\">\n<text x=\"{PAD:.1}\" y=\"{:.1}\">{text}</text>\n",
                PAD + 5.0
            ));
            let mut lx = PAD + text_w(&text, 11.0) + 22.0;
            out.push_str(&format!(
                "<text x=\"{lx:.1}\" y=\"{:.1}\" fill=\"#6e6e6e\">0</text>\n",
                PAD + 5.0
            ));
            lx += 10.0;
            for i in 0..8 {
                out.push_str(&format!(
                    "<rect x=\"{lx:.1}\" y=\"{:.1}\" width=\"9\" height=\"9\" style=\"fill:{}\"/>\n",
                    PAD - 3.5,
                    heat(i as f64 / 7.0)
                ));
                lx += 9.0;
            }
            out.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" fill=\"#6e6e6e\">greatest of the impact</text>\n</g>\n",
                lx + 4.0,
                PAD + 5.0
            ));
        }
    }
    out.push_str(&format!(
        "<circle class=\"event start\" cx=\"{:.1}\" cy=\"{ay:.1}\" r=\"{EVENT}\"/>\n",
        PAD + EVENT
    ));
    let ir = inset(tree, tree.root, &s);
    flow(&mut out, "start", &tree.root.to_string(), &[(PAD + 2.0 * EVENT, ay), (x0 + ir, ay)]);
    draw(tree, tree.root, x0, y0, &s, &mut out);
    flow(&mut out, &tree.root.to_string(), "end", &[(x0 + w - ir, ay), (x0 + w + GAP, ay)]);
    out.push_str(&format!(
        "<circle class=\"event end\" cx=\"{:.1}\" cy=\"{ay:.1}\" r=\"{EVENT}\"/>\n</svg>\n",
        x0 + w + GAP + EVENT
    ));
    out
}
