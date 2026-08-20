use super::*;

#[derive(Clone, Debug, Default)]
// A newtype and not an alias because f64 has no Eq or Hash. `canonical_axes` drops non-finite
// values first, so bit-pattern equality never has to handle NaN.
pub struct AxisKey(Vec<(String, f64)>);

impl PartialEq for AxisKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().zip(&other.0).all(|(a, b)| a.0 == b.0 && a.1.to_bits() == b.1.to_bits())
    }
}
impl Eq for AxisKey {}

// Ordered on the same bit patterns equality uses, since f64 has no Ord either. Not numerically
// meaningful for negatives – the patterns invert – but a map key needs only a total order that
// agrees with Eq.
impl Ord for AxisKey {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.0
            .iter()
            .map(|(tag, v)| (tag, v.to_bits()))
            .cmp(other.0.iter().map(|(tag, v)| (tag, v.to_bits())))
    }
}

impl PartialOrd for AxisKey {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl core::ops::Deref for AxisKey {
    type Target = [(String, f64)];
    fn deref(&self) -> &Self::Target { &self.0 }
}

pub fn canonical_axes<S: AsRef<str>>(axis_values: &[(S, f64)]) -> AxisKey {
    let mut map: alloc::collections::BTreeMap<String, f64> = alloc::collections::BTreeMap::new();
    for (tag, value) in axis_values {
        if !value.is_finite() { continue; }
        map.insert(tag.as_ref().to_string(), *value);
    }
    AxisKey(map.into_iter().collect())
}
