#!/usr/bin/env python3
"""The figures and the tables of the experimental section, from ``analysis.json``.

The campaign writes one record per run; ``analysis.py`` of the library
repository reads every record in one pass and writes ``parallel-paper/data/``,
of which ``analysis.json`` holds every number the section quotes. This script
reads that file alone and renders the figures of the experimental section as
PDF, at the text width of the paper, 347 pt in ``llncs``, so that they are
included at natural size and their type prints at the size it was set; and it
writes the tables of the section in the style of the house, plain rules and no
package. The campaign is still running while the paper is written, and the
update is

    python3 scripts/pareto-frontier/analysis.py      # in the library repository
    python3 parallel-paper/experiments/make_figures.py

so that figures, tables and prose move together or not at all.

Conventions, held across every figure so that a reader carries the palette
once, and the same ones the journal paper holds: color follows the
configuration and never the chart, the search with both tests blue, the model
checker orange, the search with the closing test alone aqua, with the
recording test alone yellow, with neither magenta; log axes wherever a decade
matters; no titles inside the figures, the caption being the title.

A run cut at the budget carries candidate points, the minimal elements of what
it had found by the cut, which no later run has confirmed: every figure and
every table that counts them says so, and every rate is given twice, once over
every run and once over the runs that closed.
"""

from __future__ import annotations

import json
from pathlib import Path

import plotly.graph_objects as go
from plotly.subplots import make_subplots

HERE = Path(__file__).resolve().parent
PAPER = HERE.parent
DATA = PAPER / "data"
FIGS = PAPER / "figures"
TABLES = PAPER / "tables"
R = json.loads((DATA / "analysis.json").read_text())

BOTH = "#2a78d6"      # the search, both tests
STORM = "#eb6834"     # the model checker
REJECT = "#1baf7a"    # the closing test alone
ACCEPT = "#eda100"    # the recording test alone
NONE = "#e87ba4"      # neither test
INK = "#0b0b0b"
MUTED = "#52514e"
GRID = "#e9e8e4"

COLOR = {"both": BOTH, "reject": REJECT, "accept": ACCEPT, "none": NONE}
DASH = {"both": "solid", "reject": "dash", "accept": "dot", "none": "dashdot"}
ORDER = ["both", "reject", "accept", "none"]

FONT = dict(family="Times New Roman, serif", size=8, color=INK)
LEGEND_TOP = dict(orientation="h", x=0, y=1.10, xanchor="left", yanchor="bottom",
                  font=dict(size=7), bgcolor="rgba(0,0,0,0)")
AXIS = dict(showline=True, linecolor=MUTED, linewidth=0.7, ticks="outside",
            tickcolor=MUTED, tickfont=dict(size=7), gridcolor=GRID,
            gridwidth=0.5, zeroline=False, title_font=dict(size=8))
# The text width of llncs is 347.12 pt. A PDF is measured by pdftex in big
# points of a seventy-second of an inch, while the pt of LaTeX is a
# seventy-second-and-a-quarter, so a figure exported at 347 is read as 348.34
# and overruns the line: the width to draw at is 347.12 times 72 over 72.27.
WIDTH = 345           # big points, that is 346.3 pt, just inside the text


def layout(fig, w, h, **kw):
    fig.update_layout(width=w, height=h, font=FONT, plot_bgcolor="white",
                      paper_bgcolor="white",
                      margin=dict(l=46, r=8,
                                  t=22 if kw.get("showlegend") else 6, b=30), **kw)
    fig.update_xaxes(**AXIS)
    fig.update_yaxes(**AXIS)


def save(fig, name):
    # kaleido reads the layout in CSS pixels at 96 to the inch and writes the
    # page in big points at 72, so a figure laid out at WIDTH must be exported
    # at four thirds for the page to come out WIDTH big points wide
    FIGS.mkdir(exist_ok=True)
    fig.write_image(str(FIGS / name), format="pdf", scale=4 / 3)
    print(FIGS / name)


def read_dat(name):
    """The rows of a data file, floats where they parse."""
    rows = []
    for line in (DATA / name).read_text().splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        out = []
        for tok in line.split():
            try:
                out.append(float(tok))
            except ValueError:
                out.append(tok)
        rows.append(out)
    return rows


# -- the comparison with the model checker ---------------------------------

def figure_storm():
    # storm-vs-ours.dat: diagonal impacts storm ours ours_tool sc oc ratio
    rows = read_dat("storm-vs-ours.dat")
    closed = [r for r in rows if r[5] == 1 and r[6] == 1 and r[2] > 0]
    fig = make_subplots(rows=1, cols=2, horizontal_spacing=0.16)
    xs = [r[3] for r in closed]
    ys = [r[2] for r in closed]
    lo, hi = 2e-3, 1.2e-1
    for k in (10, 100, 1000):
        fig.add_trace(go.Scatter(x=[lo, hi], y=[lo * k, hi * k], mode="lines",
                                 line=dict(color="#c9c8c4", width=0.8, dash="dot"),
                                 showlegend=False, hoverinfo="skip"), row=1, col=1)
    fig.add_trace(go.Scatter(x=xs, y=ys, mode="markers",
                             marker=dict(size=2, color=BOTH, opacity=0.35),
                             showlegend=False), row=1, col=1)
    fig.update_xaxes(type="log", title_text="our search (s)",
                     range=[-2.7, -0.92], tickvals=[0.003, 0.01, 0.03, 0.1],
                     ticktext=["0.003", "0.01", "0.03", "0.1"], row=1, col=1)
    fig.update_yaxes(type="log", title_text="model checker (s)",
                     range=[-1.4, 2.15], tickvals=[0.1, 1, 10, 100],
                     ticktext=["0.1", "1", "10", "100"], row=1, col=1)

    # time-median-by-diagonal.dat: diagonal instances storm_q1 storm_median
    # storm_q3 ours_q1 ours_median ours_q3 ratio_median
    med = read_dat("time-median-by-diagonal.dat")
    fig.add_trace(go.Scatter(x=[r[0] for r in med], y=[r[3] for r in med],
                             mode="lines+markers", name="model checker",
                             line=dict(color=STORM, width=1.8),
                             marker=dict(size=4)), row=1, col=2)
    fig.add_trace(go.Scatter(x=[r[0] for r in med], y=[r[6] for r in med],
                             mode="lines+markers", name="our search",
                             line=dict(color=BOTH, width=1.8),
                             marker=dict(size=4)), row=1, col=2)
    fig.update_xaxes(title_text="diagonal", dtick=1, row=1, col=2)
    fig.update_yaxes(type="log", title_text="median time (s)",
                     tickvals=[0.001, 0.01, 0.1, 1],
                     ticktext=["0.001", "0.01", "0.1", "1"], row=1, col=2)
    layout(fig, WIDTH, 158, showlegend=True, legend=LEGEND_TOP)
    save(fig, "exp-storm.pdf")


# -- the ablation ----------------------------------------------------------

def figure_ablation():
    fig = make_subplots(rows=1, cols=3, horizontal_spacing=0.115)
    cells = R["ablation"]["cells"]
    ws = R["workers"]
    for a in ORDER:
        pts = read_dat(f"points-per-second-{a}.dat")     # w rate rate_complete runs cut
        apx = read_dat(f"apexes-per-second-{a}.dat")
        line = dict(color=COLOR[a], width=1.6, dash=DASH[a])
        fig.add_trace(go.Scatter(x=[r[0] for r in pts], y=[r[1] for r in pts],
                                 mode="lines+markers", name=a, line=line,
                                 marker=dict(size=4)), row=1, col=1)
        fig.add_trace(go.Scatter(x=[r[0] for r in apx], y=[r[1] for r in apx],
                                 mode="lines+markers", name=a, showlegend=False,
                                 line=line, marker=dict(size=4)), row=1, col=2)
        fig.add_trace(go.Scatter(x=ws, y=[100 * cells[f"{a}-w{w}"]["yield"] for w in ws],
                                 mode="lines+markers", name=a, showlegend=False,
                                 line=line, marker=dict(size=4)), row=1, col=3)
    for col, title in ((1, "optima/s"), (2, "apexes/s"), (3, "yield (%)")):
        fig.update_xaxes(title_text="workers" if col == 2 else "", type="log",
                         tickvals=ws, ticktext=[str(w) for w in ws],
                         title_standoff=4, row=1, col=col)
        fig.update_yaxes(title_text=title, title_standoff=3, row=1, col=col)
    layout(fig, WIDTH, 136, showlegend=True, legend=LEGEND_TOP)
    fig.update_layout(margin=dict(l=32, r=6, t=22, b=40))
    save(fig, "exp-ablation.pdf")


# -- what a run cut at the budget holds ------------------------------------

def figure_anytime():
    fig = go.Figure()
    for a in ORDER:
        rows = read_dat(f"anytime-{a}.dat")   # s mean_points median mean_apexes fraction runs
        fig.add_trace(go.Scatter(x=[r[0] for r in rows], y=[100 * r[4] for r in rows],
                                 mode="lines", name=a,
                                 line=dict(color=COLOR[a], width=1.6, dash=DASH[a])))
    fig.update_xaxes(title_text="seconds of the budget", dtick=20,
                     title_standoff=4)
    fig.update_yaxes(title_text="per cent held", range=[0, 102],
                     title_standoff=3)
    layout(fig, WIDTH, 142, showlegend=True, legend=LEGEND_TOP)
    fig.update_layout(margin=dict(l=34, r=6, t=22, b=40))
    save(fig, "exp-anytime.pdf")


def figure_anytime_cdf(name="exp-anytime-cdf.pdf", ablations=("both",),
                       legend=False, workers=(1,)):
    """When the points of a completed run are found, from the buffers.

    Two cumulative curves per configuration at one worker, from
    ``anytime-metrics.json`` of ``anytime_metrics.py`` in the library
    repository: on the left, the per cent of the points of the final
    frontier already found within the first x per cent of the run, pooled
    over every point of every completed run; on the right, the per cent of
    the completed runs whose certification tail, the time after the last
    point in which the search only certifies, is at most x per cent of the
    run. The body takes the default configuration alone; the appendix
    repeats the two curves for the four combinations of the tests
    (settled 2026-10-15, the ablation reading lives in the appendix). The
    body keeps both tests and draws one curve per number of workers
    (Pietro, 2026-10-16), on a ramp of the house blue, light at 1 worker
    and dark at 8."""
    M = json.loads((DATA / "anytime-metrics.json").read_text())
    fig = make_subplots(rows=1, cols=2, horizontal_spacing=0.11)
    bins = M["hist_bins"]["frac_bins"]
    wblue = {1: "#a9c9ee", 2: "#6fa3e1", 4: "#2a78d6", 8: "#15498d"}
    # Nested widths, wide and light below, narrow and dark on top: on the
    # left panel the four curves lie within one point of one another, which
    # is the finding, and the nesting keeps all four visible where they
    # coincide (Pietro, 2026-10-16); the order of the series is the order
    # they are drawn in.
    wwidth = {1: 3.4, 2: 2.6, 4: 1.8, 8: 1.0}
    series = [(a, w) for a in ablations for w in workers]
    for a, w in series:
        c = M["by_conf"][f"onephase-{a}-w{w}"]
        if len(workers) > 1:
            color, dash, width = wblue[w], "solid", wwidth[w]
            label = f"{w} worker" + ("s" if w > 1 else "")
        else:
            color, dash, label = COLOR[a], DASH[a], a
            width = 1.6
        h = c["delay_hist_frac"]
        n = sum(h)
        xs, ys, cum = [0.0], [0.0], 0
        for k in range(1, bins + 1):
            cum += h[bins - k]
            xs.append(100.0 * k / bins)
            ys.append(100.0 * cum / n)
        fig.add_trace(go.Scatter(x=xs, y=ys, mode="lines", name=label,
                                 line=dict(color=color, width=width,
                                           dash=dash)), row=1, col=1)
        h2 = c["tail_hist_frac"]
        n2 = sum(h2)
        xs2, ys2, cum = [0.0], [0.0], 0
        for b in range(bins):
            cum += h2[b]
            xs2.append(100.0 * (b + 1) / bins)
            ys2.append(100.0 * cum / n2)
        # The right panel reads as a quantile since 2026-10-16: the share
        # of the completed runs on x, the tail it certifies within on y.
        fig.add_trace(go.Scatter(x=ys2, y=xs2, mode="lines", name=label,
                                 showlegend=False,
                                 line=dict(color=color, width=width,
                                           dash=dash)), row=1, col=2)
    fig.update_xaxes(title_text="% of the run elapsed", row=1, col=1,
                     range=[0, 100], dtick=25, title_standoff=4)
    fig.update_yaxes(title_text="points found, %", row=1, col=1,
                     range=[0, 102], dtick=25, title_standoff=3)
    fig.update_xaxes(title_text="completed runs, %",
                     row=1, col=2, range=[0, 102], dtick=25, title_standoff=4)
    fig.update_yaxes(title_text="certification tail, % of the run", row=1,
                     col=2, range=[0, 30], dtick=10, title_standoff=3)
    layout(fig, WIDTH, 142, showlegend=legend,
           **(dict(legend=LEGEND_TOP) if legend else {}))
    fig.update_layout(margin=dict(l=36, r=8, t=22 if legend else 8, b=42))
    save(fig, name)


# -- the tables, in the style of the house: plain rules, no package ---------

def fmt(x, d=0):
    return f"{x:,.{d}f}".replace(",", "\\,")


def table_storm():
    s = R["storm"]
    rows = []
    for d in sorted(s["by_diagonal"], key=int):
        b = s["by_diagonal"][d]
        r = b.get("ratio") or {}
        rows.append(f"    {d} & {fmt(b['instances'])} & {fmt(b['complete'])} & "
                    f"{fmt(b['ours_complete'])} & {fmt(r.get('q1', 0), 1)} & "
                    f"{fmt(r.get('median', 0), 1)} & {fmt(r.get('q3', 0), 1)} & "
                    f"{fmt(b['same'])}/{fmt(b['compared'])} \\\\")
    t = s["total"]
    h = s["headline"]["ratio"]
    rows.append("    \\hline")
    rows.append(f"    all & {fmt(t['instances'])} & {fmt(t['complete'])} & "
                f"{fmt(t['ours_complete'])} & {fmt(h['q1'], 1)} & {fmt(h['median'], 1)} & "
                f"{fmt(h['q3'], 1)} & {fmt(t['same'])}/{fmt(t['compared'])} \\\\")
    body = "\n".join(rows)
    return f"""% written by parallel-paper/experiments/make_figures.py; do not edit
\\begin{{table}}[t]
  \\caption{{Storm (checker) against the search with both tests and 1 worker,
  by diagonal: the instances each completes within the budget of 120\\,s; the
  quartiles of the ratio of the wall-clock time of Storm to that of the
  search, on the instances both complete; and the instances where the two
  frontiers are the same set, over those both complete. The
  {fmt(t['ours_truncated'])} instances on which the search is cut enter no
  ratio and no count of sets.}}\\label{{tab:storm}}
  \\centering\\scriptsize
  \\setlength{{\\tabcolsep}}{{4pt}}\\renewcommand{{\\arraystretch}}{{0.95}}
  \\begin{{tabular}}{{rrrrrrrr}}
    \\hline
    diag. & instances & checker & search & $q_1$ & median & $q_3$ & same set \\\\
    \\hline
{body}
    \\hline
  \\end{{tabular}}
\\end{{table}}
"""


def table_ablation():
    cells = R["ablation"]["cells"]
    rows = []
    for a in ORDER:
        for w in R["workers"]:
            c = cells[f"{a}-w{w}"]
            rows.append(
                f"    {a if w == R['workers'][0] else ''} & {w} & {fmt(c['runs'])} & "
                f"{fmt(c['truncated'])} & {fmt(c['apexes_per_second'], 1)} & "
                f"{fmt(c['points_per_second'], 1)} & "
                f"{fmt(c['points_per_second_complete'], 1)} & "
                f"{fmt(100 * c['yield'], 1)} \\\\")
        if a != ORDER[-1]:
            rows.append("    \\hline")
    body = "\n".join(rows)
    return f"""% written by parallel-paper/experiments/make_figures.py; do not edit
\\begin{{table}}[t]
  \\caption{{The two early tests, crossed with the workers $w$, over the
  diagonals 2 to 7: \\emph{{accept}} keeps the first test alone and
  \\emph{{reject}} the second alone. A run cut at the budget contributes
  candidate optima, the minimal elements of what it had found by the cut: the
  rate over all runs counts them, the rate on the runs that complete does
  not, and the two differ because the cut runs hold most of the time. The
  yield is the optima found over the apexes recorded.}}\\label{{tab:ablation}}
  \\centering\\scriptsize
  \\setlength{{\\tabcolsep}}{{4pt}}\\renewcommand{{\\arraystretch}}{{0.95}}
  \\begin{{tabular}}{{lrrrrrrr}}
    \\hline
    & \\multicolumn{{1}}{{c}}{{$w$}} & runs & cut & apexes/s & optima/s &
    optima/s & yield \\\\
    & & & & & (all runs) & (complete) & (\\%) \\\\
    \\hline
{body}
    \\hline
  \\end{{tabular}}
\\end{{table}}
"""


def table_grid():
    c = R["census"]["by_diagonal"]
    rows = []
    for d in sorted(c, key=int):
        b = c[d]
        rows.append(f"    {d} & {fmt(b['instances'])} & {fmt(b['runs_done'])} & "
                    f"{fmt(b['storm_records'])} \\\\")
    body = "\n".join(rows)
    return f"""% written by parallel-paper/experiments/make_figures.py; do not edit
\\begin{{table}}[t]
  \\caption{{The campaign, by diagonal. Every instance is run under 16
  configurations of the search, the four combinations of the two early tests
  at 1, 2, 4 and 8 workers, and once by Storm (checker).}}\\label{{tab:grid}}
  \\centering\\small
  \\begin{{tabular}}{{rrrr}}
    \\hline
    diag. & instances & runs of the search & checker \\\\
    \\hline
{body}
    \\hline
  \\end{{tabular}}
\\end{{table}}
"""


def main():
    figure_storm()
    figure_ablation()
    figure_anytime()
    figure_anytime_cdf(workers=(1, 2, 4, 8), legend=True)
    figure_anytime_cdf("exp-anytime-cdf-abl.pdf", ORDER, legend=True)
    TABLES.mkdir(exist_ok=True)
    for name, text in (("tab-storm.tex", table_storm()),
                       ("tab-ablation.tex", table_ablation()),
                       ("tab-grid.tex", table_grid())):
        (TABLES / name).write_text(text)
        print(TABLES / name)


if __name__ == "__main__":
    main()
