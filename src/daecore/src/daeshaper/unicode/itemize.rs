use alloc::vec::Vec;

use super::{script, Script};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScriptRun {
    pub start: usize,
    pub end: usize,
    pub script: Script,
}

pub fn script_runs(chars: &[char]) -> Vec<ScriptRun> {
    let mut out = Vec::new();
    if chars.is_empty() {
        return out;
    }

    let mut resolved: Vec<Script> = chars.iter().map(|&c| script(c)).collect();
    let mut carry: Option<Script> = None;
    for s in &mut resolved {
        match (s.is_context_dependent(), carry) {
            (true, Some(prev)) => *s = prev,
            (true, None) => {}
            (false, _) => carry = Some(*s),
        }
    }
    if let Some(first) = resolved.iter().position(|s| !s.is_context_dependent()) {
        let lead = resolved[first];
        for s in &mut resolved[..first] {
            *s = lead;
        }
    }

    let mut start = 0;
    for i in 1..resolved.len() {
        if resolved[i] != resolved[start] {
            out.push(ScriptRun { start, end: i, script: resolved[start] });
            start = i;
        }
    }
    out.push(ScriptRun { start, end: resolved.len(), script: resolved[start] });
    out
}
