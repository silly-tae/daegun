use daegun::paint::{DisplayList, Op, Paint, Rgba};
use daegun::{FillRule, OutlinePen, Path};

// Clockwise in y up, which is how a font winds a filled contour.
fn rect(p: &mut Path, x0: f32, x1: f32, y0: f32, y1: f32) {
    p.move_to(x0, y0);
    p.line_to(x0, y1);
    p.line_to(x1, y1);
    p.line_to(x1, y0);
    p.close();
}

// The topmost row that has any ink, at one device pixel per unit.
fn top_row(build: &dyn Fn(&mut Path)) -> Vec<u8> {
    let mut p = Path::default();
    build(&mut p);
    let mut list = DisplayList::default();
    let id = list.push_path(p);
    list.push(Op::Fill {
        path:      id,
        paint:     Paint::Solid(Rgba { r: 255, g: 255, b: 255, a: 255 }),
        rule:      FillRule::NonZero,
        transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    });
    let s = daegun::paint::render(&list, 1.0, 1.0).expect("renders");
    let row = (0..s.height)
        .find(|&y| (0..s.width).any(|x| s.rgba[(y * s.width + x) * 4 + 3] > 0))
        .expect("ink");
    (0..s.width).map(|x| s.rgba[(row * s.width + x) * 4 + 3]).collect()
}

// Two contours that overlap must render as their union.
//
// The coverage accumulator holds one float per pixel: the integral of winding over that pixel. A
// pixel that two contours both partly cover gets each one's share added, so an antialiased edge
// came out at twice its coverage — `102 102 204 204 204 102 102` along an edge that is one straight
// line and has to be constant. Inter's "4" is exactly this shape, its diagonal and its stem sharing
// a flat top, and it rendered that top at 255 in the middle and 188 either side.
//
// The tops here are at y = 10.4, so every column of the top row is 40% covered whichever contours
// produced it.
#[test]
fn two_overlapping_contours_read_the_same_as_their_union() {
    let one = top_row(&|p| rect(p, 0.0, 15.0, 0.0, 10.4));
    let two = top_row(&|p| {
        rect(p, 0.0, 10.0, 0.0, 10.4);
        rect(p, 5.0, 15.0, 0.0, 10.4);
    });
    assert_eq!(
        two, one,
        "two overlapping contours did not render as the single rectangle they cover\n  \
         one   {one:?}\n  two   {two:?}",
    );

    let middle = &one[3..12];
    assert!(
        middle.iter().all(|&v| v == middle[0]),
        "the reference itself is not constant along a straight edge: {one:?}",
    );
}

// The same two contours apart, which the union must leave alone.
//
// Nested and disjoint contours are already an arrangement a winding accumulator gets right, and the
// resolver is gated on a winding of two or more so that it never runs for them. Without a control
// saying so, a resolver that quietly ran on every glyph would look identical here.
#[test]
fn contours_that_do_not_overlap_are_left_alone() {
    let apart = top_row(&|p| {
        rect(p, 0.0, 5.0, 0.0, 10.4);
        rect(p, 10.0, 15.0, 0.0, 10.4);
    });
    let gap = &apart[6..11];
    assert!(gap.iter().all(|&v| v == 0), "the gap between them filled in: {apart:?}");
    assert!(apart[3] > 0 && apart[13] > 0, "the two bars did not both draw: {apart:?}");
}
