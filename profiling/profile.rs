//! Fuel profile by Wasm function (profiling patch, not part of upstream wasmi).
//!
//! Every fuel charge the executor makes (the per-block `ConsumeFuel` instructions and the
//! dynamic charges of bulk-memory instructions) is attributed to the function whose instruction
//! pointer is being executed (self fuel) and to every function on the Wasm call stack
//! (inclusive fuel). Calls into Wasm functions are counted too.
//!
//! The profile is thread-local and off until [`enable`] is called. Function indices are the
//! engine's `CompiledFunc` indices, which for a single module compiled in one engine are the
//! module's defined-function indices (imports excluded).

use crate::engine::code_map::{CodeMap, InstructionPtr};
use crate::engine::stack::CallStack;
use alloc::vec::Vec;
use core::cell::RefCell;

#[derive(Debug, Default, Clone)]
pub struct FuncProfile {
    pub func: usize,
    pub self_fuel: u64,
    pub inclusive_fuel: u64,
    pub calls: u64,
}

#[derive(Default)]
struct State {
    enabled: bool,
    self_fuel: Vec<u64>,
    inclusive_fuel: Vec<u64>,
    calls: Vec<u64>,
    /// Scratch buffer for the distinct functions on the call stack.
    on_stack: Vec<usize>,
}

std::thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn grow(v: &mut Vec<u64>, idx: usize) {
    if v.len() <= idx {
        v.resize(idx + 1, 0);
    }
}

/// Starts (or restarts, clearing) the profile for the current thread.
pub fn enable() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        *s = State::default();
        s.enabled = true;
    });
}

/// Stops profiling and returns the per-function totals collected so far.
pub fn take() -> Vec<FuncProfile> {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.enabled = false;
        let n = s.self_fuel.len().max(s.inclusive_fuel.len()).max(s.calls.len());
        (0..n)
            .map(|i| FuncProfile {
                func: i,
                self_fuel: s.self_fuel.get(i).copied().unwrap_or(0),
                inclusive_fuel: s.inclusive_fuel.get(i).copied().unwrap_or(0),
                calls: s.calls.get(i).copied().unwrap_or(0),
            })
            .collect()
    })
}

#[inline]
pub(crate) fn enabled() -> bool {
    STATE.with(|s| s.borrow().enabled)
}

/// Records `amount` fuel charged while executing at `ip`.
#[inline]
pub(crate) fn record(code_map: &CodeMap, ip: InstructionPtr, call_stack: &CallStack, amount: u64) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if !s.enabled {
            return;
        }
        let Some(current) = code_map.func_at(ip) else { return };
        grow(&mut s.self_fuel, current);
        s.self_fuel[current] += amount;
        // Inclusive: the current function plus every distinct caller on the stack.
        let mut on_stack = core::mem::take(&mut s.on_stack);
        on_stack.clear();
        on_stack.push(current);
        for frame in call_stack.frames() {
            if let Some(f) = code_map.func_at(frame.ip()) {
                if !on_stack.contains(&f) {
                    on_stack.push(f);
                }
            }
        }
        for &f in &on_stack {
            grow(&mut s.inclusive_fuel, f);
            s.inclusive_fuel[f] += amount;
        }
        s.on_stack = on_stack;
    });
}

/// Records a call into the Wasm function `func` (a `CompiledFunc` index).
#[inline]
pub(crate) fn record_call(func: usize) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if !s.enabled {
            return;
        }
        grow(&mut s.calls, func);
        s.calls[func] += 1;
    });
}
