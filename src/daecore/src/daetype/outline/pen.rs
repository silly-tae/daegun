pub trait OutlinePen {
    fn move_to(&mut self, x: f32, y: f32);
    fn line_to(&mut self, x: f32, y: f32);
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32);
    fn curve_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32);
    fn close(&mut self);
}

pub struct TransformPen<'a> {
    inner: &'a mut dyn OutlinePen,
    t: [f64; 6],
    translate_only: bool,
}

impl<'a> TransformPen<'a> {
    pub fn new(inner: &'a mut dyn OutlinePen, t: [f64; 6]) -> Self {
        let [a, b, c, d, _, _] = t;
        let translate_only = a == 1.0 && b == 0.0 && c == 0.0 && d == 1.0;
        TransformPen { inner, t, translate_only }
    }

    #[inline]
    fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let (x, y) = (x as f64, y as f64);
        let [a, b, c, d, dx, dy] = self.t;
        if self.translate_only {
            return ((x + dx) as f32, (y + dy) as f32);
        }
        ((x * a + y * c + dx) as f32, (x * b + y * d + dy) as f32)
    }
}

impl OutlinePen for TransformPen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.apply(x, y);
        self.inner.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.apply(x, y);
        self.inner.line_to(x, y);
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let (cx, cy) = self.apply(cx, cy);
        let (x, y) = self.apply(x, y);
        self.inner.quad_to(cx, cy, x, y);
    }
    fn curve_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        let (c1x, c1y) = self.apply(c1x, c1y);
        let (c2x, c2y) = self.apply(c2x, c2y);
        let (x, y) = self.apply(x, y);
        self.inner.curve_to(c1x, c1y, c2x, c2y, x, y);
    }
    fn close(&mut self) { self.inner.close(); }
}
