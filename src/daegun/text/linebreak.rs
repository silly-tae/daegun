use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Breakpoint {
    pub at: usize,
    pub ink: f64,
    pub space: f64,
    pub stretch: f64,
    pub shrink: f64,
    pub penalty: f64,
}

impl Breakpoint {
    pub fn start() -> Breakpoint {
        Breakpoint {
            at: 0,
            ink: 0.0,
            space: 0.0,
            stretch: 0.0,
            shrink: 0.0,
            penalty: 0.0,
        }
    }

    pub fn is_forced(&self) -> bool {
        self.penalty == f64::NEG_INFINITY
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fit {
    pub target: f64,
    pub line_end_stretch: f64,
    pub last_line_stretch: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Line {
    pub from: usize,
    pub to: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BreakStrategy {
    #[default]
    Greedy,
    Optimal,
}

struct Metrics<'a> {
    bps: &'a [Breakpoint],
    cum: Vec<Cumulative>,
}

#[derive(Clone, Copy, Default)]
struct Cumulative {
    ink: f64,
    space: f64,
    stretch: f64,
    shrink: f64,
}

impl<'a> Metrics<'a> {
    fn new(bps: &'a [Breakpoint]) -> Self {
        let mut cum = Vec::with_capacity(bps.len() + 1);
        let mut run = Cumulative::default();
        cum.push(run);
        for bp in bps {
            run.ink += bp.ink;
            run.space += bp.space;
            run.stretch += bp.stretch;
            run.shrink += bp.shrink;
            cum.push(run);
        }
        Metrics { bps, cum }
    }

    fn natural_width(&self, from: usize, to: usize) -> f64 {
        debug_assert!(to > from, "a line ends after the break it starts from");
        let (a, b) = (from + 1, to);
        (self.cum[b + 1].ink - self.cum[a].ink) + (self.cum[b].space - self.cum[a].space)
    }

    fn tail(&self, to: usize, fit: &Fit) -> Tail {
        let bp = self.bps[to];
        let forced = bp.is_forced();
        let mut stretch = fit.line_end_stretch;
        if forced {
            stretch += fit.last_line_stretch;
        }
        Tail {
            ink: self.cum[to + 1].ink,
            space: self.cum[to].space,
            stretch: stretch + self.cum[to].stretch,
            shrink: self.cum[to].shrink,
            penalty_sq: if forced {
                0.0
            } else if bp.penalty >= 0.0 {
                bp.penalty * bp.penalty
            } else {
                -(bp.penalty * bp.penalty)
            },
            forced,
        }
    }

    fn head(&self, from: usize) -> Cumulative {
        self.cum[from + 1]
    }
}

#[derive(Clone, Copy)]
struct Tail {
    ink: f64,
    space: f64,
    stretch: f64,
    shrink: f64,
    penalty_sq: f64,
    forced: bool,
}

fn ratio_between(tail: &Tail, head: &Cumulative, fit: &Fit) -> f64 {
    let slack = fit.target - ((tail.ink - head.ink) + (tail.space - head.space));
    let (stretch, shrink) = (tail.stretch - head.stretch, tail.shrink - head.shrink);
    if slack > 0.0 {
        // Infinite stretch is how a line says falling short costs nothing, and it makes `slack`
        // infinite too – so the ratio is NaN, which loses every comparison in the search: no node
        // stays active and the optimal strategy degrades silently to one word per line.
        if stretch.is_infinite() { 0.0 }
        else if stretch <= 0.0 { f64::INFINITY }
        else { slack / stretch }
    } else if slack < 0.0 {
        if shrink.is_infinite() { 0.0 }
        else if shrink <= 0.0 { f64::NEG_INFINITY }
        else { slack / shrink }
    } else {
        0.0
    }
}

fn badness(ratio: f64) -> f64 {
    let a = ratio.abs();
    100.0 * a * a * a
}

const LINE_PENALTY: f64 = 10.0;
const FITNESS_DEMERITS: f64 = 10_000.0;
const MAX_RATIO: f64 = 10.0;
const OVERFULL_DEMERITS: f64 = 1.0e12;

const UNDERFULL_DEMERITS: f64 = 1.0e9;

fn fitness_class(ratio: f64) -> u8 {
    if ratio < -0.5 {
        0
    } else if ratio <= 0.5 {
        1
    } else if ratio <= 1.0 {
        2
    } else {
        3
    }
}

#[derive(Clone, Copy)]
struct LineCost {
    plain: f64,
    jumped: f64,
    class: u8,
    open: bool,
}

fn line_cost_shared(tail: &Tail, head: &Cumulative, fit: &Fit) -> LineCost {
    let ratio = ratio_between(tail, head, fit);
    let (effective, surcharge) = if ratio < -1.0 {
        (-1.0, OVERFULL_DEMERITS)
    } else if ratio > MAX_RATIO {
        (MAX_RATIO, UNDERFULL_DEMERITS)
    } else {
        (ratio, 0.0)
    };
    let base = line_demerits(tail, effective);
    LineCost {
        plain: base + surcharge,
        jumped: (base + FITNESS_DEMERITS) + surcharge,
        class: fitness_class(effective),
        open: ratio >= -1.0,
    }
}

impl LineCost {
    fn demerits_after(&self, prev_class: u8) -> f64 {
        if (self.class as i16 - prev_class as i16).abs() > 1 { self.jumped } else { self.plain }
    }
}

fn line_demerits(tail: &Tail, ratio: f64) -> f64 {
    let base = LINE_PENALTY + badness(ratio);
    base * base + tail.penalty_sq
}

pub(crate) fn break_greedy(bps: &[Breakpoint], fit: &Fit) -> Vec<Line> {
    let mut lines = Vec::new();
    if bps.len() < 2 {
        return lines;
    }

    let m = Metrics::new(bps);
    let mut from = 0;
    let mut last_fit: Option<usize> = None;
    let mut j = 1;
    while j < bps.len() {
        if m.natural_width(from, j) <= fit.target {
            if bps[j].is_forced() {
                lines.push(Line { from, to: j });
                from = j;
                last_fit = None;
            } else {
                last_fit = Some(j);
            }
            j += 1;
            continue;
        }
        // A forced break is *not* exempt from falling back to the last opportunity that fit. It
        // must be taken eventually, but the text before it still has to be broken to fit, or a
        // paragraph ending in a long sentence becomes one enormous line.
        let brk = last_fit.unwrap_or(j);
        lines.push(Line { from, to: brk });
        from = brk;
        last_fit = None;
        j = brk + 1;
    }
    if from + 1 < bps.len() {
        lines.push(Line { from, to: bps.len() - 1 });
    }
    lines
}

#[derive(Clone, Copy)]
struct Node {
    position: usize,
    fitness: u8,
    demerits: f64,
    previous: usize,
}

const MAX_ACTIVE: usize = 1_000;

pub(crate) fn break_optimal(bps: &[Breakpoint], fit: &Fit) -> Option<Vec<Line>> {
    if bps.len() < 2 {
        return Some(Vec::new());
    }

    let mut nodes: Vec<Node> = alloc::vec![Node {
        position: 0,
        fitness: 1,
        demerits: 0.0,
        previous: usize::MAX,
    }];
    let mut active: Vec<usize> = alloc::vec![0];
    let mut still_active: Vec<usize> = Vec::new();
    let m = Metrics::new(bps);

    for j in 1..bps.len() {
        let tail = m.tail(j, fit);
        let forced = tail.forced;
        let mut best: [Option<(f64, usize)>; 4] = [None; 4];
        still_active.clear();
        let mut memo: Option<(usize, LineCost)> = None;

        for &a in &active {
            let node = nodes[a];
            let cost = match memo {
                Some((p, c)) if p == node.position => c,
                _ => {
                    let c = line_cost_shared(&tail, &m.head(node.position), fit);
                    memo = Some((node.position, c));
                    c
                }
            };

            let total = node.demerits + cost.demerits_after(node.fitness);
            let slot = &mut best[cost.class as usize];
            if slot.is_none_or(|(bd, _)| total < bd) {
                *slot = Some((total, a));
            }

            if !forced && cost.open {
                still_active.push(a);
            }
        }

        for (class, entry) in best.iter().enumerate() {
            let Some((d, prev)) = *entry else { continue };
            nodes.push(Node {
                position: j,
                fitness: class as u8,
                demerits: d,
                previous: prev,
            });
            still_active.push(nodes.len() - 1);
        }

        if still_active.len() > MAX_ACTIVE {
            still_active.sort_unstable_by(|&x, &y| {
                nodes[x].demerits.partial_cmp(&nodes[y].demerits).unwrap_or(core::cmp::Ordering::Equal)
            });
            still_active.truncate(MAX_ACTIVE);
        }

        core::mem::swap(&mut active, &mut still_active);
        if active.is_empty() {
            return None;
        }
    }

    let end = bps.len() - 1;
    let best = (0..nodes.len())
        .filter(|&i| nodes[i].position == end)
        .min_by(|&a, &b| {
            nodes[a].demerits.partial_cmp(&nodes[b].demerits).unwrap_or(core::cmp::Ordering::Equal)
        })?;

    let mut out = Vec::new();
    let mut cur = best;
    while nodes[cur].previous != usize::MAX {
        out.push(Line { from: nodes[nodes[cur].previous].position, to: nodes[cur].position });
        cur = nodes[cur].previous;
    }
    out.reverse();
    Some(out)
}

pub(crate) fn break_lines(bps: &[Breakpoint], fit: &Fit, strategy: BreakStrategy) -> Vec<Line> {
    match strategy {
        BreakStrategy::Greedy => break_greedy(bps, fit),
        BreakStrategy::Optimal => break_optimal(bps, fit).unwrap_or_else(|| break_greedy(bps, fit)),
    }
}
