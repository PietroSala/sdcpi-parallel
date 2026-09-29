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
struct Sizes {
    w: Vec<f64>,
    h: Vec<f64>,
    /// The detailed drawing: every region in a dashed box with its
    /// identifier, and the identifier among the data of every task.
    detailed: bool,
    /// The impacts of a task as a tooltip on its box rather than as a line
    /// in it, which keeps a box narrow when the impacts are many.
    tooltips: bool,
}

/// The lines of a task box, the name first and in a larger type: the
/// identifier in the detailed drawing, the duration, the impacts unless they
/// are a tooltip.
fn task_box_lines(tree: &Tree, id: u32, s: &Sizes) -> (Vec<(&'static str, String)>, String) {
    let (name, duration, impacts) = task_lines(tree, id);
    let mut lines: Vec<(&'static str, String)> = vec![("name", name)];
    if s.detailed {
        lines.push(("detail id", format!("id {id}")));
    }
    lines.push(("detail", duration));
    if !s.tooltips {
        lines.push(("detail", impacts.clone()));
    }
    (lines, impacts)
}

/// The height of a task box of `n` lines.
fn task_h(n: usize) -> f64 {
    16.0 + 14.0 * n as f64
}

/// How far inside its box the first and last element of a node sit: the
/// margin of its dashed box in the detailed drawing, nothing otherwise and
/// nothing for a task, which has no box around it.
fn inset(tree: &Tree, id: u32, s: &Sizes) -> f64 {
    if s.detailed && tree.node(id).kind != Kind::Task {
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
    let m = if s.detailed { REGION_M } else { 0.0 };
    let (w, h) = match n.kind {
        Kind::Task => {
            let (lines, _) = task_box_lines(tree, id, s);
            let w = lines
                .iter()
                .map(|(class, text)| text_w(text, if *class == "name" { 13.0 } else { 11.0 }))
                .fold(TASK_MIN_W, f64::max)
                + 24.0;
            (w, task_h(lines.len()))
        }
        Kind::Sequence => {
            measure(tree, n.low, s);
            measure(tree, n.high, s);
            (
                s.w[n.low as usize] + GAP + s.w[n.high as usize] + 2.0 * m,
                s.h[n.low as usize].max(s.h[n.high as usize]) + 2.0 * m,
            )
        }
        Kind::Parallel | Kind::Choice | Kind::Nature => {
            measure(tree, n.low, s);
            measure(tree, n.high, s);
            (
                GW + lead(tree, id) + s.w[n.low as usize].max(s.w[n.high as usize]) + GAP + GW
                    + 2.0 * m,
                s.h[n.low as usize] + VGAP + s.h[n.high as usize] + 2.0 * m,
            )
        }
    };
    s.w[id as usize] = w;
    s.h[id as usize] = h;
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
fn region_box(out: &mut String, id: u32, x: f64, y: f64, w: f64, h: f64) {
    out.push_str(&format!(
        "<g class=\"region\" data-id=\"{id}\">\n<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{w:.1}\" height=\"{h:.1}\" rx=\"10\" ry=\"10\"/>\n\
         <text class=\"rid\" x=\"{:.1}\" y=\"{:.1}\">{id}</text>\n</g>\n",
        x + 7.0, y + 11.0
    ));
}

/// Draws the node in its box, top-left at `(x, y)`, flow line at mid height.
/// In the detailed drawing the box of a region is its dashed box, and the
/// content sits inside the margin.
fn draw(tree: &Tree, id: u32, x: f64, y: f64, s: &Sizes, out: &mut String) {
    let n = tree.node(id);
    let (w, h) = (s.w[id as usize], s.h[id as usize]);
    let ay = y + h / 2.0;
    let m = inset(tree, id, s);
    if n.kind != Kind::Task && s.detailed {
        region_box(out, id, x, y, w, h);
    }
    // the content of a region, inside its margin; the flow line is unmoved,
    // the margin being the same above and below
    let x = x + m;
    let y = y + m;
    match n.kind {
        Kind::Task => {
            let (lines, impacts) = task_box_lines(tree, id, s);
            out.push_str(&format!("<g class=\"task\" data-id=\"{id}\">\n"));
            if s.tooltips {
                out.push_str(&format!(
                    "<title>{}</title>\n",
                    xml(if impacts.is_empty() { "no impact" } else { &impacts })
                ));
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
pub fn render(tree: &Tree, detailed: bool, tooltips: bool) -> String {
    let n = tree.nodes.len();
    let mut s = Sizes {
        w: vec![0.0; n],
        h: vec![0.0; n],
        detailed,
        tooltips,
    };
    measure(tree, tree.root, &mut s);
    let (w, h) = (s.w[tree.root as usize], s.h[tree.root as usize]);
    let x0 = PAD + 2.0 * EVENT + GAP;
    let y0 = PAD;
    let ay = y0 + h / 2.0;
    let total_w = x0 + w + GAP + 2.0 * EVENT + PAD;
    let total_h = h + 2.0 * PAD;
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
         </style>\n"
    ));
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
