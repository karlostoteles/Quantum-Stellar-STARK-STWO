#!/usr/bin/env python3
"""Applies the fuel-profiling patch to a pristine copy of soroban-wasmi 0.31.1-soroban.29.0.0.

Usage: patch-wasmi.py <path to the copied crate>
The patch adds src/profile.rs (shipped next to this script) and hooks the two places where the
executor charges fuel, the Wasm call paths, and an environment override of the fuel costs.
Idempotent: a crate that is already patched is left alone.
"""
import pathlib
import shutil
import sys

root = pathlib.Path(sys.argv[1])
here = pathlib.Path(__file__).parent


def patch(rel, old, new, count=1):
    p = root / rel
    s = p.read_text()
    if new in s:
        return
    assert s.count(old) == count, f"{rel}: expected {count} match(es) of {old!r}, found {s.count(old)}"
    p.write_text(s.replace(old, new))


# The profile module itself.
shutil.copy(here / "profile.rs", root / "src" / "profile.rs")
patch("src/lib.rs", "mod engine;\n", "mod engine;\n#[cfg(feature = \"std\")]\npub mod profile;\n")

# Which function an instruction pointer belongs to (binary search on the function start offsets).
patch(
    "src/engine/code_map.rs",
    "    pub fn header(&self, func_body: CompiledFunc) -> &FuncHeader {",
    """    /// Profiling patch: the `CompiledFunc` index whose instructions contain `ip`.
    pub fn func_at(&self, ip: InstructionPtr) -> Option<usize> {
        let base = self.instrs.as_ptr() as usize;
        let p = ip.ptr as usize;
        if p < base || p >= base + self.instrs.len() * core::mem::size_of::<Instruction>() {
            return None;
        }
        let offset = (p - base) / core::mem::size_of::<Instruction>();
        // Function bodies are appended in index order, so start offsets grow with the index.
        let (mut lo, mut hi) = (0usize, self.headers.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.headers[mid].iref.to_usize() <= offset {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo.checked_sub(1)
    }

    pub fn header(&self, func_body: CompiledFunc) -> &FuncHeader {""",
)

# The call stack's frames, read-only.
patch(
    "src/engine/stack/frames.rs",
    "impl CallStack {\n",
    """impl CallStack {
    /// Profiling patch: the live frames, callers first.
    pub fn frames(&self) -> &[FuncFrame] {
        &self.frames
    }

""",
)

# Hooks in the executor: block fuel, dynamic fuel and Wasm calls.
patch(
    "src/engine/executor.rs",
    """        self.ctx.fuel_mut().consume_fuel(block_fuel.to_u64())?;
        self.try_next_instr()""",
    """        #[cfg(feature = "std")]
        crate::profile::record(self.code_map, self.ip, self.call_stack, block_fuel.to_u64());
        self.ctx.fuel_mut().consume_fuel(block_fuel.to_u64())?;
        self.try_next_instr()""",
)
patch(
    "src/engine/executor.rs",
    """        let delta = delta(self.fuel_costs());
        match mode {""",
    """        let delta = delta(self.fuel_costs());
        #[cfg(feature = "std")]
        crate::profile::record(self.code_map, self.ip, self.call_stack, delta);
        match mode {""",
)
patch(
    "src/engine/executor.rs",
    """        let header = self.code_map.header(func);
        self.value_stack.prepare_wasm_call(header)?;
        self.sp = self.value_stack.stack_ptr();
        self.ip = self.code_map.instr_ptr(header.iref());
        Ok(())""",
    """        #[cfg(feature = "std")]
        crate::profile::record_call(wasmi_arena::ArenaIndex::into_usize(func));
        let header = self.code_map.header(func);
        self.value_stack.prepare_wasm_call(header)?;
        self.sp = self.value_stack.stack_ptr();
        self.ip = self.code_map.instr_ptr(header.iref());
        Ok(())""",
)
patch(
    "src/engine/executor.rs",
    """            FuncEntity::Wasm(wasm_func) => {
                let header = self.code_map.header(wasm_func.func_body());""",
    """            FuncEntity::Wasm(wasm_func) => {
                #[cfg(feature = "std")]
                crate::profile::record_call(wasmi_arena::ArenaIndex::into_usize(wasm_func.func_body()));
                let header = self.code_map.header(wasm_func.func_body());""",
)

# Fuel cost override from the environment, to measure one instruction class at a time:
# WASMI_FUEL_COSTS=base,entity,load,store,call
patch(
    "src/engine/config.rs",
    """    pub fn set_fuel_costs(&mut self, costs: FuelCosts) -> &mut Self {
        self.fuel_costs = costs;
        self
    }""",
    """    pub fn set_fuel_costs(&mut self, costs: FuelCosts) -> &mut Self {
        self.fuel_costs = costs;
        #[cfg(feature = "std")]
        if let Ok(spec) = std::env::var("WASMI_FUEL_COSTS") {
            let v: Vec<u64> = spec.split(',').map(|x| x.trim().parse().expect("WASMI_FUEL_COSTS")).collect();
            assert_eq!(v.len(), 5, "WASMI_FUEL_COSTS=base,entity,load,store,call");
            self.fuel_costs.base = v[0];
            self.fuel_costs.entity = v[1];
            self.fuel_costs.load = v[2];
            self.fuel_costs.store = v[3];
            self.fuel_costs.call = v[4];
        }
        self
    }""",
)
print("patched", root)
